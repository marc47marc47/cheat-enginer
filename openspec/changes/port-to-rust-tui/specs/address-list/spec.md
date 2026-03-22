## ADDED Requirements

### Requirement: 新增位址項目
系統 SHALL 允許使用者將掃描結果中的位址加入位址表。

#### Scenario: 從掃描結果新增
- **WHEN** 使用者在掃描結果中選擇一個位址並新增到位址表
- **THEN** 該位址、型別與當前值出現在位址表中

#### Scenario: 手動新增
- **WHEN** 使用者手動輸入位址與型別
- **THEN** 該項目加入位址表

### Requirement: 刪除位址項目
系統 SHALL 允許使用者從位址表中刪除項目。

#### Scenario: 刪除選中項目
- **WHEN** 使用者選擇一個位址項目並按下刪除鍵
- **THEN** 該項目從位址表中移除

### Requirement: 即時數值更新
系統 SHALL 定期讀取位址表中所有項目的當前值並更新顯示。

#### Scenario: 數值變化
- **WHEN** 目標程序的記憶體值發生變化
- **THEN** 位址表中對應項目的顯示值在下次更新週期中反映新值

### Requirement: 鎖定數值
系統 SHALL 支援鎖定位址表項目的數值，持續將指定值寫回目標記憶體。

#### Scenario: 啟用鎖定
- **WHEN** 使用者對一個位址項目啟用鎖定
- **THEN** 系統定期將該項目的指定值寫入目標記憶體，直到使用者解除鎖定

#### Scenario: 解除鎖定
- **WHEN** 使用者對已鎖定的項目解除鎖定
- **THEN** 系統停止覆寫該位址的值

### Requirement: 編輯數值
系統 SHALL 允許使用者直接編輯位址表項目的值。

#### Scenario: 修改數值
- **WHEN** 使用者選擇一個項目並輸入新值
- **THEN** 新值立即寫入目標程序記憶體

### Requirement: 項目描述
系統 SHALL 允許使用者為位址表項目設定描述文字。

#### Scenario: 設定描述
- **WHEN** 使用者為項目輸入描述
- **THEN** 該描述顯示在位址表中對應項目旁

### Requirement: 儲存與載入 Cheat Table
系統 SHALL 支援將位址表儲存為 JSON 格式的 cheat table 檔案，並能載入先前儲存的檔案。

#### Scenario: 儲存
- **WHEN** 使用者選擇儲存 cheat table
- **THEN** 所有位址項目（位址、型別、描述、鎖定狀態與值）序列化為 JSON 並寫入檔案

#### Scenario: 載入
- **WHEN** 使用者選擇載入 cheat table 檔案
- **THEN** 檔案中的位址項目取代目前的位址表內容
