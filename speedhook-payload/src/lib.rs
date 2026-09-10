//! `ce_speedhook.dll` — the payload the desktop scanner injects into a target to
//! bend its clock.
//!
//! On load it walks every loaded module's import table, finds each slot bound to
//! `kernel32!QueryPerformanceCounter`, and redirects it to [`hook_qpc`], which
//! scales the counter by a factor the scanner controls. The factor lives in a
//! named shared-memory section the scanner writes and this DLL reads, as a
//! [`SpeedClock`] mapped identically into both processes — the `#[repr(C)]` on
//! that type is what makes the shared bytes line up.
//!
//! IAT patching (rewriting a pointer the target already holds) is chosen over an
//! inline hook (rewriting kernel32's code) precisely because it needs no
//! trampoline and no length-disassembler: we keep the original function pointer
//! and call it. Nothing here is hand-written machine code.

#![cfg(windows)]
// The MSVC linker prints "Creating library …" for the export lib of every
// cdylib; that is not something to act on, so quiet the linker-message lint.
#![allow(linker_messages)]

// The exact clock the engine uses, shared by source so the layout cannot drift.
// The payload only reads it (`scale`), so its `factor`/`set_factor` are dead here.
#[allow(dead_code)]
#[path = "../../src/speedhack/clock.rs"]
mod clock;
use clock::SpeedClock;

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HMODULE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Memory::{
    FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, VirtualProtect,
};

/// Name of the shared section. The scanner creates it; we open it.
const SECTION: &str = "Local\\ce_speedhook_clock";

/// The real `QueryPerformanceCounter`, saved before patching.
static REAL_QPC: AtomicUsize = AtomicUsize::new(0);
/// Mapped `*const SpeedClock`, or 0 if the section was not found.
static SHARED: AtomicUsize = AtomicUsize::new(0);

type QpcFn = unsafe extern "system" fn(*mut i64) -> BOOL;

/// Replacement for `QueryPerformanceCounter`: real counts in, scaled out.
unsafe extern "system" fn hook_qpc(count: *mut i64) -> BOOL {
    let real = REAL_QPC.load(Ordering::Acquire);
    if real == 0 {
        return 0;
    }
    let real: QpcFn = unsafe { core::mem::transmute(real) };
    let rc = unsafe { real(count) };
    if rc != 0 && !count.is_null() {
        let shared = SHARED.load(Ordering::Acquire);
        if shared != 0 {
            let clock = unsafe { &*(shared as *const SpeedClock) };
            let raw = unsafe { *count } as u64;
            unsafe { *count = clock.scale(raw) as i64 };
        }
    }
    rc
}

// -- installation --------------------------------------------------------

fn install() {
    // Map the scanner's shared clock. If it is not there yet, the hook is a
    // no-op (factor 1.0), which is harmless.
    let section = wide(SECTION);
    let mapping = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, section.as_ptr()) };
    if !mapping.is_null() {
        let view = unsafe {
            MapViewOfFile(
                mapping,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                core::mem::size_of::<SpeedClock>(),
            )
        };
        if !view.Value.is_null() {
            SHARED.store(view.Value as usize, Ordering::Release);
        }
        // Intentionally keep `mapping` open for the life of the target. A mapped
        // view keeps the section's *pages* alive but not its *name*: once every
        // open handle closes, the name is released, and the next scanner run's
        // `CreateFileMappingW` would then make a brand-new section instead of
        // reopening ours — so our reads would freeze at the first factor while
        // the scanner writes into a section we never see. Holding this handle
        // keeps the name registered so every later scanner run shares this one.
        let _ = mapping;
    }

    // Resolve the genuine QPC once, from kernel32.
    let k32 = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) };
    if !k32.is_null() {
        if let Some(p) = unsafe { GetProcAddress(k32, c"QueryPerformanceCounter".as_ptr() as _) } {
            REAL_QPC.store(p as usize, Ordering::Release);
        }
    }
    if REAL_QPC.load(Ordering::Acquire) == 0 {
        return;
    }

    patch_all_modules();
}

