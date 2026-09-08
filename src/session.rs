//! A headless, thread-safe facade over the scanning engine.
//!
//! The TUI drives the engine directly from its render loop, which is fine when
//! the render loop is the only thing running. The Android overlay cannot: its
//! UI lives on the Java main thread, scans must not block it, and freezing has
//! to keep working while a scan is in flight. `Session` owns that concurrency
//! so the JNI layer stays a thin marshalling shim with no logic of its own.
//!
//! Everything durable lives here rather than in the front-end, which is what
//! lets an Android Activity be destroyed and recreated without losing a scan.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::address::{AddressEntry, AddressTable};
use crate::error::Result;
use crate::platform::{self, ProcessHandle};
use crate::scan::scanner::{ScanLimits, ScanProgress, ScanResult, Scanner};
use crate::scan::value_type::{ScanType, ScanValue, ValueType};

/// How often frozen values are re-written.
const FREEZE_INTERVAL: Duration = Duration::from_millis(100);
/// How often the freeze thread re-reads the memory map to decide which frozen
/// addresses are still backed by a writable mapping.
const MAP_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScanState {
    Idle = 0,
    Running = 1,
    Done = 2,
    Failed = 3,
}

impl ScanState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Running,
            2 => Self::Done,
            3 => Self::Failed,
            _ => Self::Idle,
        }
    }
}

/// One poll of the scan, cheap enough to call on a UI timer.
///
/// Every field comes from an atomic, so this never blocks behind a running
/// scan - which is the whole point of separating it from `results`.
#[derive(Debug, Clone, Copy)]
pub struct ScanStatus {
    pub state: ScanState,
    pub scanned_regions: usize,
    pub total_regions: usize,
    pub found: usize,
    pub truncated: bool,
}

/// What to scan for. Lifted out of the TUI's `ScannerView` so both front-ends
/// build the same request and parsing lives in one place.
#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub value_type: ValueType,
    pub scan_type: ScanType,
    /// Raw user input, parsed against `value_type`. `None` for the modes that
    /// compare against the previous pass.
    pub target_text: Option<String>,
    /// Start over rather than narrowing the current result set.
    pub restart: bool,
}

impl ScanRequest {
    pub fn new(value_type: ValueType, scan_type: ScanType) -> Self {
        Self {
            value_type,
            scan_type,
            target_text: None,
            restart: false,
        }
    }

    pub fn with_target(mut self, text: impl Into<String>) -> Self {
        self.target_text = Some(text.into());
        self
    }

    /// Parse the typed value, rejecting a mode that needs one but did not get
    /// a usable value.
    pub fn target(&self) -> Result<Option<ScanValue>> {
        if !self.scan_type.needs_target() {
            return Ok(None);
        }
        let text = self.target_text.as_deref().unwrap_or("").trim();
        ScanValue::parse(text, self.value_type)
            .map(Some)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} needs a valid {} value",
                    self.scan_type.label(),
                    self.value_type.label()
                )
            })
    }
}

pub struct Session {
    handle: Arc<dyn ProcessHandle>,
    scanner: Mutex<Scanner>,
    table: Mutex<AddressTable>,
    progress: Mutex<Option<Arc<ScanProgress>>>,
    state: AtomicU8,
    last_error: Mutex<Option<String>>,
    table_path: Mutex<Option<PathBuf>>,
    freeze_stop: Arc<AtomicBool>,
    freeze_thread: Mutex<Option<JoinHandle<()>>>,
}

impl Session {
    /// Attach to the calling process and start the freeze thread.
    ///
    /// Same-process is the Android case: the overlay is linked into the app it
    /// inspects, so there is no `ptrace`, no root, and nothing to ask the user
    /// for.
    pub fn attach_self() -> Result<Arc<Self>> {
        Self::with_handle(platform::attach_self()?)
    }

