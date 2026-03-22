## Why

Cheat Engine 原始碼以 Pascal/Delphi (Lazarus) 編寫，包含超過 416 個 Pascal 原始檔與 148 個 GUI 表單。Pascal 生態系的現代化工具鏈、套件管理和社群支援日漸萎縮，且 Lazarus GUI 框架僅支援桌面環境。將核心功能移植為 Rust 可獲得記憶體安全保證、現代化套件生態 (crates.io)、更佳的跨平台支援，並以 TUI 介面取代笨重的 GUI，使工具可在無桌面環境（SSH、容器）中運行。

## What Changes

- **新增** Rust 版記憶體掃描引擎，支援多種數值型別（byte、word、dword、qword、float、double、string）的精確/模糊搜尋
- **新增** 跨平台程序列舉與附加機制（Windows API / Linux procfs）
- **新增** 記憶體讀寫模組，支援透過系統 API 讀寫目標程序記憶體
- **新增** 基於 ratatui 的 TUI 介面，提供程序選擇、記憶體掃描、結果列表、位址監控等互動畫面
- **新增** 位址表 (address list) 管理，支援新增、刪除、鎖定數值、儲存/載入 cheat table
- **新增** 十六進位記憶體檢視器 (hex viewer)
- **移除** Pascal/Lazarus GUI、Lua 腳本引擎、核心驅動 (DBKKernel/DBVM)、外掛系統等進階功能（第一階段不移植）

## Capabilities

### New Capabilities
- `process-attach`: 程序列舉、選擇與附加，跨平台抽象（Windows/Linux）
- `memory-scan`: 記憶體掃描引擎，支援首次掃描與後續篩選，多種數值型別與比較模式
- `memory-rw`: 讀寫目標程序記憶體的底層模組，封裝平台 API
- `address-list`: 位址表管理，支援鎖定數值、標記、儲存/載入 cheat table 檔案
- `hex-viewer`: 十六進位記憶體檢視器，支援捲動、跳轉、即時更新
- `tui-app`: ratatui 為基礎的 TUI 應用程式框架，包含畫面路由、鍵盤操作、狀態管理

### Modified Capabilities
<!-- 無既有規格需要修改 -->

## Impact

- **程式碼**: 全新 Rust 專案，建構於現有 `Cargo.toml`（edition 2024）之上
- **依賴**: 新增 `ratatui`、`crossterm`、`sysinfo`、`windows-sys`/`nix` 等 crate
- **API**: 無外部 API 影響，純本地工具
- **系統**: 需要目標平台的程序存取權限（Windows: `OpenProcess`、Linux: `process_vm_readv` 或 `/proc/pid/mem`）
