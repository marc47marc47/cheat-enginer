//! Manual test harness for the Windows speedhack (stands in for the TUI control
//! until Stage B3). Injects the payload into a target pid and sets a factor.
//!
//!   cargo run --example speed_inject -- <pid> <factor>
//!
//! The payload keeps the shared section alive, so the target stays hooked at the
//! last factor even after this process exits.

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pid: u32 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .expect("usage: speed_inject <pid> <factor>");
    let factor: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4.0);

    let dll = std::path::Path::new("speedhook-payload/target/release/ce_speedhook.dll")
        .canonicalize()
        .expect("build the payload first: (cd speedhook-payload && cargo build --release)");
    let dll = dll.to_string_lossy().into_owned();

    println!("inject {dll}\n    into pid {pid}, factor {factor}");
    let ok = ce_engine::speedhack::inject(pid, &dll);
    println!("inject returned: {ok}");
    if !ok {
        std::process::exit(1);
    }
    // Give the payload's install thread a moment to patch the IAT.
    std::thread::sleep(std::time::Duration::from_millis(300));
    ce_engine::speedhack::set_factor(factor);
    println!("factor now {:.2}x", ce_engine::speedhack::factor());
    // Hold briefly so the write is visible; the DLL keeps the section afterward.
    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("speed_inject is Windows-only");
}
