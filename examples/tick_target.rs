//! A tiny "game" whose clock the desktop speedhack can bend — the desktop
//! counterpart to Dungeon Tap.
//!
//! It derives a game clock from the OS performance counter
//! (`QueryPerformanceCounter` on Windows, `clock_gettime(CLOCK_MONOTONIC)` on
//! Linux) and prints it a few times a second. The print cadence uses an
//! ordinary wall-clock `sleep`, which the speedhack does not touch; the game
//! clock uses the performance counter, which it does. So when the engine scales
//! that counter, the printed game clock visibly runs fast or slow while the
//! line keeps refreshing at the same rate — exactly the signal a speedhack is
//! meant to produce.
//!
//! Run it, note its pid, attach the TUI to that pid, and drive the Speed
//! control.
//!
//!   cargo run --example tick_target

use std::io::Write;
use std::time::Duration;

fn main() {
    let start = perf_now_ns();
    println!(
        "tick_target  pid {}  — attach the scanner to this pid and bend its clock",
        std::process::id()
    );
    let mut ticks: u64 = 0;
    loop {
        let elapsed = perf_now_ns().saturating_sub(start);
        let game_secs = elapsed as f64 / 1e9;
        // One tick per 100 ms of *game* time. Under 4x the counter climbs four
        // times faster than the wall clock; under 0.25x, four times slower.
        let want = (game_secs * 10.0) as u64;
        if want > ticks {
            ticks = want;
        }
        print!(
            "\rgame clock {:8.2}s   ticks {:8}   (wall refresh is steady)   ",
            game_secs, ticks
        );
        let _ = std::io::stdout().flush();
        // Wall-clock sleep: a relative delay, not read off the perf counter, so
        // the speedhack leaves the refresh rate alone.
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
fn perf_now_ns() -> u64 {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };
    let mut freq: i64 = 0;
    let mut count: i64 = 0;
    unsafe {
        QueryPerformanceFrequency(&mut freq);
        QueryPerformanceCounter(&mut count);
    }
    if freq == 0 {
        return 0;
    }
    // count / freq seconds → ns, done as u128 to avoid overflow.
    ((count as u128 * 1_000_000_000u128) / freq as u128) as u64
}

#[cfg(not(windows))]
fn perf_now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64)
        .wrapping_mul(1_000_000_000)
        .wrapping_add(ts.tv_nsec as u64)
}
