#[cfg(windows)]
pub mod windows;
#[cfg(unix)]
pub mod linux;
// Pure string handling, so it builds and is tested everywhere - the Android
// path most worth getting right before a device is involved.
pub mod maps;

use std::sync::Arc;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub base_address: usize,
    pub size: usize,
    pub readable: bool,
    pub writable: bool,
    /// The `/proc/<pid>/maps` pathname column, when the mapping has one:
    /// a file path (`/data/app/.../libfoo.so`), or a pseudo-name such as
    /// `[stack]`, `[heap]`, `[anon:libc_malloc]`, `[anon:dalvik-main space]`.
    /// Always `None` on Windows. Android needs it to keep the scanner away
    /// from device mappings that fault or hang when read.
    pub path: Option<String>,
}

/// `Sync` as well as `Send`: the handle is shared behind an `Arc` so a scan
/// thread, the freeze thread and the UI can all read through it at once.
pub trait ProcessHandle: Send + Sync {
    fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>>;
    fn write_memory(&self, address: usize, data: &[u8]) -> Result<()>;
    fn memory_regions(&self) -> Result<Vec<MemoryRegion>>;
}

pub trait Platform {
    fn enumerate_processes(&self) -> Result<Vec<ProcessInfo>>;
    fn attach(&self, pid: u32) -> Result<Arc<dyn ProcessHandle>>;
}

/// Attach to the calling process.
///
/// The Android overlay is linked into the app it inspects, so its target is
/// always itself: same UID, no `ptrace`, no root. Useful on the desktop too -
/// it is what lets the engine be tested end to end without a second process.
pub fn attach_self() -> Result<Arc<dyn ProcessHandle>> {
    #[cfg(unix)]
    {
        linux::LinuxPlatform.attach_self()
    }
    #[cfg(windows)]
    {
        windows::WindowsPlatform.attach(std::process::id())
    }
}

pub fn create_platform() -> Box<dyn Platform> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsPlatform)
    }
    #[cfg(unix)]
    {
        Box::new(linux::LinuxPlatform)
    }
}
