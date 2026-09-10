//! The scaling math shared by every speedhack backend.
//!
//! A speedhack works by making a game read a *scaled* clock. This type owns the
//! scaling and nothing else: given the real monotonic time in nanoseconds it
//! returns a virtual time that advances `factor`× as fast.
//!
//! ```text
//! virt = virt0 + (real_now - real0) * factor
//! ```
//!
//! Changing the factor re-anchors `(real0, virt0)` to *now*, so the virtual
//! output stays **continuous** (never jumps at the moment of change) and
//! **monotonic non-decreasing** (for any `factor > 0`). Those two properties are
//! what keep a game's timers sane while the multiplier changes underneath them.
//!
//! # Concurrency
//! The hook stub calls [`SpeedClock::scale`] on the target's own threads, once
//! per `clock_gettime` — which is *hot*. Factor changes come from a user tapping
//! a button — *rare*. That read-heavy / write-rare shape is exactly a seqlock:
//! readers never block and never take a lock, writers bump an odd/even sequence
//! around the update. A mutex on this path would be a needless source of
//! contention (and, on the target's main thread, of jank).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

/// A monotonic clock whose rate can be scaled at runtime. See the module docs.
///
/// `#[repr(C)]` because the desktop backend places one of these in shared memory
/// and drives it from two processes at once — the scanner (writer) and an
/// injected payload (reader). A fixed field order is what lets both map the same
/// bytes as the same type. The fields are all atomics, so cross-process
/// lock-free access is sound: the CPU's atomic instructions operate on the
/// shared physical page.
#[repr(C)]
pub struct SpeedClock {
    /// Even = readable, odd = a writer is mid-update.
    seq: AtomicU32,
    /// Real monotonic time (ns) at the last anchor.
    real0: AtomicU64,
    /// Virtual time (ns) at the last anchor.
    virt0: AtomicU64,
    /// Multiplier, as `f64::to_bits`.
    factor_bits: AtomicU64,
}

impl SpeedClock {
    /// Identity clock: `factor = 1.0`, anchored at the origin, so `scale(t) == t`
    /// until [`set_factor`](Self::set_factor) moves the anchor.
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            real0: AtomicU64::new(0),
            virt0: AtomicU64::new(0),
            factor_bits: AtomicU64::new(1.0f64.to_bits()),
        }
    }

    /// Current multiplier. `1.0` means the clock runs at real time.
    pub fn factor(&self) -> f64 {
        // A torn read here only misreports the factor for display; the scaling
        // path uses the seqlock read below. Relaxed is fine.
        f64::from_bits(self.factor_bits.load(Ordering::Relaxed))
    }

    /// Map a real monotonic time to virtual time. Lock-free, safe from any
    /// thread. Retries only if a factor change lands mid-read (microseconds).
    pub fn scale(&self, real_ns: u64) -> u64 {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                // Writer in progress; let it finish.
                std::hint::spin_loop();
                continue;
            }
            let real0 = self.real0.load(Ordering::Relaxed);
            let virt0 = self.virt0.load(Ordering::Relaxed);
            let factor = f64::from_bits(self.factor_bits.load(Ordering::Relaxed));
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return apply(real0, virt0, factor, real_ns);
            }
            // The triple changed under us; read again.
        }
    }

    /// Change the multiplier, re-anchoring at `real_now_ns` so the virtual clock
    /// does not jump. `factor` must be `> 0`; callers clamp to a sane range.
    ///
    /// `real_now_ns` must come from the **same real source** that
    /// [`scale`](Self::scale) is fed — on Android that is the saved, un-hooked
    /// `clock_gettime`; feeding it a different clock would reintroduce the jump
    /// this method exists to avoid.
    pub fn set_factor(&self, real_now_ns: u64, factor: f64) {
        // Virtual time *now*, under the current (old) parameters — this becomes
        // the new virt0 so output is continuous across the change.
        let virt_now = self.scale(real_now_ns);

        // seqlock write: odd → update → even.
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Release);
        fence(Ordering::Release);
        self.real0.store(real_now_ns, Ordering::Relaxed);
        self.virt0.store(virt_now, Ordering::Relaxed);
        self.factor_bits.store(factor.to_bits(), Ordering::Relaxed);
        fence(Ordering::Release);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }
}