    pub fn with_handle(handle: Arc<dyn ProcessHandle>) -> Result<Arc<Self>> {
        let session = Arc::new(Self {
            handle,
            scanner: Mutex::new(Scanner::new()),
            table: Mutex::new(AddressTable::new()),
            progress: Mutex::new(None),
            state: AtomicU8::new(ScanState::Idle as u8),
            last_error: Mutex::new(None),
            table_path: Mutex::new(None),
            freeze_stop: Arc::new(AtomicBool::new(false)),
            freeze_thread: Mutex::new(None),
        });
        let handle = spawn_freeze_thread(&session);
        *session.freeze_thread.lock().unwrap() = Some(handle);
        Ok(session)
    }

    pub fn handle(&self) -> &Arc<dyn ProcessHandle> {
        &self.handle
    }

    /// Take the last error, clearing it. Errors are stored rather than thrown
    /// so the polling path never has to build an exception.
    pub fn take_last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().take()
    }

    fn set_error(&self, msg: String) {
        *self.last_error.lock().unwrap() = Some(msg);
    }

    /// Record an error raised by a caller (the JNI layer) so it reaches the UI
    /// through the same channel as an internal one.
    pub fn record_error(&self, msg: String) {
        self.set_error(msg);
    }

    pub fn set_limits(&self, limits: ScanLimits) {
        self.scanner.lock().unwrap().set_limits(limits);
    }

    // -- scanning ------------------------------------------------------------

    /// Kick off a scan on a background thread. Returns immediately; poll
    /// [`Self::status`] for progress.
    pub fn start_scan(self: &Arc<Self>, request: ScanRequest) -> Result<()> {
        if self.status().state == ScanState::Running {
            return Err(anyhow::anyhow!("A scan is already running"));
        }
        // Parse before spawning so a bad value is a synchronous error the UI
        // can put next to the input field.
        let target = request.target()?;

        // Zero, not a guess: the scanner publishes the real denominator as
        // soon as it has filtered the region list.
        let progress = Arc::new(ScanProgress::new(0));
        *self.progress.lock().unwrap() = Some(Arc::clone(&progress));
        self.state
            .store(ScanState::Running as u8, Ordering::Release);

        let session = Arc::clone(self);
        std::thread::Builder::new()
            .name("ce-scan".into())
            .spawn(move || {
                let outcome = {
                    let mut scanner = session.scanner.lock().unwrap();
                    if request.restart {
                        scanner.reset();
                    }
                    scanner.set_value_type(request.value_type);
                    let scan = if scanner.has_scanned() {
                        Scanner::next_scan
                    } else {
                        Scanner::first_scan
                    };
                    scan(
                        &mut scanner,
                        session.handle.as_ref(),
                        request.scan_type,
                        target.as_ref(),
                        Some(Arc::clone(&progress)),
                    )
                };

                match outcome {
                    Ok(_) => session.state.store(ScanState::Done as u8, Ordering::Release),
                    Err(e) => {
                        session.set_error(e.to_string());
                        session
                            .state
                            .store(ScanState::Failed as u8, Ordering::Release);
                    }
                }
            })?;

        Ok(())
    }

    pub fn status(&self) -> ScanStatus {
        let state = ScanState::from_u8(self.state.load(Ordering::Acquire));
        match self.progress.lock().unwrap().as_ref() {
            Some(p) => ScanStatus {
                state,
                scanned_regions: p.scanned_regions.load(Ordering::Relaxed),
                total_regions: p.total_regions.load(Ordering::Relaxed),
                found: p.found_count.load(Ordering::Relaxed),
                truncated: p.truncated.load(Ordering::Relaxed),
            },
            None => ScanStatus {
                state,
                scanned_regions: 0,
                total_regions: 0,
                found: 0,
                truncated: false,
            },
        }
    }

    pub fn cancel_scan(&self) {
        if let Some(p) = self.progress.lock().unwrap().as_ref() {
            p.cancelled.store(true, Ordering::Relaxed);
        }
    }

    pub fn reset_scan(&self) {
        if let Ok(mut scanner) = self.scanner.try_lock() {
            scanner.reset();
        }
        *self.progress.lock().unwrap() = None;
        self.state.store(ScanState::Idle as u8, Ordering::Release);
    }

    /// Number of results, or `None` while a scan holds the lock.
    pub fn result_count(&self) -> Option<usize> {
        self.scanner.try_lock().ok().map(|s| s.result_count())
    }

    /// A page of results. Empty while a scan is running rather than blocking
    /// the caller - the UI is expected to poll [`Self::status`] first.
    pub fn results_page(&self, offset: usize, limit: usize) -> Vec<ScanResult> {
        let Ok(scanner) = self.scanner.try_lock() else {
            return Vec::new();
        };
        scanner
            .results()
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn value_type(&self) -> ValueType {
        self.scanner
            .try_lock()
            .map(|s| s.value_type())
            .unwrap_or(ValueType::U32)
    }

    // -- direct memory access ------------------------------------------------

    pub fn read_bytes(&self, address: usize, len: usize) -> Result<Vec<u8>> {
        self.handle.read_memory(address, len)
    }

    pub fn read_value(&self, address: usize, value_type: ValueType) -> Result<ScanValue> {
        let bytes = self.handle.read_memory(address, value_type.size())?;
        ScanValue::from_bytes(&bytes, value_type)
            .ok_or_else(|| anyhow::anyhow!("Short read at 0x{address:X}"))
    }

    pub fn write_value_text(
        &self,
        address: usize,
        value_type: ValueType,
        text: &str,
    ) -> Result<()> {
        let value = ScanValue::parse(text.trim(), value_type)
            .ok_or_else(|| anyhow::anyhow!("'{text}' is not a valid {}", value_type.label()))?;
        self.handle.write_memory(address, &value.to_bytes())
    }

    // -- address table -------------------------------------------------------

    pub fn table_add(&self, address: usize, value_type: ValueType, description: String) {
        self.table
            .lock()
            .unwrap()
            .add(AddressEntry::new(address, value_type, description));
    }

    pub fn table_remove(&self, index: usize) {
        self.table.lock().unwrap().remove(index);
    }

    pub fn table_toggle_freeze(&self, index: usize) -> Result<()> {
        self.table
            .lock()
            .unwrap()
            .toggle_freeze(index, Some(self.handle.as_ref()))
    }

    pub fn table_set_value_text(&self, index: usize, text: &str) -> Result<()> {
        let mut table = self.table.lock().unwrap();
        let entry = table
            .entries
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("No entry {index}"))?;
        let value_type = entry.value_type;
        let value = ScanValue::parse(text.trim(), value_type)
            .ok_or_else(|| anyhow::anyhow!("'{text}' is not a valid {}", value_type.label()))?;
        table.write_value(self.handle.as_ref(), index, value)
    }

    pub fn table_len(&self) -> usize {
        self.table.lock().unwrap().entries.len()
    }

    /// Snapshot of the table with values refreshed from memory.
    pub fn table_entries(&self) -> Vec<AddressEntry> {
        let mut table = self.table.lock().unwrap();
        table.update_values(self.handle.as_ref());
        table.entries.clone()
    }

    pub fn table_save(&self, path: &str) -> Result<()> {
        self.table.lock().unwrap().save_to_file(path)?;
        *self.table_path.lock().unwrap() = Some(PathBuf::from(path));
        Ok(())
    }

    pub fn table_load(&self, path: &str) -> Result<()> {
        let loaded = AddressTable::load_from_file(path)?;
        *self.table.lock().unwrap() = loaded;
        *self.table_path.lock().unwrap() = Some(PathBuf::from(path));
        Ok(())
    }

    // -- teardown ------------------------------------------------------------

    /// Stop the freeze thread and wait for it.
    ///
    /// The JNI layer must call this before freeing the session: a live worker
    /// dereferencing a freed `Session` takes the host app down with it.
    pub fn shutdown(&self) {
        self.cancel_scan();
        self.freeze_stop.store(true, Ordering::Release);
        if let Some(handle) = self.freeze_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.freeze_stop.store(true, Ordering::Release);
        if let Some(handle) = self.freeze_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

/// The freeze loop.
///
/// Holds a `Weak` so it never keeps the session alive, and uses `try_lock` so
/// it silently skips a tick rather than queueing up behind the UI.
fn spawn_freeze_thread(session: &Arc<Session>) -> JoinHandle<()> {
    let weak = Arc::downgrade(session);
    let stop = Arc::clone(&session.freeze_stop);

    std::thread::Builder::new()
        .name("ce-freeze".into())
        .spawn(move || freeze_loop(weak, stop))
        .expect("freeze thread")
}

fn freeze_loop(weak: Weak<Session>, stop: Arc<AtomicBool>) {
    let mut writable: Vec<(usize, usize)> = Vec::new();
    let mut refreshed_at: Option<Instant> = None;

    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(FREEZE_INTERVAL);
        let Some(session) = weak.upgrade() else {
            return;
        };
        if stop.load(Ordering::Acquire) {
            return;
        }

        let stale = refreshed_at.is_none_or(|t| t.elapsed() >= MAP_REFRESH_INTERVAL);
        if stale {
            if let Ok(regions) = session.handle.memory_regions() {
                writable = regions
                    .iter()
                    .filter(|r| r.writable)
                    .map(|r| (r.base_address, r.base_address + r.size))
                    .collect();
                writable.sort_unstable();
            }
            refreshed_at = Some(Instant::now());
        }

        if let Ok(mut table) = session.table.try_lock() {
            if table.has_frozen() {
                table.write_frozen_values_within(session.handle.as_ref(), &writable);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The end-to-end shape the Android overlay uses: attach to our own
    /// process, then read and write our own memory through the engine.
    #[test]
    fn test_self_attach_reads_and_writes_own_memory() {
        let session = Session::attach_self().expect("attaching to self must work");
        let mut cell = Box::new(0xDEAD_BEEFu32);
        let ptr = &raw mut *cell;
        let address = ptr as usize;

        let read = session.read_value(address, ValueType::U32).unwrap();
        assert_eq!(read, ScanValue::U32(0xDEAD_BEEF));

        session
            .write_value_text(address, ValueType::U32, "4242")
            .unwrap();
        // Volatile: the value was changed from outside anything the compiler
        // can see, so a plain read is entitled to hand back the old one.
        assert_eq!(
            unsafe { std::ptr::read_volatile(ptr) },
            4242,
            "the write must land in our own variable"
        );

        session.shutdown();
    }

    #[test]
    fn test_freeze_thread_holds_a_value_down() {
        let session = Session::attach_self().unwrap();
        let mut cell = Box::new(1000u32);
        let ptr = &raw mut *cell;
        let address = ptr as usize;

        session.table_add(address, ValueType::U32, "demo".into());
        session.table_toggle_freeze(0).unwrap();

        // Stomp it the way the host app would, then let the freeze loop run.
        unsafe { std::ptr::write_volatile(ptr, 7) };
        std::thread::sleep(FREEZE_INTERVAL * 8);

        assert_eq!(
            unsafe { std::ptr::read_volatile(ptr) },
            1000,
            "the freeze thread should have put it back"
        );
        session.shutdown();
    }

    #[test]
    fn test_request_rejects_a_missing_target() {
        let request = ScanRequest::new(ValueType::U32, ScanType::ExactValue);
        assert!(request.target().is_err());

        let ok = ScanRequest::new(ValueType::U32, ScanType::ExactValue).with_target("42");
        assert_eq!(ok.target().unwrap(), Some(ScanValue::U32(42)));

        // Relative modes take no target and must not demand one.
        let relative = ScanRequest::new(ValueType::U32, ScanType::Increased);
        assert_eq!(relative.target().unwrap(), None);
    }

    #[test]
    fn test_shutdown_is_idempotent() {
        let session = Session::attach_self().unwrap();
        session.shutdown();
        session.shutdown();
    }
}
