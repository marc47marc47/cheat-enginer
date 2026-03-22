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
- **Hex Viewer** - View raw memory with hex + ASCII display, address jumping, changed-byte highlighting
- **Background Scanning** - Parallel scanning with `rayon`, runs in background thread with progress bar to keep UI responsive
- **Cross-platform Abstraction** - Platform trait layer for Windows (`windows-sys`) and Linux (`/proc` filesystem) support

## Screenshots

```
 [explorer.exe:1234]  F1:Proc F2:Main F3:Hex Tab:Panel t:Type s:Mode v:Value Enter:Scan
┌─ [t]ype ──┐┌─ [s]can mode ─┐┌─ [v]alue → Enter:First Scan ──────────┐
│ U32       ││ Exact Value   ││ 100                                    │
└───────────┘└───────────────┘└────────────────────────────────────────┘
┌─ Results: 1532 | [a]dd to table | [r]eset ───────────────────────────┐
│ 0x00007FF6A1B23040  100                                              │
│ 0x00007FF6A1B23180  100                                              │
└──────────────────────────────────────────────────────────────────────┘
┌─ Address Table (2) | [f]reeze [e]dit [d]esc [S]ave [L]oad Del:Remove ┐
│ [ ] 0x00007FF6A1B23040       U32          100  Player HP             │
│ [F] 0x00007FF6A1B23180       U32          999  Gold (frozen)         │
└──────────────────────────────────────────────────────────────────────┘
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
| | `a` | Add selected result to address table |
| | `Tab` | Switch between scanner and address table |
| **Address Table (F2)** | `f` | Toggle freeze on selected address |
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
src/
├── main.rs              # Entry point, terminal setup, event loop
├── error.rs             # Error type alias
├── platform/
│   ├── mod.rs           # Platform trait abstraction
│   ├── windows.rs       # Windows implementation (windows-sys)
│   └── linux.rs         # Linux implementation (/proc)
├── process/
│   └── mod.rs           # Process filtering (fuzzy search)
├── memory/
│   └── mod.rs           # Memory region re-exports
├── scan/
│   ├── mod.rs
│   ├── value_type.rs    # Value types, scan types, parsing
│   ├── filter.rs        # Scan comparison logic
│   └── scanner.rs       # Scan engine (parallel with rayon)
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