impl Default for SpeedClock {
    fn default() -> Self {
        Self::new()
    }
}

/// `virt0 + (real_ns - real0) * factor`, clamped so it never runs backwards.
fn apply(real0: u64, virt0: u64, factor: f64, real_ns: u64) -> u64 {
    let delta = real_ns.saturating_sub(real0) as f64;
    let scaled = delta * factor;
    // Round to nearest ns; guard against negative/overflow just in case.
    let virt = virt0 as f64 + scaled;
    if virt <= virt0 as f64 {
        virt0
    } else {
        virt.round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_by_default() {
        let c = SpeedClock::new();
        assert_eq!(c.scale(0), 0);
        assert_eq!(c.scale(1_000), 1_000);
        assert_eq!(c.scale(5_000_000_000), 5_000_000_000);
        assert_eq!(c.factor(), 1.0);
    }

    #[test]
    fn doubles_the_delta_after_2x() {
        let c = SpeedClock::new();
        // Up to the change the clock is real time.
        assert_eq!(c.scale(1_000), 1_000);
        c.set_factor(1_000, 2.0);
        // No jump at the anchor.
        assert_eq!(c.scale(1_000), 1_000);
        // Then time runs twice as fast: +1000 real → +2000 virtual.
        assert_eq!(c.scale(2_000), 3_000);
        assert_eq!(c.scale(3_000), 5_000);
        assert_eq!(c.factor(), 2.0);
    }

    #[test]
    fn halves_the_delta_after_slowmo() {
        let c = SpeedClock::new();
        c.set_factor(0, 0.5);
        assert_eq!(c.scale(0), 0);
        assert_eq!(c.scale(1_000), 500);
        assert_eq!(c.scale(4_000), 2_000);
    }

    #[test]
    fn stays_continuous_across_a_change() {
        let c = SpeedClock::new();
        // Run at 4x for a while.
        c.set_factor(0, 4.0);
        let before = c.scale(1_000); // 4000
        assert_eq!(before, 4_000);
        // Switch to 0.25x at that same instant — output must not jump.
        c.set_factor(1_000, 0.25);
        assert_eq!(c.scale(1_000), before);
        // Now creeps at quarter speed from 4000.
        assert_eq!(c.scale(5_000), 5_000);
    }

    #[test]
    fn monotonic_across_changes() {
        let c = SpeedClock::new();
        let mut last = 0u64;
        let mut real = 0u64;
        for (i, &f) in [1.0, 3.0, 0.2, 8.0, 0.5, 1.0].iter().enumerate() {
            c.set_factor(real, f);
            for _ in 0..1000 {
                real += 100 + (i as u64); // strictly increasing real time
                let v = c.scale(real);
                assert!(v >= last, "virt went backwards: {v} < {last} at factor {f}");
                last = v;
            }
        }
    }

    #[test]
    fn concurrent_reads_stay_monotonic() {
        // The property under concurrency: with ONE real clock that only moves
        // forward, every reader sees virtual time that never goes backwards,
        // even while the factor is hammered from another thread. That exercises
        // the seqlock (a torn read would surface as a non-monotonic jump) and
        // encodes the physical fact that a factor change happens "now", never
        // behind a time a reader has already observed.
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::thread;

        let c = Arc::new(SpeedClock::new());
        let real = Arc::new(AtomicU64::new(0)); // the shared, forward-only clock
        let stop = Arc::new(AtomicBool::new(false));

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let c = Arc::clone(&c);
                let real = Arc::clone(&real);
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    let mut last = 0u64;
                    while !stop.load(Ordering::Relaxed) {
                        let r = real.load(Ordering::Relaxed);
                        let v = c.scale(r);
                        assert!(v >= last, "virt went backwards: {v} < {last}");
                        last = v;
                    }
                })
            })
            .collect();

        // Advance the one real clock and change the factor at that same instant.
        for i in 0..5000 {
            let now = real.fetch_add(1_000, Ordering::Relaxed) + 1_000;
            let f = [0.5, 1.0, 2.0, 4.0][i % 4];
            c.set_factor(now, f);
        }
        stop.store(true, Ordering::Relaxed);
        for r in readers {
            r.join().unwrap();
        }
    }
}
