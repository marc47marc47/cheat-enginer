# Cheat Enginer

A memory scanner and editor for game hacking, inspired by [Cheat Engine](https://github.com/cheat-engine/cheat-engine). Built from scratch in Rust with a terminal-based UI (TUI).

![Rust](https://img.shields.io/badge/Rust-2024_edition-orange)
![Platform](https://img.shields.io/badge/Platform-Windows-blue)
![License](https://img.shields.io/badge/License-MIT-green)

## Features

- **Process Management** - Enumerate running processes with window titles, fuzzy search filtering, attach to target process
- **Memory Scanner** - First scan & subsequent filtering with multiple scan modes (exact, unknown initial, increased, decreased, changed, unchanged, greater/less than)
- **Value Types** - U8, U16, U32, U64, I8, I16, I32, I64, F32, F64
- **Address Table** - Save addresses of interest, freeze values, edit values/descriptions, save/load cheat tables (JSON)
- **Value Freezing** - Locked values are re-written every 100 ms; entries whose writes start failing are flagged in the table (`[!]`)
- **Auto Rescan** - `A` re-runs the current scan every 777 ms using whichever scan mode is selected. With mode set to *Unchanged* this repeatedly drops every address that moved, narrowing the set down to values holding steady - the classic way to isolate an unknown value
- **Hex Viewer** - View raw memory with hex + ASCII display, address jumping, changed-byte highlighting
- **Background Scanning** - Parallel scanning with `rayon`, runs in background thread with progress bar to keep UI responsive
- **Cross-platform Abstraction** - Platform trait layer for Windows (`windows-sys`) and Linux (`/proc` filesystem) support

## Screenshots

The scanner and the address table sit side by side, address table on the right:

```
 [explorer.exe:1234]  F1:Proc F3:Hex Tab:Panel | t:Type s:Mode v:Value Enter:Scan r:Reset A:Auto a:Add
┌ [t]ype ──────┐┌ [s]can mode ───────────────────┐┌ Addresses (2) ─────────────────────────────────┐
│ 4 Bytes (u32)││ Unchanged                      ││[ ] 0x7FF6A1B23040 U32         100 Player HP    │
└──────────────┘└────────────────────────────────┘│[F] 0x7FF6A1B23180 U32         999 Gold         │
┌ [v]alue → Enter:Next Scan ─────────────────────┐│                                                │
│                                                ││                                                │
└────────────────────────────────────────────────┘│                                                │
┌ Results: 1532 ↕3/1532 [AUTO 777ms] ────────────┐│                                                │
│ 0x7FF6A1B23040  100                            ││                                                │
│ 0x7FF6A1B23180  100                            ││                                                │
│ 0x7FF6A1B232C0  100                            ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
```

## Requirements

- **Rust** 1.85+ (edition 2024)
- **Windows** 10/11 (primary target)
- **Administrator privileges** required for reading/writing other process memory

## Building

```bash
cargo build --release
```

The binary will be at `target/release/cheat-enginer.exe`.

### Installing

Builds in release mode and copies the binary into `~/.cargo/bin`, so
`cheat-enginer` is on `PATH`:

```bash
cargo install --path . --force
```

`--force` is needed to overwrite a previously installed copy.

Check what is installed:

```bash
cheat-enginer --version
# cheat-enginer v0.2.0 (built YYYY-MM-DD)
```

The version comes from `Cargo.toml`; the build date is stamped by `build.rs` at
compile time and is also shown at the right edge of the status bar whenever the
terminal is wide enough.

## Usage

Run as administrator:

```bash
cheat-enginer.exe
```

### Keyboard Shortcuts

| Screen | Key | Action |
|--------|-----|--------|
| **All** | `F1` | Process list |
| | `F2` | Main (scanner + address table) |
| | `F3` | Hex viewer |
| | `Esc` | Quit confirmation |
| | `Ctrl+C` | Quit immediately |
| **Process List (F1)** | Type | Fuzzy search filter |
| | `Enter` | Attach to selected process |
| | `Up/Down` | Navigate |
| | `F5` | Refresh process list |
| **Scanner (F2)** | `t` | Cycle value type |
| | `s` | Cycle scan mode |
| | `v` | Edit search value |
| | `Enter` | Start scan |
| | `r` | Reset scan results |
| | `A` | Toggle auto rescan (re-runs the current scan every 777 ms) |
| | `a` | Add selected result to address table |
| | `Tab` | Switch between scanner and address table |
| **Address Table (F2)** | `f` | Toggle freeze on selected address (`[F]` locked, `[!]` writes failing) |
| | `e` | Edit value |
| | `d` | Edit description |
| | `S` | Save cheat table to file |
| | `L` | Load cheat table from file |
| | `Delete` | Remove selected address |
| **Hex Viewer (F3)** | `g` | Go to address |
| | `Up/Down` | Scroll |
| | `PageUp/PageDown` | Fast scroll |

## Downloads

Pre-built Windows binaries are available on the [Releases](https://github.com/marc47marc47/cheat-enginer/releases) page.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | Terminal UI framework |
| `crossterm` | Cross-platform terminal input/output |
| `windows-sys` | Windows API bindings (process, memory, window enumeration) |
| `rayon` | Parallel memory scanning |
| `serde` / `serde_json` | Cheat table serialization |
| `unicode-width` | Proper CJK character width handling |
| `anyhow` | Error handling |

## Architecture

```
build.rs                 # Stamps BUILD_DATE at compile time
src/
├── main.rs              # Entry point, --version, terminal setup, event loop
├── error.rs             # Error type alias
├── platform/
│   ├── mod.rs           # Platform trait abstraction
│   ├── windows.rs       # Windows implementation (windows-sys)
│   └── linux.rs         # Linux implementation (/proc)
├── process/
│   └── mod.rs           # Process filtering (fuzzy search)
├── scan/
│   ├── mod.rs
│   ├── value_type.rs    # Value types, scan types, parsing
│   ├── filter.rs        # Scan comparison logic
│   └── scanner.rs       # Scan engine (parallel with rayon, writable regions only)
├── address/
│   └── mod.rs           # Address table (CRUD, freeze, save/load)
└── ui/
    ├── mod.rs
    ├── app.rs           # App state machine, input handling, drawing
    ├── process_list.rs  # Process list view
    ├── scanner_view.rs  # Scanner controls & results view
    ├── address_list_view.rs  # Address table view
    └── hex_viewer.rs    # Hex memory viewer
```

## License

MIT
