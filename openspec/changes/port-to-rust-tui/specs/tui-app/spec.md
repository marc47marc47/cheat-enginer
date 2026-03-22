## ADDED Requirements

### Requirement: 應用程式啟動
系統 SHALL 啟動後進入 TUI 模式，接管終端並顯示程序選擇畫面。

#### Scenario: 正常啟動
- **WHEN** 使用者執行程式
- **THEN** 終端切換至替代螢幕，顯示程序選擇畫面

#### Scenario: 結束程式
- **WHEN** 使用者按下 Ctrl+C 或 q 鍵（非輸入模式時）
- **THEN** 終端恢復原始狀態，程式正常退出

### Requirement: 畫面導航
系統 SHALL 支援在不同功能畫面之間切換：程序選擇、記憶體掃描（主畫面）、位址表、hex viewer。

#### Scenario: Tab 鍵切換
- **WHEN** 使用者按下 Tab 鍵
- **THEN** 焦點在主畫面的不同面板之間循環切換

#### Scenario: 快捷鍵導航
- **WHEN** 使用者按下對應功能鍵（如 F1-F4）
- **THEN** 切換至對應的功能畫面

### Requirement: 鍵盤操作
系統 SHALL 完全透過鍵盤操作，提供一致的快捷鍵系統。

#### Scenario: 方向鍵導航
- **WHEN** 使用者在列表中按上/下方向鍵
- **THEN** 選擇項目上/下移動

#### Scenario: Enter 確認
- **WHEN** 使用者按下 Enter
- **THEN** 執行當前上下文的主要動作（附加程序、開始掃描、編輯值等）

#### Scenario: Esc 取消
- **WHEN** 使用者按下 Esc
- **THEN** 取消當前操作或返回上一層

### Requirement: 狀態列
系統 SHALL 在畫面底部顯示狀態列，包含已附加程序資訊與可用快捷鍵提示。

#### Scenario: 顯示狀態
- **WHEN** 已附加至程序
- **THEN** 狀態列顯示程序名稱、PID 與可用操作的快捷鍵

### Requirement: 主畫面佈局
主掃描畫面 SHALL 分為上下兩個區塊：上方為掃描控制與結果，下方為位址表。

#### Scenario: 主畫面顯示
- **WHEN** 使用者附加程序後進入主畫面
- **THEN** 畫面分為掃描區域（含型別選擇、值輸入、掃描按鈕、結果列表）與位址表區域

### Requirement: 輸入模式
系統 SHALL 區分「一般模式」與「輸入模式」，輸入模式時鍵盤事件導向文字輸入欄位。

#### Scenario: 進入輸入模式
- **WHEN** 使用者選擇需要文字輸入的欄位（如掃描值、位址）
- **THEN** 進入輸入模式，快捷鍵暫停，鍵盤輸入導向該欄位

#### Scenario: 離開輸入模式
- **WHEN** 使用者按下 Enter 或 Esc
- **THEN** 回到一般模式，快捷鍵恢復運作

### Requirement: 錯誤訊息顯示
系統 SHALL 在操作失敗時顯示錯誤訊息。

#### Scenario: 顯示錯誤
- **WHEN** 操作失敗（如附加程序失敗、記憶體讀取失敗）
- **THEN** 在狀態列或彈出區域顯示錯誤訊息，數秒後自動消失
