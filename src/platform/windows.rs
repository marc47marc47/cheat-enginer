use std::collections::HashMap;
use std::ffi::c_void;
use std::mem;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQueryEx,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

use super::{MemoryRegion, Platform, ProcessHandle, ProcessInfo};
use crate::error::Result;

// Global state for EnumWindows callback
static WINDOW_TITLES: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1; // continue
    }

    let text_len = unsafe { GetWindowTextLengthW(hwnd) };
    if text_len == 0 {
        return 1;
    }

    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return 1;
    }

    let buf_len = (text_len + 1) as usize;
    let mut buf: Vec<u16> = vec![0; buf_len];
    let actual = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf_len as i32) };
    if actual > 0 {
        let title = String::from_utf16_lossy(&buf[..actual as usize]);
        if !title.is_empty() {
            if let Ok(mut guard) = WINDOW_TITLES.lock() {
                if let Some(ref mut map) = *guard {
                    // Keep the first (usually main) window title per process
                    map.entry(pid).or_insert(title);
                }
            }
        }
    }

    1 // continue enumeration
}

fn collect_window_titles() -> HashMap<u32, String> {
    {
        let mut guard = WINDOW_TITLES.lock().unwrap();
        *guard = Some(HashMap::new());
    }

    unsafe {
        EnumWindows(Some(enum_windows_callback), 0);
    }

    let mut guard = WINDOW_TITLES.lock().unwrap();
    guard.take().unwrap_or_default()
}

pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn enumerate_processes(&self) -> Result<Vec<ProcessInfo>> {
        let titles = collect_window_titles();
        let mut processes = Vec::new();

        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(anyhow::anyhow!("Failed to create process snapshot"));
            }

            let mut entry: PROCESSENTRY32 = mem::zeroed();
            entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

            if Process32First(snapshot, &mut entry) != 0 {
                loop {
                    let name_bytes: Vec<u8> = entry
                        .szExeFile
                        .iter()
                        .take_while(|&&b| b != 0)
                        .map(|&b| b as u8)
                        .collect();
                    let name = String::from_utf8_lossy(&name_bytes).to_string();
                    let pid = entry.th32ProcessID;

                    processes.push(ProcessInfo {
                        pid,
                        name,
                        window_title: titles.get(&pid).cloned(),
                    });

                    if Process32Next(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }

            CloseHandle(snapshot);
        }
        Ok(processes)
    }

    fn attach(&self, pid: u32) -> Result<Box<dyn ProcessHandle>> {
        let access =
            PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION;
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Err(anyhow::anyhow!(
                "Failed to open process {pid}. Run as administrator?"
            ));
        }
        Ok(Box::new(WindowsProcessHandle { pid, handle }))
    }
}

pub struct WindowsProcessHandle {
    pid: u32,
    handle: HANDLE,
}

unsafe impl Send for WindowsProcessHandle {}

impl Drop for WindowsProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

impl ProcessHandle for WindowsProcessHandle {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; size];
        let mut bytes_read: usize = 0;
        let result = unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                size,
                &mut bytes_read,
            )
        };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to read memory at 0x{address:X}"));
        }
        buffer.truncate(bytes_read);
        Ok(buffer)
    }

    fn write_memory(&self, address: usize, data: &[u8]) -> Result<()> {
        let mut bytes_written: usize = 0;
        let result = unsafe {
            WriteProcessMemory(
                self.handle,
                address as *mut c_void,
                data.as_ptr() as *const c_void,
                data.len(),
                &mut bytes_written,
            )
        };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to write memory at 0x{address:X}"));
        }
        Ok(())
    }

    fn memory_regions(&self) -> Result<Vec<MemoryRegion>> {
        let mut regions = Vec::new();
        let mut address: usize = 0;

        unsafe {
            loop {
                let mut info: MEMORY_BASIC_INFORMATION = mem::zeroed();
                let result = VirtualQueryEx(
                    self.handle,
                    address as *const c_void,
                    &mut info,
                    mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                );
                if result == 0 {
                    break;
                }

                if info.State == MEM_COMMIT
                    && (info.Protect & PAGE_GUARD) == 0
                    && (info.Protect & PAGE_NOACCESS) == 0
                {
                    let protect = info.Protect;
                    regions.push(MemoryRegion {
                        base_address: info.BaseAddress as usize,
                        size: info.RegionSize,
                        readable: true,
                        writable: (protect & 0x04) != 0
                            || (protect & 0x08) != 0
                            || (protect & 0x40) != 0
                            || (protect & 0x80) != 0,
                        executable: (protect & 0x10) != 0
                            || (protect & 0x20) != 0
                            || (protect & 0x40) != 0
                            || (protect & 0x80) != 0,
                    });
                }

                let next = (info.BaseAddress as usize).checked_add(info.RegionSize);
                match next {
                    Some(n) if n > address => address = n,
                    _ => break,
                }
            }
        }

        Ok(regions)
    }
}
