//! `/proc`-based process access, shared by desktop Linux and Android.
//!
//! Reads and writes go through `pread`/`pwrite` on a single cached
//! `/proc/<pid>/mem` file descriptor. That keeps the calls `&self`-only and
//! free of seek races, so one handle can be shared by the scan thread, the
//! freeze thread and the UI at the same time - the previous `seek` + `read`
//! pair was actively racy the moment two threads touched it.

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::sync::Arc;

use super::maps::parse_maps;
use super::{MemoryRegion, Platform, ProcessHandle, ProcessInfo};
use crate::error::Result;

/// Largest single `pread`. Android mappings routinely run to hundreds of
/// megabytes and `Scanner::first_scan` asks for a whole region at a time;
/// pulling one down in a single syscall is an easy way to trip the host app's
/// allocator.
const MAX_CHUNK: usize = 4 * 1024 * 1024;

pub struct LinuxPlatform;

impl LinuxPlatform {
    /// Attach to the calling process.
    ///
    /// This is the Android case: an overlay embedded in an app scans the app it
    /// lives in, so the target is same-UID and needs neither `ptrace` nor root.
    pub fn attach_self(&self) -> Result<Arc<dyn ProcessHandle>> {
        self.attach(std::process::id())
    }
}

impl Platform for LinuxPlatform {
    fn enumerate_processes(&self) -> Result<Vec<ProcessInfo>> {
        let mut processes = Vec::new();
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Ok(pid) = name_str.parse::<u32>() {
                let comm_path = format!("/proc/{pid}/comm");
                if let Ok(comm) = fs::read_to_string(&comm_path) {
                    processes.push(ProcessInfo {
                        pid,
                        name: comm.trim().to_string(),
                        window_title: None,
                    });
                }
            }
        }
        processes.sort_by_key(|p| p.pid);
        Ok(processes)
    }

    fn attach(&self, pid: u32) -> Result<Arc<dyn ProcessHandle>> {
        let mem_path = format!("/proc/{pid}/mem");
        // Read/write if we can, read-only if not: a scanner that can only look
        // is still useful, and the refusal then surfaces on the write itself
        // with a message the user can act on.
        let mem = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&mem_path)
            .or_else(|_| File::open(&mem_path))
            .map_err(|e| anyhow::anyhow!("Cannot open {mem_path}: {e}"))?;

        Ok(Arc::new(LinuxProcessHandle { pid, mem }))
    }
}

pub struct LinuxProcessHandle {
    pid: u32,
    /// Opened once at attach time. `pread`/`pwrite` carry their own offset, so
    /// this needs no locking and the handle stays `Sync`.
    mem: File,
}

impl ProcessHandle for LinuxProcessHandle {
    fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; size];
        let mut filled = 0usize;

        // Stop at the first short or failed read and hand back what we got,
        // matching the Windows `bytes_read` behaviour. `read_exact` failed the
        // whole call instead, so a single unreadable page - and Android is full
        // of them, between scudo guard pages and regions the GC unmaps
        // mid-scan - silently dropped an entire mapping from the scan.
        while filled < size {
            let chunk = MAX_CHUNK.min(size - filled);
            match self
                .mem
                .read_at(&mut buffer[filled..filled + chunk], (address + filled) as u64)
            {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }

        if filled == 0 && size > 0 {
            return Err(anyhow::anyhow!("Failed to read memory at 0x{address:X}"));
        }

        buffer.truncate(filled);
        Ok(buffer)
    }

    fn write_memory(&self, address: usize, data: &[u8]) -> Result<()> {
        // Writes through `/proc/<pid>/mem` go via `FOLL_FORCE`, so they land
        // even on a private mapping the target has left read-only. Shared file
        // mappings are still refused, which is the behaviour we want.
        let mut written = 0usize;
        while written < data.len() {
            match self.mem.write_at(&data[written..], (address + written) as u64) {
                Ok(0) => break,
                Ok(n) => written += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(anyhow::anyhow!("Failed to write at 0x{address:X}: {e}")),
            }
        }
        if written < data.len() {
            return Err(anyhow::anyhow!(
                "Short write at 0x{address:X}: {written}/{} bytes",
                data.len()
            ));
        }
        Ok(())
    }

    fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let maps = fs::read_to_string(&maps_path)?;
        Ok(parse_maps(&maps))
    }
}

