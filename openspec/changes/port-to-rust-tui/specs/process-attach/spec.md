## ADDED Requirements

### Requirement: 程序列舉
系統 SHALL 列舉所有正在執行的程序，顯示 PID 與程序名稱。

#### Scenario: 列舉系統程序
- **WHEN** 使用者進入程序選擇畫面
- **THEN** 系統顯示所有可見程序的 PID 與名稱列表，按 PID 排序

#### Scenario: 重新整理程序列表
- **WHEN** 使用者按下重新整理鍵
- **THEN** 系統重新列舉程序並更新列表

### Requirement: 程序篩選
系統 SHALL 支援依名稱篩選程序列表。

#### Scenario: 輸入篩選關鍵字
- **WHEN** 使用者在篩選欄位輸入文字
- **THEN** 列表即時篩選，僅顯示名稱包含該文字的程序（不分大小寫）

### Requirement: 附加程序
系統 SHALL 允許使用者選擇並附加到目標程序。

#### Scenario: 成功附加
- **WHEN** 使用者選擇一個程序並確認附加
- **THEN** 系統取得該程序的讀寫存取權限，並導航至主掃描畫面

#### Scenario: 附加失敗（權限不足）
- **WHEN** 使用者嘗試附加到無權限的程序
- **THEN** 系統顯示錯誤訊息，說明需要提升權限

### Requirement: 跨平台支援
程序列舉與附加 SHALL 透過平台抽象層支援 Windows 與 Linux。

#### Scenario: Windows 平台
- **WHEN** 在 Windows 上執行
- **THEN** 使用 Windows API（CreateToolhelp32Snapshot、OpenProcess）列舉與附加程序

#### Scenario: Linux 平台
- **WHEN** 在 Linux 上執行
- **THEN** 使用 /proc 檔案系統列舉程序，使用 ptrace 或 process_vm_readv 附加
