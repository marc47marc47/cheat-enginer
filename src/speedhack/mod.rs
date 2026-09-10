//! Speedhack: make a target read a scaled clock so its time runs faster or
//! slower.
//!
//! The scaling math lives in [`clock`] and is platform-agnostic. *Installing*
//! the hook is not, and the platforms split into two shapes:
//!
//! * **Android** — the engine runs *inside* the target, so it patches its own
//!   GOT and reads a process-local [`clock::SpeedClock`] (see [`android`]).
//! * **Windows / Linux desktop** — the engine is a *separate* process, so it
//!   injects a payload (a DLL / an `.so`) and shares one `SpeedClock` with it
//!   through named memory. The factor is set per attached target (see
//!   [`windows`] / [`linux`]).
//!
//! `set_factor` / `factor` route to whichever backend is active. The in-process
//! `CLOCK` only exists on Android, where the hook reads it locally.

pub mod clock;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(windows)]
pub mod windows;

#[cfg(all(unix, not(target_os = "android")))]
pub mod linux;

/// Sane bounds for a *running* speed. Outside these the target's own timeouts
/// (ANR watchdog, GC, input) get unstable enough that the demo stops being a
/// demo. `0.0` is special-cased as a freeze (pause) — see [`clamp`].
pub const MIN_FACTOR: f64 = 0.1;
pub const MAX_FACTOR: f64 = 8.0;

fn clamp(factor: f64) -> f64 {
    if !factor.is_finite() {
        return 1.0;
    }
    // 0 (or below) = freeze the clock (a true pause). The desktop backends hook
    // the target *cross-process*, so freezing the target's clock never touches
    // the scanner's own UI — a real pause. Android's in-process overlay must not
    // send 0 (it shares the target's main thread; a freeze would freeze the
    // overlay too), so there the "pause" slows to a low factor instead.
    if factor <= 0.0 {
        return 0.0;
    }
    factor.clamp(MIN_FACTOR, MAX_FACTOR)
}

/// Set the speed multiplier. `1.0` is real time.
pub fn set_factor(factor: f64) {
    let f = clamp(factor);
    #[cfg(windows)]
    {
        windows::set_factor(f);
    }
    #[cfg(target_os = "android")]
    {
        CLOCK.set_factor(android::real_monotonic_ns(), f);
    }
    #[cfg(all(unix, not(target_os = "android")))]
    {
        linux::set_factor(f);
    }
}

/// The current multiplier.
pub fn factor() -> f64 {
    #[cfg(windows)]
    {
        windows::factor()
    }
    #[cfg(target_os = "android")]
    {
        CLOCK.factor()
    }
    #[cfg(all(unix, not(target_os = "android")))]
    {
        linux::factor()
    }
}

/// Install the in-process hook (Android). Idempotent; returns the number of
/// hook points patched (0 = unsupported / nothing to do). The desktop injects a
/// payload instead — see [`inject`].
pub fn install() -> usize {
    #[cfg(target_os = "android")]
    {
        android::install()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// Inject the speedhack payload into another process (desktop, cross-process).
#[cfg(windows)]
pub fn inject(pid: u32, dll_path: &str) -> bool {
    windows::install(pid, dll_path)
}

/// Inject the speedhack payload `.so` into another process (Linux desktop).
#[cfg(all(unix, not(target_os = "android")))]
pub fn inject(pid: u32, so_path: &str) -> bool {
    linux::install(pid, so_path)
}

// -- in-process clock (Android only; desktop uses a shared one) ---------------

/// The one process-wide speed clock the in-process Android hook reads. `const`
/// so it needs no lazy init and the hook stub can reach it without a guard.
#[cfg(target_os = "android")]
static CLOCK: clock::SpeedClock = clock::SpeedClock::new();

/// Map a real monotonic time (ns) to scaled virtual time. The Android hook stub
/// calls this from inside its replacement `clock_gettime`.
#[cfg(target_os = "android")]
pub fn scale_ns(real_ns: u64) -> u64 {
    CLOCK.scale(real_ns)
}
