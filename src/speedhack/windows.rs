//! Windows backend: cross-process speedhack by DLL injection.
//!
//! Unlike Android, the scanner here is a *separate* process from the target, so
//! it cannot patch the target's imports directly — it injects `ce_speedhook.dll`
//! (built from `speedhook-payload/`), which does the IAT patching from inside.
//! The two processes share one [`SpeedClock`] through a named file-mapping: the
//! scanner writes the factor, the injected hook reads it. Because `SpeedClock`
//! is `#[repr(C)]`, the same bytes are the same type in both.
//!
//! Injection is the textbook `VirtualAllocEx` + `WriteProcessMemory` +
//! `CreateRemoteThread(LoadLibraryW)` dance. Nothing is hand-written machine
//! code; the hook logic is ordinary Rust compiled into the payload DLL.

#![cfg(windows)]

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MapViewOfFile,
    PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;
use windows_sys::Win32::System::Threading::{
    CreateRemoteThread, LPTHREAD_START_ROUTINE, OpenProcess, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    WaitForSingleObject,
};

use crate::speedhack::clock::SpeedClock;

/// Must match the payload's `SECTION`.
const SECTION: &str = "Local\\ce_speedhook_clock";
const ERROR_ALREADY_EXISTS: u32 = 183;

/// The shared clock lives in a named mapping so the injected DLL can read it.
struct Shared(*mut SpeedClock);
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// Create-or-open the shared `SpeedClock` and return it. The backing mapping
/// handle is intentionally leaked so the section outlives this call; a mapped
/// view keeps it alive for the target too.
fn shared() -> &'static SpeedClock {
    let s = SHARED.get_or_init(|| {
        let name = wide(SECTION);
        let size = std::mem::size_of::<SpeedClock>();
        unsafe {
            let mapping = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                size as u32,
                name.as_ptr(),
            );
            // Fresh mapping starts zeroed; a zeroed SpeedClock has factor 0.0,
            // so a first creator must initialise it to identity.
            let fresh = GetLastError() != ERROR_ALREADY_EXISTS;
            let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size);
            let ptr = view.Value as *mut SpeedClock;
            if fresh && !ptr.is_null() {
                std::ptr::write(ptr, SpeedClock::new());
            }
            Shared(ptr)
        }
    });
    unsafe { &*s.0 }
}

fn qpc_now() -> u64 {
    let mut c: i64 = 0;
    unsafe { QueryPerformanceCounter(&mut c) };
    c as u64
}

/// Set the multiplier. Anchors against the performance counter (the same clock
/// the hook scales), in count-space, so no jump.
pub fn set_factor(factor: f64) {
    shared().set_factor(qpc_now(), factor);
}

/// The current multiplier.
pub fn factor() -> f64 {
    shared().factor()
}

/// Inject `dll_path` into `pid` and return whether the remote `LoadLibraryW`
/// succeeded. Ensures the shared clock section exists first, so the payload can
/// map it the moment it loads.
pub fn install(pid: u32, dll_path: &str) -> bool {
    let _ = shared();

    let access = PROCESS_CREATE_THREAD
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ
        | PROCESS_QUERY_INFORMATION;
    unsafe {
        let proc = OpenProcess(access, 0, pid);
        if proc.is_null() {
            return false;
        }
        let result = inject_into(proc, dll_path);
        CloseHandle(proc);
        result
    }
}

unsafe fn inject_into(proc: HANDLE, dll_path: &str) -> bool {
    let wpath = wide(dll_path);
    let bytes = wpath.len() * 2;

    let remote = unsafe {
        VirtualAllocEx(
            proc,
            std::ptr::null(),
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        return false;
    }

    let mut written = 0usize;
    let wrote = unsafe {
        WriteProcessMemory(
            proc,
            remote,
            wpath.as_ptr() as *const _,
            bytes,
            &mut written,
        )
    };
    if wrote == 0 {
        unsafe { VirtualFreeEx(proc, remote, 0, MEM_RELEASE) };
        return false;
    }

    // LoadLibraryW lives at the same address in every process this session.
    let k32 = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) };
    let load = unsafe { GetProcAddress(k32, c"LoadLibraryW".as_ptr() as _) };
    let start: LPTHREAD_START_ROUTINE = unsafe { std::mem::transmute(load) };

    let thread = unsafe {
        CreateRemoteThread(
            proc,
            std::ptr::null(),
            0,
            start,
            remote,
            0,
            std::ptr::null_mut(),
        )
    };
    let ok = if !thread.is_null() {
        unsafe {
            WaitForSingleObject(thread, 5000);
            CloseHandle(thread);
        }
        true
    } else {
        false
    };

    unsafe { VirtualFreeEx(proc, remote, 0, MEM_RELEASE) };
    ok
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
