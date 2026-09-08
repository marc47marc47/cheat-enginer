use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::address::{AddressEntry, AddressTable};
use crate::error::Result;
use crate::platform::{self, ProcessHandle, ProcessInfo};
use crate::process;
use crate::scan::scanner::{ScanProgress, Scanner};
use crate::scan::value_type::ScanValue;
use crate::session::ScanRequest;

use super::address_list_view::AddressListView;
use super::hex_viewer::HexViewer;
use super::process_list::ProcessListView;
use super::scanner_view::ScannerView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    ProcessList,
    Main,
    HexViewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainPanel {
    Scanner,
    AddressList,
}

/// Background scan result sent back via channel.
///
/// The handle is not carried back: it lives behind an `Arc`, so the scan
/// thread borrows it rather than taking it away, and freezing and hex reads
/// keep working for the duration of a scan.
struct ScanDone {
    scanner: Scanner,
    result: std::result::Result<usize, String>,
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// How often frozen values are re-written into the target process. Freezing is
/// a write-rate race against the target, so this has to be far shorter than the
/// value-display refresh below.
const FREEZE_INTERVAL: Duration = Duration::from_millis(100);

/// How often address table values are re-read for display.
const VALUE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// How often the hex window is re-read while it is on screen. Fast enough that
/// changed bytes light up, slow enough that scrolling is not a syscall storm.
const HEX_REFRESH_INTERVAL: Duration = Duration::from_millis(200);

/// How often the current scan is re-run while auto rescan is on.
pub const AUTO_SCAN_INTERVAL: Duration = Duration::from_millis(777);

pub struct App {
    pub screen: Screen,
    pub input_mode: InputMode,
    pub should_quit: bool,
    pub confirm_quit: bool,
    pub error_message: Option<(String, Instant)>,

    // Platform
    pub platform: Box<dyn platform::Platform>,
    pub process_handle: Option<Arc<dyn ProcessHandle>>,
    pub attached_process: Option<ProcessInfo>,

    // Process list
    pub process_list: Vec<ProcessInfo>,
    pub process_view: ProcessListView,

    // Scanner
    pub scanner: Scanner,
    pub scanner_view: ScannerView,
    pub scan_progress: Option<Arc<ScanProgress>>,

    // Background scan
    scanning: bool,
    scan_start: Option<Instant>,
    scan_receiver: Option<mpsc::Receiver<ScanDone>>,
    last_auto_scan: Instant,
    /// Whether the in-flight scan was started by the auto rescan timer rather
    /// than by the user - it decides whether the cursor is reset on completion.
    scan_was_auto: bool,

    // Address list
    pub address_table: AddressTable,
    pub address_view: AddressListView,
    last_value_update: Instant,
    last_freeze_write: Instant,
    values_dirty: bool,

    // Hex viewer
    pub hex_viewer: HexViewer,
    last_hex_read: Instant,

