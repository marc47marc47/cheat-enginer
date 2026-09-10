//! Linux desktop backend: cross-process speedhack by `ptrace` + remote `dlopen`.
//!
//! Symmetric to [`super::windows`]: the scanner is a *separate* process from the
//! target, shares one [`SpeedClock`] with it through a named POSIX shared-memory
//! object, and injects a payload `.so` (`ce_speedhook_linux`) that patches its
//! own GOT `clock_gettime` slots from inside and reads the shared factor. The
//! injection is the textbook `PTRACE_ATTACH` → set up a remote call to `dlopen`
//! → run it → restore registers → detach.
//!
//! **VERIFICATION STATUS: UNVERIFIED ON A LINUX DEVICE.** This was written and
//! `cargo check`ed on a Windows host against `x86_64-unknown-linux-gnu` and
//! `-musl`; it has never been run. The scanner half (shared clock) mirrors the
//! device-proven Windows path, but the `ptrace` injector below has known
//! simplifying assumptions (x86_64 only; target and scanner share the same libc
//! file so a `dlopen` offset can be reused; a bad return address is used to
//! trap call completion). Treat it as a starting point to verify on Linux, not
//! as a finished, tested path. See `android/TODO-speedup.md` (B2).

#![cfg(all(unix, not(target_os = "android")))]

use std::sync::OnceLock;

use crate::speedhack::clock::SpeedClock;

/// Must match the payload's shared-object name.
const SECTION: &str = "/ce_speedhook_clock";

struct Shared(*mut SpeedClock);
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// Create-or-open the shared `SpeedClock` in a POSIX shm object and return it.
/// A fresh object is zeroed (factor 0.0), so the first creator initialises it to
/// identity; a later opener maps the same bytes. The fd is intentionally leaked
/// so the mapping outlives this call.
fn shared() -> &'static SpeedClock {
    let s = SHARED.get_or_init(|| {
        let size = std::mem::size_of::<SpeedClock>();
        let name = std::ffi::CString::new(SECTION).unwrap();
        unsafe {
            // O_CREAT|O_EXCL tells us whether we are the creator (must init).
            let mut fresh = true;
            let mut fd = libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            );
            if fd < 0 {
                fresh = false;
                fd = libc::shm_open(name.as_ptr(), libc::O_RDWR, 0o600);
            }
            if fd < 0 {
                return Shared(std::ptr::null_mut());
            }
            if fresh {
                libc::ftruncate(fd, size as libc::off_t);
            }
            let p = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            libc::close(fd);
            if p == libc::MAP_FAILED {
                return Shared(std::ptr::null_mut());
            }
            let ptr = p as *mut SpeedClock;
            if fresh {
                std::ptr::write(ptr, SpeedClock::new());
            }
            Shared(ptr)
        }
    });
    // Null only if shm failed; fall back to a throwaway identity clock so callers
    // never deref null. (A failed map means injection can't share anyway.)
    static FALLBACK: SpeedClock = SpeedClock::new();
    if s.0.is_null() {
        &FALLBACK
    } else {
        unsafe { &*s.0 }
    }
}

/// Raw monotonic ns — the same timeline the payload's hook scales, so factor
/// changes anchor without a jump.
fn monotonic_now_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    (ts.tv_sec as u64)
        .wrapping_mul(1_000_000_000)
        .wrapping_add(ts.tv_nsec as u64)
}

/// Set the multiplier for the attached target(s).
pub fn set_factor(factor: f64) {
    shared().set_factor(monotonic_now_ns(), factor);
}

/// The current multiplier.
pub fn factor() -> f64 {
    shared().factor()
}

// -- ptrace injector (UNVERIFIED) --------------------------------------------

const RTLD_NOW: libc::c_int = 2;

/// Inject `so_path` into `pid` via `ptrace` + a remote `dlopen`. Returns whether
/// the remote `dlopen` appeared to succeed (non-null handle). Ensures the shared
/// clock exists first so the payload can map it on load.
pub fn install(pid: u32, so_path: &str) -> bool {
    let _ = shared();
    let pid = pid as libc::pid_t;

    unsafe {
        if libc::ptrace(libc::PTRACE_ATTACH, pid, 0, 0) < 0 {
            return false;
        }
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0);

        let ok = remote_dlopen(pid, so_path);

        libc::ptrace(libc::PTRACE_DETACH, pid, 0, 0);
        ok
    }
}