/// Patch every loaded module's IAT — except the system modules that *implement*
/// QPC and our own DLL. Patching those causes infinite recursion: `hook_qpc`
/// calls the real `QueryPerformanceCounter`, whose kernel32/kernelbase body
/// re-enters QPC through its own (now-redirected) IAT slot, back into
/// `hook_qpc`, until the stack overflows. Skipping the system chain keeps
/// `REAL_QPC` a genuine implementation; only the target app's modules are bent.
fn patch_all_modules() {
    let snap = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, 0 /* self */)
    };
    if snap == INVALID {
        return;
    }
    let mut me: MODULEENTRY32W = unsafe { core::mem::zeroed() };
    me.dwSize = core::mem::size_of::<MODULEENTRY32W>() as u32;
    let mut ok = unsafe { Module32FirstW(snap, &mut me) };
    while ok != 0 {
        if !skip_module(&me.szModule) {
            patch_module(me.modBaseAddr as usize);
        }
        ok = unsafe { Module32NextW(snap, &mut me) };
    }
    unsafe { CloseHandle(snap) };
}

/// Whether a module (by base name, e.g. `kernel32.dll`) must not be patched.
/// The named system DLLs form the real QPC implementation chain; the
/// `api-ms-win-*` / `ext-ms-*` API-set stubs forward into it. Our own payload is
/// excluded so the hook never redirects the very slot it reads through.
fn skip_module(sz_module: &[u16; 256]) -> bool {
    // szModule is a null-terminated UTF-16 base name; lowercase-ASCII it.
    let mut name = String::new();
    for &c in sz_module.iter() {
        if c == 0 {
            break;
        }
        name.push((c as u8 as char).to_ascii_lowercase());
    }
    const DENY: [&str; 4] = [
        "ce_speedhook.dll",
        "kernel32.dll",
        "kernelbase.dll",
        "ntdll.dll",
    ];
    DENY.contains(&name.as_str()) || name.starts_with("api-ms-win-") || name.starts_with("ext-ms-")
}

const INVALID: HANDLE = -1isize as HANDLE;

/// Whether `[rva, rva+need)` stays inside a module of `size` bytes. Every read
/// below goes through this: a real process (Unity, say) carries dozens of
/// modules and some carry import tables that do not walk the way a textbook PE
/// does. Trusting the layout and reading off the end of the mapped image faults
/// the *target* — this keeps every access inside the module's own image.
#[inline]
fn in_image(rva: usize, need: usize, size: usize) -> bool {
    rva.checked_add(need).is_some_and(|end| end <= size)
}

/// Walk one module's import table, redirecting `QueryPerformanceCounter` slots.
/// Defensive throughout: validates the headers, bounds every RVA against
/// `SizeOfImage`, bounds the descriptor walk by the import directory size, and
/// caps every loop — a malformed or unexpected module is skipped, never faulted.
fn patch_module(base: usize) {
    if base == 0 {
        return;
    }
    unsafe {
        // DOS header → PE header. e_lfanew is small in any real image.
        let e_lfanew = read::<u32>(base + 0x3C) as usize;
        if e_lfanew > 0x1000 {
            return;
        }
        let nt = base + e_lfanew;
        if read::<u32>(nt) != 0x0000_4550 {
            return; // not "PE\0\0"
        }
        // Optional header after Signature(4) + FileHeader(20). Only PE32+ (x64
        // process ⇒ x64 modules); anything else is skipped rather than misparsed.
        let opt = nt + 24;
        if read::<u16>(opt) != 0x020b {
            return; // not IMAGE_NT_OPTIONAL_HDR64_MAGIC
        }
        let size_of_image = read::<u32>(opt + 56) as usize;
        if size_of_image < 0x1000 {
            return;
        }
        // DataDirectory[1] = import table: RVA at opt+120, Size at opt+124.
        let import_rva = read::<u32>(opt + 120) as usize;
        let import_size = read::<u32>(opt + 124) as usize;
        if import_rva == 0 || !in_image(import_rva, 20, size_of_image) {
            return;
        }
        // Cap the descriptor count by the directory size (each entry is 20 B),
        // and hard-cap regardless — never trust the terminator alone.
        let max_desc = (import_size / 20).clamp(1, 8192);
        let mut desc_rva = import_rva;
        for _ in 0..max_desc {
            if !in_image(desc_rva, 20, size_of_image) {
                break;
            }
            let desc = base + desc_rva;
            let orig_first = read::<u32>(desc) as usize; // OriginalFirstThunk (INT)
            let name_rva = read::<u32>(desc + 12) as usize; // Name
            let first = read::<u32>(desc + 16) as usize; // FirstThunk (IAT)
            if name_rva == 0 && first == 0 {
                break; // the real terminator
            }
            // Names come from the INT if present, else from the IAT itself.
            let names = if orig_first != 0 { orig_first } else { first };
            if names != 0 && first != 0 {
                patch_thunks(base, names, first, size_of_image);
            }
            desc_rva += 20;
        }
    }
}