    // Main panel focus
    pub main_panel: MainPanel,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::ProcessList,
            input_mode: InputMode::Normal,
            should_quit: false,
            confirm_quit: false,
            error_message: None,
            platform: platform::create_platform(),
            process_handle: None,
            attached_process: None,
            process_list: Vec::new(),
            process_view: ProcessListView::new(),
            scanner: Scanner::new(),
            scanner_view: ScannerView::new(),
            scan_progress: None,
            scanning: false,
            scan_start: None,
            scan_receiver: None,
            last_auto_scan: Instant::now(),
            scan_was_auto: false,
            address_table: AddressTable::new(),
            address_view: AddressListView::new(),
            last_value_update: Instant::now(),
            last_freeze_write: Instant::now(),
            values_dirty: false,
            hex_viewer: HexViewer::new(),
            last_hex_read: Instant::now(),
            main_panel: MainPanel::Scanner,
        }
    }

    pub fn set_error(&mut self, msg: String) {
        self.error_message = Some((msg, Instant::now()));
    }

    pub fn refresh_processes(&mut self) {
        match self.platform.enumerate_processes() {
            Ok(procs) => self.process_list = procs,
            Err(e) => self.set_error(format!("Failed to enumerate processes: {e}")),
        }
    }

    pub fn attach_process(&mut self, pid: u32) {
        match self.platform.attach(pid) {
            Ok(handle) => {
                // Saved addresses are absolute and only meaningful for the
                // process they came from. Attaching elsewhere would leave the
                // freeze loop writing them into unrelated memory.
                let switching = self.attached_process.as_ref().map(|p| p.pid) != Some(pid);
                if switching {
                    let had_freezes = self.address_table.has_frozen();
                    self.address_table.clear_freezes();
                    if had_freezes {
                        self.set_error(
                            "Attached to a different process - freezes cleared".into(),
                        );
                    }
                }

                self.attached_process = self.process_list.iter().find(|p| p.pid == pid).cloned();
                self.process_handle = Some(handle);
                self.screen = Screen::Main;
                self.scanner.reset();
                self.scanner_view.auto_scan = false;
                self.values_dirty = true;
            }
            Err(e) => self.set_error(format!("Failed to attach: {e}")),
        }
    }

    /// Build the request the engine takes, from what the scanner pane is
    /// currently showing.
    ///
    /// The Android bridge builds the same struct from its own widgets, so
    /// parsing and validation of the typed value happen in exactly one place
    /// (`ScanRequest::target`) rather than once per front-end.
    fn scan_request(&self) -> ScanRequest {
        let mut request = ScanRequest::new(self.scanner.value_type(), self.scanner_view.scan_type());
        request.target_text = Some(self.scanner_view.value_input.clone());
        request
    }

    /// Why auto rescan cannot run right now, if anything. Checked both when the
    /// user arms it (so the key press gives immediate feedback) and on every
    /// tick (so it switches itself off instead of letting `do_scan` raise the
    /// same error every 777ms).
    fn auto_scan_blocker(&self) -> Option<&'static str> {
        if self.process_handle.is_none() {
            Some("no process attached")
        } else if !self.scanner.has_scanned() {
            Some("run a first scan first")
        } else if self.scanner.result_count() == 0 && !self.scanner.snapshot_pending() {
            // A pending Unknown-Initial snapshot also has zero results, but the
            // next scan builds the candidate list from it - that is exactly the
            // unknown-value workflow, so it must not be treated as exhausted.
            Some("no results left")
        } else if self.scan_request().target().is_err() {
            Some("this scan mode needs a valid value")
        } else {
            None
        }
    }

    /// Toggle the auto rescan timer. Combined with the Unchanged scan mode this
    /// repeatedly drops every address that moved, narrowing the result set down
    /// to values that are holding steady.
    fn toggle_auto_scan(&mut self) {
        if self.scanner_view.auto_scan {
            self.scanner_view.auto_scan = false;
            return;
        }
        if let Some(reason) = self.auto_scan_blocker() {
            self.set_error(format!("Cannot start auto rescan: {reason}"));
            return;
        }
        self.scanner_view.auto_scan = true;
        self.last_auto_scan = Instant::now();
    }

    /// Re-run the current scan on a fixed interval.
    fn poll_auto_scan(&mut self) {
        if !self.scanner_view.auto_scan || self.scanning {
            return;
        }
        if self.last_auto_scan.elapsed() < AUTO_SCAN_INTERVAL {
            return;
        }

        if let Some(reason) = self.auto_scan_blocker() {
            self.scanner_view.auto_scan = false;
            self.set_error(format!("Auto rescan off: {reason}"));
            return;
        }

        self.last_auto_scan = Instant::now();
        self.scan_was_auto = true;
        self.do_scan();
    }

    pub fn do_scan(&mut self) {
        if self.scanning {
            return;
        }

        let Some(handle) = self.process_handle.clone() else {
            self.set_error("No process attached".into());
            return;
        };

        let request = self.scan_request();
        let scan_type = request.scan_type;
        let value_type = request.value_type;

        let target = match request.target() {
            Ok(target) => target,
            Err(e) => {
                self.set_error(e.to_string());
                return;
            }
        };

        let progress = Arc::new(ScanProgress::new(0));
        self.scan_progress = Some(Arc::clone(&progress));
        self.scanning = true;
        self.scan_start = Some(Instant::now());

        let mut scanner = std::mem::replace(&mut self.scanner, Scanner::new());
        self.scanner.set_value_type(value_type);

        let (tx, rx) = mpsc::channel();
        self.scan_receiver = Some(rx);

        std::thread::spawn(move || {
            let result = if scanner.has_scanned() {
                scanner.next_scan(handle.as_ref(), scan_type, target.as_ref(), Some(progress))
            } else {
                scanner.first_scan(handle.as_ref(), scan_type, target.as_ref(), Some(progress))
            };

            let _ = tx.send(ScanDone {
                scanner,
                result: result.map_err(|e| e.to_string()),
            });
        });
    }

    fn poll_scan(&mut self) {
        if !self.scanning {
            return;
        }

        let Some(ref rx) = self.scan_receiver else {
            return;
        };

        match rx.try_recv() {
            Ok(done) => {
                self.scanner = done.scanner;
                self.scanning = false;
                self.scan_start = None;
                self.scan_progress = None;
                self.scan_receiver = None;

                let was_auto = std::mem::take(&mut self.scan_was_auto);
                self.last_auto_scan = Instant::now();

                match done.result {
                    Ok(count) => {
                        if was_auto {
                            // Keep the cursor where the user left it - resetting
                            // it every 777ms would make the list unbrowsable.
                            self.scanner_view.result_selected = self
                                .scanner_view
                                .result_selected
                                .min(count.saturating_sub(1));
                        } else {
                            self.scanner_view.result_scroll = 0;
                            self.scanner_view.result_selected = 0;
                        }

                        if count == 0 && self.scanner.has_scanned() {
                            if self.scanner_view.auto_scan {
                                self.scanner_view.auto_scan = false;
                                self.set_error("Auto rescan off: no results left".into());
                            } else {
                                self.set_error("No results found".into());
                            }
                        }
                    }
                    Err(e) => {
                        self.scanner_view.auto_scan = false;
                        self.set_error(format!("Scan error: {e}"));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.scanning = false;
                self.scan_start = None;
                self.scan_progress = None;
                self.scan_receiver = None;
                self.scan_was_auto = false;
                self.scanner_view.auto_scan = false;
                self.set_error("Scan thread crashed".into());
            }
        }
    }

    pub fn add_selected_to_address_table(&mut self) {
        let results = self.scanner.results();
        let idx = self.scanner_view.result_selected;
        if idx < results.len() {
            let result = &results[idx];
            let entry = AddressEntry::new(
                result.address,
                self.scanner.value_type(),
                String::new(),
            );
            self.address_table.add(entry);
        }
    }

    /// Get the address currently selected in the active panel (scanner results or address table)
    fn selected_address(&self) -> Option<usize> {
        match self.main_panel {
            MainPanel::Scanner => {
                let results = self.scanner.results();
                results.get(self.scanner_view.result_selected).map(|r| r.address)
            }
            MainPanel::AddressList => {
                self.address_table.entries.get(self.address_view.selected).map(|e| e.address)
            }
        }
    }

    /// Called once per main-loop iteration. Freezes are re-applied first so the
    /// values read for display are the post-freeze ones, otherwise the table
    /// flickers between the target's value and the locked one.
    pub fn tick(&mut self) {
        self.apply_freezes();
        self.poll_auto_scan();
        self.update_values();
        self.refresh_hex();
    }

    /// Re-read the hex window.
    ///
    /// This used to happen inside `HexViewer::draw`, which meant memory was
    /// read as a side effect of terminal layout. Doing it here keeps rendering
    /// free of I/O and gives the byte-change highlighting a fixed cadence.
    fn refresh_hex(&mut self) {
        if self.screen != Screen::HexViewer {
            return;
        }
        if self.last_hex_read.elapsed() < HEX_REFRESH_INTERVAL {
            return;
        }
        self.last_hex_read = Instant::now();

        if let Some(ref handle) = self.process_handle {
            let _ = self.hex_viewer.model.refresh(handle.as_ref());
        }
    }

    /// Re-write frozen values on a short interval. Write failures are recorded
    /// per entry (rendered as a marker in the address table) instead of pushed
    /// to the error banner - at this rate they would drown out real errors.
    fn apply_freezes(&mut self) {
        if self.last_freeze_write.elapsed() < FREEZE_INTERVAL {
            return;
        }
        self.last_freeze_write = Instant::now();

        if !self.address_table.has_frozen() {
            return;
        }

        if let Some(ref handle) = self.process_handle {
            self.address_table.write_frozen_values(handle.as_ref());
        }
    }

    pub fn update_values(&mut self) {
        let should_update =
            self.values_dirty || self.last_value_update.elapsed() >= VALUE_REFRESH_INTERVAL;

        if !should_update {
            return;
        }

        if let Some(ref handle) = self.process_handle {
            self.address_table.update_values(handle.as_ref());
        }

        self.last_value_update = Instant::now();
        self.values_dirty = false;
    }

    pub fn handle_events(&mut self, timeout: Duration) -> Result<()> {
        self.poll_scan();

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.handle_key(key);
                }
            }
        }

        if let Some((_, instant)) = &self.error_message {
            if instant.elapsed() > Duration::from_secs(5) {
                self.error_message = None;
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits immediately
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Quit confirmation dialog
        if self.confirm_quit {
            match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                _ => self.confirm_quit = false, // any other key cancels
            }
            return;
        }

        // Block most input while scanning
        if self.scanning {
            if key.code == KeyCode::Esc {
                self.confirm_quit = true;
            }
            return;
        }

        if self.input_mode == InputMode::Editing {
            self.handle_editing_key(key);
            return;
        }

        // Esc triggers quit confirmation on all screens
        if key.code == KeyCode::Esc {
            self.confirm_quit = true;
            return;
        }

        // Global F-key shortcuts
        match key.code {
            KeyCode::F(1) => {
                self.screen = Screen::ProcessList;
                self.refresh_processes();
                return;
            }
            KeyCode::F(2) => {
                if self.process_handle.is_some() {
                    self.screen = Screen::Main;
                }
                return;
            }
            KeyCode::F(3) => {
                if self.process_handle.is_some() {
                    // If on Main screen, jump hex viewer to selected address
                    if self.screen == Screen::Main {
                        if let Some(addr) = self.selected_address() {
                            self.hex_viewer.set_address(addr);
                        }
                    }
                    self.screen = Screen::HexViewer;
                }
                return;
            }
            _ => {}
        }

        match self.screen {
            Screen::ProcessList => self.handle_process_list_key(key),
            Screen::Main => self.handle_main_key(key),
            Screen::HexViewer => self.handle_hex_viewer_key(key),
        }
    }

    fn handle_process_list_key(&mut self, key: KeyEvent) {
        let filtered_len = process::filter_processes(&self.process_list, &self.process_view.filter_input).len();
        match key.code {
            KeyCode::Up => {
                if self.process_view.selected > 0 {
                    self.process_view.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.process_view.selected + 1 < filtered_len {
                    self.process_view.selected += 1;
                }
            }
            KeyCode::PageUp => {
                self.process_view.selected = self.process_view.selected.saturating_sub(20);
            }
            KeyCode::PageDown => {
                self.process_view.selected = (self.process_view.selected + 20).min(filtered_len.saturating_sub(1));
            }
            KeyCode::Enter => {
                let filtered = process::filter_processes(&self.process_list, &self.process_view.filter_input);
                if let Some(proc) = filtered.get(self.process_view.selected) {
                    let pid = proc.pid;
                    self.attach_process(pid);
                }
            }
            KeyCode::F(5) => self.refresh_processes(),
            KeyCode::Backspace => {
                self.process_view.filter_input.pop();
                self.process_view.selected = 0;
                self.process_view.scroll_offset = 0;
            }
            KeyCode::Char(c) => {
                self.process_view.filter_input.push(c);
                self.process_view.selected = 0;
                self.process_view.scroll_offset = 0;
            }
            _ => {}
        }
        // Keep selected in bounds after filter change
        let new_filtered_len = process::filter_processes(&self.process_list, &self.process_view.filter_input).len();
        if self.process_view.selected >= new_filtered_len && new_filtered_len > 0 {
            self.process_view.selected = new_filtered_len - 1;
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                self.main_panel = match self.main_panel {
                    MainPanel::Scanner => MainPanel::AddressList,
                    MainPanel::AddressList => MainPanel::Scanner,
                };
            }
            _ => match self.main_panel {
                MainPanel::Scanner => self.handle_scanner_key(key),
                MainPanel::AddressList => self.handle_address_key(key),
            },
        }
    }

    fn handle_scanner_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.scanner_view.result_selected > 0 {
                    self.scanner_view.result_selected -= 1;
                }
            }
            KeyCode::Down => {
                let count = self.scanner.result_count();
                if self.scanner_view.result_selected + 1 < count {
                    self.scanner_view.result_selected += 1;
                }
            }
            KeyCode::Char('v') | KeyCode::Char('e') => {
                self.input_mode = InputMode::Editing;
                self.scanner_view.editing_value = true;
            }
            KeyCode::Char('t') => {
                self.scanner_view.cycle_value_type();
                self.scanner.set_value_type(self.scanner_view.value_type());
            }
            KeyCode::Char('s') => {
                self.scanner_view.cycle_scan_type();
            }
            KeyCode::Enter => {
                self.do_scan();
            }
            KeyCode::Char('r') => {
                self.scanner.reset();
                self.scanner_view.result_selected = 0;
                self.scanner_view.result_scroll = 0;
                self.scanner_view.auto_scan = false;
            }
            KeyCode::Char('A') => {
                self.toggle_auto_scan();
            }
            KeyCode::Char('a') => {
                self.add_selected_to_address_table();
            }
            _ => {}
        }
    }

    fn handle_address_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.address_view.selected > 0 {
                    self.address_view.selected -= 1;
                    self.values_dirty = true;
                }
            }
            KeyCode::Down => {
                if self.address_view.selected + 1 < self.address_table.entries.len() {
                    self.address_view.selected += 1;
                    self.values_dirty = true;
                }
            }
            KeyCode::Char('f') => {
                let handle = self.process_handle.as_deref();
                if let Err(e) = self
                    .address_table
                    .toggle_freeze(self.address_view.selected, handle)
                {
                    self.set_error(e.to_string());
                }
            }
            KeyCode::Delete => {
                if !self.address_table.entries.is_empty() {
                    self.address_table.remove(self.address_view.selected);
                    if self.address_view.selected >= self.address_table.entries.len()
                        && self.address_view.selected > 0
                    {
                        self.address_view.selected -= 1;
                    }
                }
            }
            KeyCode::Char('e') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_value = true;
                self.address_view.edit_buffer.clear();
            }
            KeyCode::Char('d') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_description = true;
                if let Some(entry) = self.address_table.entries.get(self.address_view.selected) {
                    self.address_view.edit_buffer = entry.description.clone();
                }
            }
            KeyCode::Char('S') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_save_path = true;
                self.address_view.edit_buffer = "cheat_table.json".to_string();
            }
            KeyCode::Char('L') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_load_path = true;
                self.address_view.edit_buffer = "cheat_table.json".to_string();
            }
            _ => {}
        }
    }

    fn handle_hex_viewer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.hex_viewer.scroll_up(),
            KeyCode::Down => self.hex_viewer.scroll_down(),
            KeyCode::PageUp => self.hex_viewer.page_up(),
            KeyCode::PageDown => self.hex_viewer.page_down(),
            KeyCode::Char('g') => {
                self.input_mode = InputMode::Editing;
                self.hex_viewer.editing_address = true;
                self.hex_viewer.address_input.clear();
            }
            _ => {}
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.clear_editing_state();
            }
            KeyCode::Enter => {
                self.commit_edit();
                self.input_mode = InputMode::Normal;
                self.clear_editing_state();
            }
            KeyCode::Backspace => {
                self.active_edit_buffer_mut().map(|b| { b.pop(); });
            }
            KeyCode::Char(c) => {
                self.active_edit_buffer_mut().map(|b| b.push(c));
            }
            _ => {}
        }
    }

    fn active_edit_buffer_mut(&mut self) -> Option<&mut String> {
        if self.process_view.editing_filter {
            return Some(&mut self.process_view.filter_input);
        }
        if self.scanner_view.editing_value {
            return Some(&mut self.scanner_view.value_input);
        }
        if self.address_view.editing_value
            || self.address_view.editing_description
            || self.address_view.editing_save_path
            || self.address_view.editing_load_path
        {
            return Some(&mut self.address_view.edit_buffer);
        }
        if self.hex_viewer.editing_address {
            return Some(&mut self.hex_viewer.address_input);
        }
        None
    }

    fn commit_edit(&mut self) {
        if self.process_view.editing_filter {
            self.process_view.selected = 0;
            return;
        }
        if self.hex_viewer.editing_address {
            if let Ok(addr) = usize::from_str_radix(&self.hex_viewer.address_input, 16) {
                self.hex_viewer.set_address(addr);
            } else {
                self.set_error("Invalid hex address".into());
            }
            return;
        }
        if self.address_view.editing_value {
            let idx = self.address_view.selected;
            if let Some(entry) = self.address_table.entries.get(idx) {
                let vt = entry.value_type;
                if let Some(val) = ScanValue::parse(&self.address_view.edit_buffer, vt) {
                    if let Some(ref handle) = self.process_handle {
                        if let Err(e) = self.address_table.write_value(handle.as_ref(), idx, val) {
                            self.set_error(format!("Write error: {e}"));
                        }
                    }
                } else {
                    self.set_error("Invalid value".into());
                }
            }
            return;
        }
        if self.address_view.editing_description {
            let idx = self.address_view.selected;
            if let Some(entry) = self.address_table.entries.get_mut(idx) {
                entry.description = self.address_view.edit_buffer.clone();
            }
            return;
        }
        if self.address_view.editing_save_path {
            let path = self.address_view.edit_buffer.clone();
            if let Err(e) = self.address_table.save_to_file(&path) {
                self.set_error(format!("Save error: {e}"));
            }
            return;
        }
        if self.address_view.editing_load_path {
            let path = self.address_view.edit_buffer.clone();
            match AddressTable::load_from_file(&path) {
                Ok(table) => self.address_table = table,
                Err(e) => self.set_error(format!("Load error: {e}")),
            }
            return;
        }
    }

    fn clear_editing_state(&mut self) {
        self.process_view.editing_filter = false;
        self.scanner_view.editing_value = false;
        self.address_view.editing_value = false;
        self.address_view.editing_description = false;
        self.address_view.editing_save_path = false;
        self.address_view.editing_load_path = false;
        self.hex_viewer.editing_address = false;
    }

    fn spinner_frame(&self) -> &'static str {
        let elapsed = self.scan_start.map(|s| s.elapsed()).unwrap_or_default();
        let idx = (elapsed.as_millis() / 80) as usize % SPINNER_FRAMES.len();
        SPINNER_FRAMES[idx]
    }

    fn scan_elapsed_str(&self) -> String {
        let elapsed = self.scan_start.map(|s| s.elapsed()).unwrap_or_default();
        let secs = elapsed.as_secs_f64();
        if secs < 1.0 {
            format!("{:.0}ms", elapsed.as_millis())
        } else {
            format!("{secs:.1}s")
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let size = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(size);

        // Status bar at top
        self.draw_status_bar(frame, chunks[0]);

        match self.screen {
            Screen::ProcessList => {
                let filtered = process::filter_processes(&self.process_list, &self.process_view.filter_input);
                // list area height = chunks[1].height - search bar(3) - borders(2)
                let visible_rows = chunks[1].height.saturating_sub(5) as usize;
                self.process_view.ensure_visible(visible_rows, filtered.len());
                self.process_view.draw(frame, chunks[1], &filtered, self.input_mode);
            }
            Screen::Main => {
                // Side by side: scanner left, address table right.
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(chunks[1]);

                // Pane heights are only known here, so the scroll windows are
                // corrected right before rendering.
                self.scanner_view
                    .ensure_visible(main_chunks[0], self.scanner.result_count());
                self.address_view.ensure_visible(
                    main_chunks[1],
                    self.input_mode,
                    self.address_table.entries.len(),
                );

                self.scanner_view.draw(
                    frame,
                    main_chunks[0],
                    &self.scanner,
                    self.input_mode,
                    self.main_panel == MainPanel::Scanner,
                    self.scanning,
                    self.spinner_frame(),
                    &self.scan_elapsed_str(),
                    self.scan_progress.as_ref(),
                );
                self.address_view.draw(
                    frame,
                    main_chunks[1],
                    &self.address_table,
                    self.input_mode,
                    self.main_panel == MainPanel::AddressList,
                );
            }
            Screen::HexViewer => {
                self.hex_viewer.draw(frame, chunks[1], self.input_mode);
            }
        }

        // Quit confirmation overlay
        if self.confirm_quit {
            self.draw_quit_dialog(frame, size);
        }
    }

    fn draw_quit_dialog(&self, frame: &mut Frame, area: Rect) {
        let dialog_width = 40u16;
        let dialog_height = 5u16;
        let x = area.width.saturating_sub(dialog_width) / 2;
        let y = area.height.saturating_sub(dialog_height) / 2;
        let dialog_area = Rect::new(x, y, dialog_width.min(area.width), dialog_height.min(area.height));

        frame.render_widget(Clear, dialog_area);

        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Quit? ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("Quit  "),
                Span::styled("[c] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Cancel"),
            ]),
        ];

        let dialog = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Confirm Exit "),
        );
        frame.render_widget(dialog, dialog_area);
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![];

        if let Some(ref proc) = self.attached_process {
            spans.push(Span::styled(
                format!(" [{}:{}] ", proc.name, proc.pid),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                " [No process] ",
                Style::default().fg(Color::DarkGray),
            ));
        }

        if self.scanning {
            let spinner = self.spinner_frame();
            let elapsed = self.scan_elapsed_str();
            spans.push(Span::styled(
                format!(" {spinner} Scanning... ({elapsed}) "),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        } else {
            // The pane titles are too narrow at half width to carry the key
            // hints, so they live here and follow the focused panel.
            let help = match self.screen {
                Screen::ProcessList => "F5:Refresh Enter:Attach Esc:Quit | Type to filter",
                Screen::Main => match self.main_panel {
                    MainPanel::Scanner => {
                        "F1:Proc F3:Hex Tab:Panel | t:Type s:Mode v:Value Enter:Scan r:Reset A:Auto a:Add"
                    }
                    MainPanel::AddressList => {
                        "F1:Proc F3:Hex Tab:Panel | f:Freeze e:Edit d:Desc S:Save L:Load Del:Remove"
                    }
                },
                Screen::HexViewer => "F1:Proc F2:Main F3:Hex g:GoTo Up/Down:Scroll",
            };
            spans.push(Span::styled(
                format!(" {help}"),
                Style::default().fg(Color::DarkGray),
            ));
        }

        if let Some((ref msg, _)) = self.error_message {
            spans.push(Span::styled(
                format!("  ERR: {msg}"),
                Style::default().fg(Color::Red),
            ));
        }

        // Version stamp is pinned right, and only gets whatever columns the
        // status text does not need - it must never truncate the key hints or
        // an error message.
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let free = (area.width as usize).saturating_sub(used);
        let full = format!(" v{} {} ", env!("CARGO_PKG_VERSION"), env!("BUILD_DATE"));
        let short = concat!(" v", env!("CARGO_PKG_VERSION"), " ").to_string();

        let (area, version) = if free >= full.chars().count() {
            (area, Some(full))
        } else if free >= short.chars().count() {
            (area, Some(short))
        } else {
            (area, None)
        };

        let bar_area = if let Some(ref version) = version {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(version.chars().count() as u16),
                ])
                .split(area);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    version.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
                chunks[1],
            );
            chunks[0]
        } else {
            area
        };

        let bar = Paragraph::new(Line::from(spans));
        frame.render_widget(bar, bar_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::MemoryRegion;
    use crate::scan::value_type::{ScanType, ValueType};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// One small writable region, enough to drive a real scan.
    struct FakeHandle {
        data: Vec<u8>,
    }

    const FAKE_BASE: usize = 0x1000;

    impl FakeHandle {
        fn new() -> Self {
            Self {
                data: 7u32.to_le_bytes().repeat(8),
            }
        }
    }

    impl ProcessHandle for FakeHandle {
        fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>> {
            let offset = address
                .checked_sub(FAKE_BASE)
                .ok_or_else(|| anyhow::anyhow!("out of range"))?;
            if offset + size > self.data.len() {
                return Err(anyhow::anyhow!("out of range"));
            }
            Ok(self.data[offset..offset + size].to_vec())
        }

        fn write_memory(&self, _address: usize, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
            Ok(vec![MemoryRegion {
                base_address: FAKE_BASE,
                size: self.data.len(),
                readable: true,
                writable: true,
                path: None,
            }])
        }
    }

    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn test_main_screen_is_split_side_by_side() {
        let mut app = App::new();
        app.screen = Screen::Main;
        app.address_table
            .add(AddressEntry::new(0x1000, ValueType::U32, "hp".into()));

        let lines = render(&mut app, 100, 24);
        let joined = lines.join("\n");
        assert!(joined.contains("Results"), "scanner pane must render");
        assert!(joined.contains("Addresses"), "address pane must render");

        // The address table must start in the right-hand half.
        let addr_line = lines
            .iter()
            .find(|l| l.contains("Addresses"))
            .expect("address pane title");
        assert!(
            addr_line.find("Addresses").unwrap() >= 50,
            "address table belongs on the right, found at column {}",
            addr_line.find("Addresses").unwrap()
        );

        // The scanner controls must be on the left of the same row.
        let type_line = lines.iter().find(|l| l.contains("[t]ype")).unwrap();
        assert!(type_line.find("[t]ype").unwrap() < 50);
    }

    #[test]
    fn test_scanner_pane_fits_at_half_width() {
        let mut app = App::new();
        app.screen = Screen::Main;
        // 80 columns wide leaves the scanner 40 - the tightest realistic case.
        let lines = render(&mut app, 80, 24);
        let joined = lines.join("\n");
        // Both control widgets must still be legible rather than clipped away.
        assert!(joined.contains("[t]ype"), "value type control clipped");
        assert!(joined.contains("[s]can mode"), "scan mode control clipped");
        assert!(joined.contains("4 Bytes (u32)"), "value type label clipped");
    }

    /// Not an assertion - run with `cargo test -- --ignored --nocapture` to
    /// eyeball the layout after changing pane constraints.
    #[test]
    #[ignore]
    fn dump_main_screen() {
        let mut app = App::new();
        app.screen = Screen::Main;
        app.scanner_view.value_input = "100".into();
        app.scanner_view.auto_scan = true;
        for i in 0..3 {
            app.address_table.add(AddressEntry::new(
                0x7FF6A1B23040 + i * 0x140,
                ValueType::U32,
                format!("entry {i}"),
            ));
        }
        app.address_table.entries[1].frozen = true;
        for line in render(&mut app, 100, 20) {
            println!("{line}");
        }
    }

    #[test]
    fn test_status_bar_shows_version_and_build_date() {
        let mut app = App::new();
        app.screen = Screen::Main;
        let lines = render(&mut app, 140, 10);
        assert!(
            lines[0].contains(env!("CARGO_PKG_VERSION")),
            "version missing: {}",
            lines[0]
        );
        assert!(
            lines[0].contains(env!("BUILD_DATE")),
            "build date missing: {}",
            lines[0]
        );
    }

    #[test]
    fn test_status_bar_gives_help_text_priority_over_version() {
        let mut app = App::new();
        app.screen = Screen::Main;
        // Too narrow for both - the version stamp yields its columns rather
        // than truncating the key hints.
        let lines = render(&mut app, 100, 10);
        assert!(
            lines[0].contains("A:Auto a:Add"),
            "help truncated: {}",
            lines[0]
        );
    }

    #[test]
    fn test_auto_scan_needs_a_first_scan() {
        let mut app = App::new();
        assert!(!app.scanner.has_scanned());

        app.toggle_auto_scan();
        assert!(!app.scanner_view.auto_scan, "must not arm before a first scan");
        assert!(app.error_message.is_some());
    }

    #[test]
    fn test_auto_scan_toggles_off_without_a_first_scan_check() {
        let mut app = App::new();
        // Armed by hand, as it would be after a real scan.
        app.scanner_view.auto_scan = true;
        app.toggle_auto_scan();
        assert!(!app.scanner_view.auto_scan);
    }

    #[test]
    fn test_auto_scan_disarms_when_nothing_left_to_scan() {
        let mut app = App::new();
        app.scanner_view.auto_scan = true;
        // Force the interval to have elapsed.
        app.last_auto_scan = Instant::now() - AUTO_SCAN_INTERVAL - Duration::from_millis(1);

        // No process attached is the first blocker checked.
        app.poll_auto_scan();
        assert!(!app.scanner_view.auto_scan);
        let (msg, _) = app.error_message.clone().expect("reason reported");
        assert!(msg.contains("Auto rescan off"), "got: {msg}");
    }

    #[test]
    fn test_auto_scan_survives_a_pending_unknown_initial_snapshot() {
        let mut app = App::new();
        // Unknown Initial captures memory but produces no candidate list yet.
        // That is the unknown-value workflow, not an exhausted scan.
        app.scanner
            .first_scan(&FakeHandle::new(), ScanType::UnknownInitial, None, None)
            .unwrap();
        assert_eq!(app.scanner.result_count(), 0);
        assert!(app.scanner.snapshot_pending());

        app.process_handle = Some(Arc::new(FakeHandle::new()));
        while app.scanner_view.scan_type() != ScanType::Unchanged {
            app.scanner_view.cycle_scan_type();
        }
        app.scanner_view.auto_scan = true;
        app.last_auto_scan = Instant::now() - AUTO_SCAN_INTERVAL - Duration::from_millis(1);

        app.poll_auto_scan();
        assert!(
            app.scanner_view.auto_scan,
            "must not disarm while a snapshot is pending"
        );
        assert!(app.error_message.is_none(), "no error should be raised");
    }

    #[test]
    fn test_auto_scan_disarms_when_results_are_truly_exhausted() {
        let mut app = App::new();
        app.process_handle = Some(Arc::new(FakeHandle::new()));
        // has_scanned but neither results nor a snapshot - nothing to repeat.
        app.scanner
            .first_scan(
                &FakeHandle::new(),
                ScanType::ExactValue,
                Some(&ScanValue::U32(999)),
                None,
            )
            .unwrap();
        assert_eq!(app.scanner.result_count(), 0);
        assert!(!app.scanner.snapshot_pending());

        while app.scanner_view.scan_type() != ScanType::Unchanged {
            app.scanner_view.cycle_scan_type();
        }
        app.scanner_view.auto_scan = true;
        app.last_auto_scan = Instant::now() - AUTO_SCAN_INTERVAL - Duration::from_millis(1);
        app.poll_auto_scan();
        assert!(!app.scanner_view.auto_scan);
    }

    #[test]
    fn test_auto_scan_does_not_flood_the_error_banner() {
        let mut app = App::new();
        app.scanner_view.auto_scan = true;
        app.last_auto_scan = Instant::now() - AUTO_SCAN_INTERVAL - Duration::from_millis(1);

        app.poll_auto_scan();
        let first = app.error_message.clone().unwrap().1;

        // Second tick: already disarmed, so nothing new is raised and the
        // banner keeps its original timestamp instead of being refreshed.
        app.poll_auto_scan();
        assert_eq!(app.error_message.clone().unwrap().1, first);
    }
}
