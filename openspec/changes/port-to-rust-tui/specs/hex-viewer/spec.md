## ADDED Requirements

### Requirement: 十六進位記憶體顯示
系統 SHALL 以十六進位格式顯示目標程序的記憶體內容，每行顯示位址、十六進位位元組與 ASCII 表示。

#### Scenario: 顯示記憶體
- **WHEN** 使用者開啟 hex viewer 並指定起始位址
- **THEN** 顯示從該位址開始的記憶體內容，格式為 `位址 | XX XX XX ... | ASCII...`，每行 16 位元組

### Requirement: 捲動瀏覽
系統 SHALL 支援上下捲動以瀏覽不同記憶體區域。

#### Scenario: 向下捲動
- **WHEN** 使用者按下向下捲動鍵
- **THEN** 顯示區域向高位址方向移動

#### Scenario: 向上捲動
- **WHEN** 使用者按下向上捲動鍵
- **THEN** 顯示區域向低位址方向移動

### Requirement: 跳轉到位址
系統 SHALL 允許使用者輸入位址直接跳轉。

#### Scenario: 輸入位址跳轉
- **WHEN** 使用者輸入十六進位位址並確認
- **THEN** 顯示區域以該位址為起點重新渲染

### Requirement: 即時更新
系統 SHALL 定期重新讀取並更新顯示的記憶體內容。

#### Scenario: 記憶體值變化
- **WHEN** 目標程序記憶體值改變
- **THEN** hex viewer 在下次更新週期中反映新值

### Requirement: 位元組高亮
系統 SHALL 對自上次更新以來發生變化的位元組進行高亮顯示。

#### Scenario: 變化的位元組
- **WHEN** 記憶體值在兩次更新之間改變
- **THEN** 改變的位元組以不同顏色高亮顯示
