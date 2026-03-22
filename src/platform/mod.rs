#[cfg(windows)]
pub mod windows;
#[cfg(unix)]
pub mod linux;

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
    pub executable: bool,
}

pub trait ProcessHandle: Send {
    fn pid(&self) -> u32;
    fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>>;
    fn write_memory(&self, address: usize, data: &[u8]) -> Result<()>;
    fn memory_regions(&self) -> Result<Vec<MemoryRegion>>;
}

pub trait Platform {
    fn enumerate_processes(&self) -> Result<Vec<ProcessInfo>>;
    fn attach(&self, pid: u32) -> Result<Box<dyn ProcessHandle>>;
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
