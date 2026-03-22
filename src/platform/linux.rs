use std::fs;
use std::io::{Read, Seek, SeekFrom, Write as IoWrite};

use super::{MemoryRegion, Platform, ProcessHandle, ProcessInfo};
use crate::error::Result;

pub struct LinuxPlatform;

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

    fn attach(&self, pid: u32) -> Result<Box<dyn ProcessHandle>> {
        let mem_path = format!("/proc/{pid}/mem");
        if !std::path::Path::new(&mem_path).exists() {
            return Err(anyhow::anyhow!("Process {pid} not found"));
        }
        Ok(Box::new(LinuxProcessHandle { pid }))
    }
}

pub struct LinuxProcessHandle {
    pid: u32,
}

impl ProcessHandle for LinuxProcessHandle {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>> {
        let mem_path = format!("/proc/{}/mem", self.pid);
        let mut file = fs::File::open(&mem_path)?;
        file.seek(SeekFrom::Start(address as u64))?;
        let mut buffer = vec![0u8; size];
        file.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    fn write_memory(&self, address: usize, data: &[u8]) -> Result<()> {
        let mem_path = format!("/proc/{}/mem", self.pid);
        let mut file = fs::OpenOptions::new().write(true).open(&mem_path)?;
        file.seek(SeekFrom::Start(address as u64))?;
        file.write_all(data)?;
        Ok(())
    }

    fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let maps = fs::read_to_string(&maps_path)?;
        let mut regions = Vec::new();

        for line in maps.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() != 2 {
                continue;
            }
            let start = usize::from_str_radix(addr_range[0], 16).unwrap_or(0);
            let end = usize::from_str_radix(addr_range[1], 16).unwrap_or(0);
            let perms = if parts.len() > 1 { parts[1] } else { "" };

            regions.push(MemoryRegion {
                base_address: start,
                size: end - start,
                readable: perms.contains('r'),
                writable: perms.contains('w'),
                executable: perms.contains('x'),
            });
        }

        Ok(regions)
    }
}
