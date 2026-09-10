//! Android backend: an in-process GOT hook of `clock_gettime`.
//!
//! The engine already runs inside the target process (embedded, or the shared
//! process of the standalone pair), so "hooking" here is not code injection —
//! it is overwriting a data pointer. Every shared object that calls
//! `clock_gettime` reaches it through a GOT slot; we walk each loaded object's
//! relocations, find the slot for `clock_gettime`, and point it at
//! [`hooked_clock_gettime`], which scales `CLOCK_MONOTONIC` before returning.
//! Because we only rewrite a pointer (not executable bytes) there is no code
//! generation and no instruction-cache flush.
//!
//! `SystemClock.uptimeMillis()` resolves to `CLOCK_MONOTONIC`, so scaling that
//! id bends every `Handler.postDelayed` schedule the game relies on. Scaling
//! only `CLOCK_MONOTONIC` keeps `CLOCK_REALTIME` / `CLOCK_BOOTTIME` truthful and
//! avoids anchoring against a different-origin clock.
//!
//! **What this does and does not speed up** (measured on an x86_64 emulator,
//! Android 15). A `postDelayed(delay)` message becomes due when the scaled
//! `uptimeMillis` passes its target, and the Looper re-checks that on every
//! wake — so *time-scheduled* logic (Dungeon Tap's `ATTACK_MS = 1500` cadence,
//! cooldowns, spawns) accelerates cleanly by the factor: at 4× the player takes
//! ~4× the damage per wall-second (2.8 → 11.5 HP/s, verified). But a per-frame
//! animation whose step is a *fixed* delta each frame is paced by vsync, not by
//! the clock: the display's `DisplayEventReceiver` fd wakes the Looper at a real
//! 60 Hz regardless of our scaling, so `ArenaView`'s `x += vx` drift stays at
//! 60 fps. Scaling the clock cannot touch a vsync-locked frame rate (the vsync
//! fd is driven by SurfaceFlinger, out of process); a game whose motion is
//! frame-rate-independent — `x += vx * dt` with `dt` off `uptimeMillis` — would
//! scale visibly too. This is inherent to a clock-only speedhack, not a bug.
//!
//! **We never patch our own `libce_engine.so`.** If we did, the engine's own
//! freeze thread (100 ms) and scan timing would be scaled too. Excluding self
//! also lets [`hooked_clock_gettime`] call the real `clock_gettime` through our
//! own untouched GOT without recursing.

#![cfg(target_os = "android")]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use libc::{
    PROT_READ, PROT_WRITE, PT_DYNAMIC, _SC_PAGESIZE, clock_gettime, clockid_t, dl_iterate_phdr,
    dl_phdr_info, mprotect, sysconf, timespec,
};

// libc exposes the Elf64 *aliases* (Elf64_Off, Elf64_Half…) but not these
// aggregate structs, so we spell them out. Field order is fixed by the ELF64
// ABI; `#[repr(C)]` pins it.
#[repr(C)]
#[allow(non_camel_case_types)]
struct Elf64_Dyn {
    d_tag: i64,
    d_un: u64,
}

#[repr(C)]
#[allow(non_camel_case_types)]
struct Elf64_Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

#[repr(C)]
#[allow(non_camel_case_types)]
struct Elf64_Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

// Dynamic-section tags we care about (Elf64_Dyn.d_tag is a signed 64-bit int).
const DT_NULL: i64 = 0;
const DT_PLTRELSZ: i64 = 2;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_JMPREL: i64 = 23;

const CLOCK_MONOTONIC_ID: clockid_t = libc::CLOCK_MONOTONIC;

/// The real `clock_gettime`, saved so the stub can call it. Set at install; it
/// is our own untouched import, i.e. the genuine libc function.
static ORIG: AtomicUsize = AtomicUsize::new(0);
static PATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// One line to logcat under tag `ce-speed`. Called once from [`install`], never
/// from the hot hook path (a log call re-enters `clock_gettime`).
fn log(msg: &str) {
    unsafe extern "C" {
        fn __android_log_write(prio: i32, tag: *const c_char, text: *const c_char) -> i32;
    }
    if let Ok(t) = std::ffi::CString::new(msg) {
        let tag = c"ce-speed";
        unsafe { __android_log_write(4, tag.as_ptr(), t.as_ptr()) };
    }
}

/// Real monotonic time in ns, for anchoring factor changes on the *same*
/// timeline the stub scales. Always the genuine clock (our GOT is never
/// patched), so it is safe to call before or after install.
pub fn real_monotonic_ns() -> u64 {
    let mut ts = timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { clock_gettime(CLOCK_MONOTONIC_ID, &mut ts) };
    (ts.tv_sec as u64)
        .wrapping_mul(1_000_000_000)
        .wrapping_add(ts.tv_nsec as u64)
}

/// Replacement for `clock_gettime`: real value in, scaled `CLOCK_MONOTONIC` out.
extern "C" fn hooked_clock_gettime(clk_id: clockid_t, tp: *mut timespec) -> c_int {
    let orig = ORIG.load(Ordering::Acquire);
    let rc = if orig != 0 {
        let real: extern "C" fn(clockid_t, *mut timespec) -> c_int =
            unsafe { std::mem::transmute(orig) };
        real(clk_id, tp)
    } else {
        unsafe { clock_gettime(clk_id, tp) }
    };
    if rc == 0 && clk_id == CLOCK_MONOTONIC_ID && !tp.is_null() {
        let ts = unsafe { &mut *tp };
        let ns = (ts.tv_sec as u64)
            .wrapping_mul(1_000_000_000)
            .wrapping_add(ts.tv_nsec as u64);
        let v = crate::speedhack::scale_ns(ns);
        ts.tv_sec = (v / 1_000_000_000) as libc::time_t;
        ts.tv_nsec = (v % 1_000_000_000) as _;
    }
    rc
}

