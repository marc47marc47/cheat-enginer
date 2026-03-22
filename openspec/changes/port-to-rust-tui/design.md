## Context

Cheat Engine 是一個以 Pascal/Lazarus 開發的記憶體掃描與除錯工具，包含 416+ 個 Pascal 原始檔。目前專案根目錄已有一個空的 Rust 專案 (edition 2024)。目標是將核心功能移植為 Rust 實作，以 TUI 介面取代 Lazarus GUI，優先實作記憶體掃描、程序附加、位址管理與十六進位檢視等基礎功能。

現有 Cheat Engine 的進階功能（Lua 腳本、核心驅動 DBKKernel/DBVM、外掛系統、反組譯器、除錯器）不在第一階段範圍內。

## Goals / Non-Goals

**Goals:**
- 建立模組化的 Rust 核心庫，將平台抽象與業務邏輯分離
- 實作跨平台程序列舉與附加 (Windows / Linux)
- 實作高效的多執行緒記憶體掃描引擎
- 實作基於 ratatui 的 TUI 介面，提供流暢的鍵盤操作體驗
- 支援 cheat table 檔案的儲存與載入
- 確保記憶體安全，利用 Rust 型別系統防止常見錯誤

**Non-Goals:**
- 不實作 Lua 腳本引擎
- 不實作核心態驅動 (kernel driver) 功能
- 不實作反組譯器或除錯器
- 不實作外掛系統
- 不實作 GUI 介面
- 不實作遠端除錯 (ceserver)
- 不實作 .NET/Java/Mono 檢查器
- 不實作 DirectX 掛鉤或速度修改

## Decisions

### 1. TUI 框架：ratatui + crossterm

**選擇**: 使用 `ratatui`（前身為 tui-rs）搭配 `crossterm` 作為後端。

**替代方案**:
- `cursive`: 更高層級的抽象，但自訂性較差，社群較小
- `egui` (即時模式 GUI): 不符合 TUI 需求，需要圖形環境

**理由**: ratatui 是 Rust TUI 生態中最活躍的專案，API 設計良好，crossterm 提供跨平台終端支援。

### 2. 架構：分層模組設計

**選擇**: 將專案分為以下模組：
```
src/
├── main.rs            # 入口點
├── app.rs             # 應用狀態與事件循環
├── platform/          # 平台抽象層
│   ├── mod.rs         # trait 定義
│   ├── windows.rs     # Windows 實作
│   └── linux.rs       # Linux 實作
├── process/           # 程序管理
│   └── mod.rs
├── memory/            # 記憶體操作
│   ├── mod.rs
│   ├── reader.rs      # 記憶體讀寫
│   └── scanner.rs     # 掃描引擎
├── scan/              # 掃描邏輯
│   ├── mod.rs
│   ├── value_type.rs  # 數值型別定義
│   └── filter.rs      # 篩選邏輯
├── address/           # 位址表管理
│   ├── mod.rs
│   └── table.rs       # cheat table 序列化
├── ui/                # TUI 介面
│   ├── mod.rs
│   ├── app.rs         # 主要 UI 狀態機
│   ├── process_list.rs
│   ├── scanner.rs
│   ├── address_list.rs
│   └── hex_viewer.rs
└── error.rs           # 錯誤型別
```

**理由**: 清晰的關注點分離，平台抽象層使跨平台支援容易擴展，UI 與核心邏輯解耦便於測試。

### 3. 記憶體掃描策略：分區塊多執行緒掃描

**選擇**: 使用 `rayon` 進行平行掃描，將目標程序的可讀記憶體區域分割為區塊，各執行緒獨立掃描後合併結果。

**替代方案**:
- 單執行緒逐頁掃描：實作簡單但效能差
- async/await：記憶體掃描是 CPU 密集型，async 無明顯優勢

**理由**: Cheat Engine 原版也採用多執行緒掃描，rayon 的工作竊取排程器能自動平衡負載。

### 4. 數值型別系統：Rust enum

**選擇**: 使用 Rust enum 表示掃描值型別：
```rust
enum ScanValue {
    U8(u8), U16(u16), U32(u32), U64(u64),
    I8(i8), I16(i16), I32(i32), I64(i64),
    F32(f32), F64(f64),
    Bytes(Vec<u8>),
    String(String),
}
```

**理由**: 型別安全，模式匹配確保所有型別都被處理，編譯期即可捕獲遺漏。

### 5. Cheat Table 格式：JSON

**選擇**: 使用 JSON 格式儲存 cheat table，透過 `serde` 序列化。

**替代方案**:
- XML（Cheat Engine 原生格式）：較冗長，解析較慢
- 自訂二進位格式：不便於人工檢視與版本控制

**理由**: JSON 在 Rust 生態有極佳支援（serde_json），人類可讀，易於除錯。未來可考慮相容 CE 原始 XML 格式的匯入功能。

### 6. 平台 API 抽象

**選擇**: 定義 `Platform` trait，包含程序列舉、記憶體讀寫等操作，各平台提供具體實作。

**Windows**: `OpenProcess` + `ReadProcessMemory` / `WriteProcessMemory` + `VirtualQueryEx`（透過 `windows-sys` crate）
**Linux**: `/proc/pid/maps` + `process_vm_readv` / `process_vm_writev`（透過 `nix` crate）

**理由**: 避免 `#[cfg]` 散布在業務邏輯中，集中平台差異於抽象層。

## Risks / Trade-offs

- **[權限不足]** → 在 Windows 需要管理員權限，Linux 需要 `CAP_SYS_PTRACE`。啟動時檢測權限並提示使用者。
- **[功能差距]** → 第一階段不包含原版 CE 的進階功能（除錯器、Lua、核心驅動）。透過模組化設計確保未來可逐步擴展。
- **[效能]** → 純使用者態 API 讀取記憶體較核心驅動慢。對多數使用場景已足夠，核心驅動可作為未來擴展。
- **[TUI 限制]** → TUI 無法呈現與 GUI 同等的視覺豐富度。但 TUI 的鍵盤導向操作對進階使用者更高效，且可在無 GUI 環境運行。
- **[跨平台測試]** → 需要在 Windows 和 Linux 上分別測試。CI 可使用 GitHub Actions 的多平台 runner。
