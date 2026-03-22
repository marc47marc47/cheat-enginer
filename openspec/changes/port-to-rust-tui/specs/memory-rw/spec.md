## ADDED Requirements

### Requirement: 讀取程序記憶體
系統 SHALL 能從目標程序的指定位址讀取指定長度的原始位元組。

#### Scenario: 成功讀取
- **WHEN** 提供有效的位址與長度
- **THEN** 回傳該位址範圍的原始位元組資料

#### Scenario: 讀取無效位址
- **WHEN** 提供的位址不可讀（未映射或受保護）
- **THEN** 回傳錯誤，不造成程式崩潰

### Requirement: 寫入程序記憶體
系統 SHALL 能向目標程序的指定位址寫入位元組資料。

#### Scenario: 成功寫入
- **WHEN** 提供有效的可寫位址與資料
- **THEN** 目標程序記憶體被修改為指定值

#### Scenario: 寫入唯讀位址
- **WHEN** 嘗試寫入受保護的記憶體區域
- **THEN** 回傳錯誤，目標記憶體不受影響

### Requirement: 列舉記憶體區域
系統 SHALL 能列舉目標程序的所有記憶體區域，包含起始位址、大小與保護屬性。

#### Scenario: 列舉可讀區域
- **WHEN** 請求列舉記憶體區域
- **THEN** 回傳所有記憶體區域的列表，包含基址、大小、保護旗標（可讀/可寫/可執行）

### Requirement: 平台抽象
記憶體讀寫操作 SHALL 透過統一的 trait 介面抽象，各平台提供具體實作。

#### Scenario: 透過 trait 使用
- **WHEN** 呼叫記憶體讀寫 trait 方法
- **THEN** 自動分派至當前平台的實作（Windows: ReadProcessMemory/WriteProcessMemory，Linux: process_vm_readv/process_vm_writev）
