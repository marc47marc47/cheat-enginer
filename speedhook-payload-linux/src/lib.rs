//! `libce_speedhook_linux.so` — the payload the Linux desktop scanner injects
//! (via `ptrace` + remote `dlopen`) into a target to bend its clock.
//!
//! On load (an `.init_array` constructor, run by the dynamic loader the moment
//! `dlopen` maps us) it maps the scanner's shared [`SpeedClock`] from a POSIX shm
//! object, then walks every *other* loaded object's relocations and repoints its
//! `clock_gettime` GOT slot at [`hooked_clock_gettime`], which scales
//! `CLOCK_MONOTONIC`. This is the exact in-process GOT-patch the Android backend
//! uses (same ELF64 layout), minus the JNI wiring and reading the factor from
//! shared memory instead of a process-local static.
//!
//! **VERIFICATION STATUS: UNVERIFIED ON A LINUX DEVICE — cargo-checked only.**
//! The ELF walk is copied from the device-proven Android hook, but this crate
//! has never been built with a real linker or loaded into a process. See
//! `android/TODO-speedup.md` (B2).

#![cfg(all(unix, not(target_os = "android")))]

// The exact clock the engine uses, shared by source so the layout cannot drift.
// The payload only reads it (`scale`), so its `factor`/`set_factor` are dead here.
#[allow(dead_code)]
#[path = "../../src/speedhack/clock.rs"]
mod clock;
use clock::SpeedClock;

use core::ffi::{CStr, c_char, c_int, c_void};
use core::mem::size_of;
use core::sync::atomic::{AtomicUsize, Ordering};

const SECTION: &str = "/ce_speedhook_clock";
const CLOCK_MONOTONIC_ID: libc::clockid_t = libc::CLOCK_MONOTONIC;

/// Mapped `*const SpeedClock`, or 0 if the shm object was not found.
static SHARED: AtomicUsize = AtomicUsize::new(0);
/// The real `clock_gettime` (our own untouched import; self is never patched).
static ORIG: AtomicUsize = AtomicUsize::new(0);

// -- ELF64 aggregates the `libc` crate does not expose (see android.rs) -------
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

const DT_NULL: i64 = 0;
const DT_PLTRELSZ: i64 = 2;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_JMPREL: i64 = 23;

/// Replacement for `clock_gettime`: real value in, scaled `CLOCK_MONOTONIC` out.
extern "C" fn hooked_clock_gettime(clk_id: libc::clockid_t, tp: *mut libc::timespec) -> c_int {
    let orig = ORIG.load(Ordering::Acquire);
    let rc = if orig != 0 {
        let real: extern "C" fn(libc::clockid_t, *mut libc::timespec) -> c_int =
            unsafe { core::mem::transmute(orig) };
        real(clk_id, tp)
    } else {
        unsafe { libc::clock_gettime(clk_id, tp) }
    };
    if rc == 0 && clk_id == CLOCK_MONOTONIC_ID && !tp.is_null() {
        let shared = SHARED.load(Ordering::Acquire);
        if shared != 0 {
            let clock = unsafe { &*(shared as *const SpeedClock) };
            let ts = unsafe { &mut *tp };
            let ns = (ts.tv_sec as u64)
                .wrapping_mul(1_000_000_000)
                .wrapping_add(ts.tv_nsec as u64);
            let v = clock.scale(ns);
            ts.tv_sec = (v / 1_000_000_000) as libc::time_t;
            ts.tv_nsec = (v % 1_000_000_000) as _;
        }
    }
    rc
}

fn install() {
    // Map the scanner's shared clock (read/write; we only read). Absent → the
    // hook is a harmless factor-1.0 no-op.
    if let Ok(name) = std::ffi::CString::new(SECTION) {
        unsafe {
            let fd = libc::shm_open(name.as_ptr(), libc::O_RDWR, 0o600);
            if fd >= 0 {
                let p = libc::mmap(
                    core::ptr::null_mut(),
                    size_of::<SpeedClock>(),
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                );
                libc::close(fd);
                if p != libc::MAP_FAILED {
                    SHARED.store(p as usize, Ordering::Release);
                }
            }
        }
    }

    ORIG.store(libc::clock_gettime as *const () as usize, Ordering::Release);
    unsafe { libc::dl_iterate_phdr(Some(iterate), core::ptr::null_mut()) };
}

unsafe extern "C" fn iterate(
    info: *mut libc::dl_phdr_info,
    _size: usize,
    _data: *mut c_void,
) -> c_int {
    let info = unsafe { &*info };
    let base = info.dlpi_addr as usize;

    let name = if info.dlpi_name.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(info.dlpi_name) }.to_str().unwrap_or("")
    };
    // Never our own module (would scale our own reads / recurse), nor vDSO/linker.
    if name.contains("ce_speedhook_linux")
        || name.contains("[vdso]")
        || name.contains("ld-linux")
        || name.contains("ld-musl")
    {
        return 0;
    }

    let phdrs = unsafe { core::slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum as usize) };
    let mut dynamic: *const Elf64_Dyn = core::ptr::null();
    for ph in phdrs {
        if ph.p_type == libc::PT_DYNAMIC {
            dynamic = (base + ph.p_vaddr as usize) as *const Elf64_Dyn;
            break;
        }
    }
    if dynamic.is_null() {
        return 0;
    }

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
    unsafe {
        patch_relocs(jmprel, pltrelsz, symtab, strtab, base);
        patch_relocs(rela, relasz, symtab, strtab, base);
    }
    0
}

fn resolve(value: usize, base: usize) -> usize {
    if value < base { base + value } else { value }
}

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
        let name = unsafe { CStr::from_ptr((strtab + sym.st_name as usize) as *const c_char) };
        if name.to_bytes() != b"clock_gettime" {
            continue;
        }
        let slot = (base + r.r_offset as usize) as *mut usize;
        if unsafe { make_writable(slot) } {
            unsafe { *slot = hooked_clock_gettime as *const () as usize };
        }
    }
}

unsafe fn make_writable(addr: *mut usize) -> bool {
    let pagesize = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    if pagesize == 0 {
        return false;
    }
    let page = (addr as usize) & !(pagesize - 1);
    unsafe {
        libc::mprotect(
            page as *mut c_void,
            pagesize,
            libc::PROT_READ | libc::PROT_WRITE,
        ) == 0
    }
}

// -- constructor: run by the loader the moment dlopen maps us -----------------

/// Placed in `.init_array` so the dynamic loader calls it during `dlopen`,
/// after relocations are applied — the standard "shared-object constructor".
#[used]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".init_array"))]
static CTOR: extern "C" fn() = ctor;

extern "C" fn ctor() {
    // Do the work off the loader path is unnecessary here (no loader lock like
    // Windows DllMain); a plain in-line install is fine.
    install();
}
