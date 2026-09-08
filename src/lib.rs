//! cheat-enginer — a memory scanner and editor.
//!
//! The crate is split so the scanning engine can be embedded without a UI:
//!
//! * [`platform`] — the five-method OS seam (`Platform` / `ProcessHandle`).
//! * [`scan`], [`address`], [`process`] — the engine proper. No UI, no I/O
//!   beyond the platform seam, no threads of its own.
//! * [`ui`] — the terminal front-end, behind the default `tui` feature.
//! * `android` — the JNI bridge, compiled only when targeting Android.
//!
//! Building with `--no-default-features` leaves just the engine, which is what
//! the Android library links against.

pub mod address;
pub mod error;
pub mod hex;
pub mod platform;
pub mod process;
pub mod scan;
pub mod session;

#[cfg(feature = "tui")]
pub mod ui;

#[cfg(target_os = "android")]
pub mod android;

/// Version and build date, stamped by `build.rs`. Shown in the status bar and
/// printed by `--version`.
pub const VERSION_LINE: &str = concat!(
    "cheat-enginer v",
    env!("CARGO_PKG_VERSION"),
    " (built ",
    env!("BUILD_DATE"),
    ")"
);