/// For one import descriptor, patch the IAT entry whose name is
/// `QueryPerformanceCounter`. Every RVA is bounded against `size_of_image`.
unsafe fn patch_thunks(base: usize, names_rva: usize, iat_rva: usize, size_of_image: usize) {
    // 23 chars + NUL; the bound for reading an IMAGE_IMPORT_BY_NAME's name.
    const QPC: &[u8] = b"QueryPerformanceCounter";
    for i in 0..8192usize {
        let nr = names_rva + i * 8;
        if !in_image(nr, 8, size_of_image) {
            break;
        }
        let name_thunk = unsafe { read::<u64>(base + nr) };
        if name_thunk == 0 {
            break;
        }
        // High bit set → import by ordinal; skip (no name to match).
        if name_thunk & 0x8000_0000_0000_0000 == 0 {
            // IMAGE_IMPORT_BY_NAME: u16 hint then a C string.
            let name_off = name_thunk as usize + 2;
            if in_image(name_off, QPC.len() + 1, size_of_image)
                && unsafe { cstr_eq(base + name_off, QPC) }
            {
                let ir = iat_rva + i * 8;
                if in_image(ir, 8, size_of_image) {
                    unsafe { write_ptr(base + ir, hook_qpc as *const () as usize) };
                }
            }
        }
    }
}

/// Overwrite a pointer-sized IAT slot, flipping page protection around it.
unsafe fn write_ptr(slot: usize, value: usize) {
    let mut old: u32 = 0;
    let ok = unsafe {
        VirtualProtect(
            slot as *const c_void,
            core::mem::size_of::<usize>(),
            PAGE_READWRITE,
            &mut old,
        )
    };
    if ok == 0 {
        return;
    }
    unsafe { core::ptr::write_unaligned(slot as *mut usize, value) };
    let mut ignore: u32 = 0;
    unsafe {
        VirtualProtect(
            slot as *const c_void,
            core::mem::size_of::<usize>(),
            old,
            &mut ignore,
        )
    };
}

// -- helpers -------------------------------------------------------------

unsafe fn read<T: Copy>(addr: usize) -> T {
    unsafe { core::ptr::read_unaligned(addr as *const T) }
}

unsafe fn cstr_eq(addr: usize, want: &[u8]) -> bool {
    for (i, &w) in want.iter().enumerate() {
        if unsafe { read::<u8>(addr + i) } != w {
            return false;
        }
    }
    unsafe { read::<u8>(addr + want.len()) == 0 }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// -- DllMain -------------------------------------------------------------

const DLL_PROCESS_ATTACH: u32 = 1;

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        unsafe { DisableThreadLibraryCalls(hinst) };
        // Do the work off the loader lock: the spawned thread's body does not
        // run until DllMain returns and the lock is released.
        std::thread::spawn(install);
    }
    1
}