/// Patch every loaded object's `clock_gettime` GOT slot. Idempotent. Returns the
/// number of slots patched. On 32-bit ABIs (armeabi-v7a, Elf32) this does
/// nothing and returns 0 — the caller greys out the control there.
pub fn install() -> usize {
    if size_of::<usize>() != 8 {
        return 0; // Elf32 (armeabi-v7a) not handled.
    }
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return PATCHED.load(Ordering::Relaxed);
    }
    // Our own `clock_gettime` import is the real one (self is excluded from
    // patching), so its address is a safe "original" for the stub to call.
    ORIG.store(clock_gettime as *const () as usize, Ordering::Release);
    unsafe { dl_iterate_phdr(Some(iterate), std::ptr::null_mut()) };
    let n = PATCHED.load(Ordering::Relaxed);
    log(&format!("install: patched {n} clock_gettime slot(s)"));
    n
}

/// dl_iterate_phdr callback: one call per loaded object.
unsafe extern "C" fn iterate(info: *mut dl_phdr_info, _size: usize, _data: *mut c_void) -> c_int {
    let info = unsafe { &*info };
    let base = info.dlpi_addr as usize;

    let name = if info.dlpi_name.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(info.dlpi_name) }
            .to_str()
            .unwrap_or("")
    };
    // Never our own module (would scale the engine's own threads), nor the vDSO
    // or the dynamic linker.
    if name.contains("libce_engine") || name.contains("[vdso]") || name.contains("linker") {
        return 0;
    }

    // Locate the PT_DYNAMIC segment.
    let phdrs = unsafe { std::slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum as usize) };
    let mut dynamic: *const Elf64_Dyn = std::ptr::null();
    for ph in phdrs {
        if ph.p_type == PT_DYNAMIC {
            dynamic = (base + ph.p_vaddr as usize) as *const Elf64_Dyn;
            break;
        }
    }
    if dynamic.is_null() {
        return 0;
    }

    // Walk the dynamic array, gathering the tables we need.
    let (mut symtab, mut strtab) = (0usize, 0usize);
    let (mut jmprel, mut pltrelsz) = (0usize, 0usize);
    let (mut rela, mut relasz) = (0usize, 0usize);
    let mut d = dynamic;
    loop {
        let e = unsafe { &*d };
        match e.d_tag {
            DT_NULL => break,
            DT_SYMTAB => symtab = resolve(e.d_un as usize, base),
            DT_STRTAB => strtab = resolve(e.d_un as usize, base),
            DT_JMPREL => jmprel = resolve(e.d_un as usize, base),
            DT_PLTRELSZ => pltrelsz = e.d_un as usize,
            DT_RELA => rela = resolve(e.d_un as usize, base),
            DT_RELASZ => relasz = e.d_un as usize,
            _ => {}
        }
        d = unsafe { d.add(1) };
    }
    if symtab == 0 || strtab == 0 {
        return 0;
    }

    // .rela.plt (JUMP_SLOT) and .rela.dyn (GLOB_DAT) can both hold clock_gettime.
    unsafe {
        patch_relocs(jmprel, pltrelsz, symtab, strtab, base);
        patch_relocs(rela, relasz, symtab, strtab, base);
    }
    0
}

/// Some Bionic builds store dynamic table addresses base-relative, others
/// absolute. The load-bias heuristic: anything below the module base must be an
/// offset into it.
fn resolve(value: usize, base: usize) -> usize {
    if value < base { base + value } else { value }
}

/// Overwrite the GOT slot of every `clock_gettime` relocation in one table.
unsafe fn patch_relocs(rela: usize, size: usize, symtab: usize, strtab: usize, base: usize) {
    if rela == 0 || size == 0 {
        return;
    }
    let count = size / size_of::<Elf64_Rela>();
    let entries = rela as *const Elf64_Rela;
    for i in 0..count {
        let r = unsafe { &*entries.add(i) };
        let sym_index = (r.r_info >> 32) as usize;
        let sym = unsafe { &*(symtab as *const Elf64_Sym).add(sym_index) };
        let name_ptr = (strtab + sym.st_name as usize) as *const c_char;
        let name = unsafe { CStr::from_ptr(name_ptr) };
        if name.to_bytes() != b"clock_gettime" {
            continue;
        }
        let slot = (base + r.r_offset as usize) as *mut usize;
        if unsafe { make_writable(slot) } {
            unsafe { *slot = hooked_clock_gettime as *const () as usize };
            PATCHED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// mprotect the page holding `addr` to R+W (the slot may sit in RELRO).
unsafe fn make_writable(addr: *mut usize) -> bool {
    let pagesize = unsafe { sysconf(_SC_PAGESIZE) } as usize;
    if pagesize == 0 {
        return false;
    }
    let page = (addr as usize) & !(pagesize - 1);
    unsafe { mprotect(page as *mut c_void, pagesize, PROT_READ | PROT_WRITE) == 0 }
}