/// The body of the injection, with the tracee stopped. Sets up registers to call
/// the target's `dlopen(path, RTLD_NOW)`, runs until it faults on a deliberately
/// bogus return address, reads the returned handle, and restores registers.
#[cfg(target_arch = "x86_64")]
unsafe fn remote_dlopen(pid: libc::pid_t, so_path: &str) -> bool {
    // Save the current registers so we can put the thread back exactly.
    let mut saved: libc::user_regs_struct = unsafe { std::mem::zeroed() };
    if unsafe { libc::ptrace(libc::PTRACE_GETREGS, pid, 0, &mut saved) } < 0 {
        return false;
    }

    // Resolve the target's dlopen. Assumption: target and scanner share the same
    // libc file, so dlopen's offset within libc is identical; only the load base
    // (ASLR) differs. Reuse our own dlopen offset against the target's libc base.
    let our_libc = match module_base("/proc/self/maps", "libc") {
        Some(b) => b,
        None => return false,
    };
    let their_libc = match module_base(&format!("/proc/{pid}/maps"), "libc") {
        Some(b) => b,
        None => return false,
    };
    let our_dlopen = libc::dlopen as usize;
    let dlopen_off = our_dlopen.wrapping_sub(our_libc);
    let target_dlopen = their_libc.wrapping_add(dlopen_off);

    // Write the .so path into the target's stack, well below rsp.
    let path_bytes = {
        let mut v = so_path.as_bytes().to_vec();
        v.push(0);
        v
    };
    let path_addr = (saved.rsp as usize).wrapping_sub(512);
    if !write_mem(pid, path_addr, &path_bytes) {
        return false;
    }

    // Build the call frame. Align, then place a bogus return address (0): when
    // dlopen returns, `ret` jumps to 0 and faults with SIGSEGV, which stops the
    // tracee and hands control back to us — a portable way to detect completion.
    let mut regs = saved;
    let mut sp = (path_addr & !0xF) as u64;
    sp -= 8; // leave 8 below the string, keep 16-byte call alignment
    // push return address 0
    if !write_mem(pid, sp as usize, &0u64.to_ne_bytes()) {
        return false;
    }
    regs.rsp = sp;
    regs.rip = target_dlopen as u64;
    regs.rdi = path_addr as u64; // arg1: path
    regs.rsi = RTLD_NOW as u64; // arg2: flags
    if unsafe { libc::ptrace(libc::PTRACE_SETREGS, pid, 0, &regs) } < 0 {
        return false;
    }

    // Run until the bogus return faults.
    if unsafe { libc::ptrace(libc::PTRACE_CONT, pid, 0, 0) } < 0 {
        return false;
    }
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };

    // Read the handle dlopen returned (rax), then restore the thread.
    let mut after: libc::user_regs_struct = unsafe { std::mem::zeroed() };
    let handle = if unsafe { libc::ptrace(libc::PTRACE_GETREGS, pid, 0, &mut after) } >= 0 {
        after.rax
    } else {
        0
    };
    unsafe { libc::ptrace(libc::PTRACE_SETREGS, pid, 0, &saved) };

    handle != 0
}

/// Non-x86_64 Linux is not implemented (register/ABI layout differs).
#[cfg(not(target_arch = "x86_64"))]
unsafe fn remote_dlopen(_pid: libc::pid_t, _so_path: &str) -> bool {
    false
}

/// First load address of the mapping whose path contains `needle` (e.g. "libc")
/// in a `/proc/<pid>/maps` file. Returns the lowest base seen.
fn module_base(maps_path: &str, needle: &str) -> Option<usize> {
    let text = std::fs::read_to_string(maps_path).ok()?;
    for line in text.lines() {
        if !line.contains(needle) {
            continue;
        }
        // "<start>-<end> perms offset dev inode path"
        let start = line.split('-').next()?;
        if let Ok(base) = usize::from_str_radix(start, 16) {
            return Some(base);
        }
    }
    None
}

/// Write `bytes` into the tracee's address space via `/proc/<pid>/mem`
/// (writable while ptrace-stopped).
fn write_mem(pid: libc::pid_t, addr: usize, bytes: &[u8]) -> bool {
    use std::io::{Seek, SeekFrom, Write};
    let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .open(format!("/proc/{pid}/mem"))
    else {
        return false;
    };
    if f.seek(SeekFrom::Start(addr as u64)).is_err() {
        return false;
    }
    f.write_all(bytes).is_ok()
}
