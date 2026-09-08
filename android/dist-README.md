# 在實體 Android 手機上安裝與測試

三支 APK，全部是 debug build、全部用同一把 `~/.android/debug.keystore` 簽的。
**簽章相同是必要條件**，共用 uid 就靠它。

| 檔案 | package | 用途 | 權限 |
|---|---|---|---|
| `cheat-engine-v0.3.1.apk` | `dev.marc.ce.app` | 掃描器，自帶啟動器圖示 | 顯示在其他應用程式上層、前景服務、通知 |
| `dungeon-tap-v0.3.1.apk` | `dev.marc.ce.game` | 被掃描的小遊戲 | **零權限** |
| `ce-sample-embedded-v0.3.1.apk` | `dev.marc.ce.sample` | 嵌入模式示範（自己掃自己） | **零權限** |

- 支援 ABI：`arm64-v8a`（16 KB page 對齊）、`armeabi-v7a`、`x86_64`
- minSdk 24（Android 7.0）／targetSdk 35
- 遊戲 APK 只有 18 KB —— 裡面真的沒有引擎

---

## 最重要的一條規則

**package 的 uid 在安裝當下就固定了。** 事後才補 `sharedUserId` 是沒有作用的，
覆蓋安裝會失敗（`INSTALL_FAILED_UID_CHANGED` / `INSTALL_FAILED_SHARED_USER_INCOMPATIBLE`）。

所以只要之前裝過舊版，**一定要先兩支都解除安裝**：

```bash
adb uninstall dev.marc.ce.app
adb uninstall dev.marc.ce.game
```

裝好之後 CE 的首頁會自己比對 uid，若共用失敗會直接顯示紅字說明 ——
不用猜，因為另一種症狀只是「一切正常但掃不到東西」。

---

## 方式 A：用 adb 安裝（建議）

手機先開「開發人員選項 → USB 偵錯」，接上線後：

```bash
adb devices                      # 確認看得到手機

adb uninstall dev.marc.ce.app    # 沒裝過會顯示 Failure，可忽略
adb uninstall dev.marc.ce.game

adb install cheat-engine-v0.3.1.apk    # 先裝 CE
adb install dungeon-tap-v0.3.1.apk     # 再裝遊戲

# 可選：用指令直接給浮動視窗權限，省去手動點
adb shell appops set dev.marc.ce.app SYSTEM_ALERT_WINDOW allow
```

**先裝 CE 再裝遊戲。** CE 帶著原生庫，遊戲沒有；共用 uid 的 ABI 是一起協調的，
讓帶 `.so` 的那支先決定就不會有意外。

## 方式 B：不接電腦

把兩個 `.apk` 傳到手機（雲端硬碟、傳輸線、藍牙都可以），用檔案管理員點開安裝。
系統會問要不要允許「安裝未知來源的應用程式」，允許即可。順序一樣：CE 先、遊戲後。

---

## 測試流程

1. 開 **Cheat Engine**。首頁狀態區塊應該顯示：
   ```
   overlay  NOT GRANTED
   game     same uid 10xxx - shared process OK
   ```
   若 `game` 那行是 `NOT SHARED`，代表兩支沒共用成功 —— 兩支都解除安裝重來。

2. 點 **Grant overlay permission** → 在系統設定裡打開「顯示在其他應用程式上層」→ 返回。
   狀態應該翻成 `overlay granted`。

3. 點 **Start engine**。Android 13 以上會先問「要不要允許傳送通知」——
   允許或拒絕都不影響引擎運作，只是拒絕的話通知列上的 Stop 按鈕會看不到
   （所以啟動器上另外放了一顆 Stop）。接著畫面上出現一顆綠色 **CE** 小球。

4. 首頁下方「**同一個 UID 的 APP**」會列出所有跟 CE 共用 uid 的 App。
   綠框、標「同一行程 · 可掃描」的才是掃得到的；**點一下就啟動**。

   若某一列是琥珀色、寫「同 uid，但 process = …」，代表那支 App 雖然共用了
   uid、卻宣告了不同的 `android:process`，所以在別的行程裡 —— 這正是
   「兩支都裝得起來、跑得起來，掃描卻找不到東西」的失敗態。

5. 開啟之後，遊戲的 HUD 會印出 `pid` 與 `uid` ——
   **和 CE 首頁印的那組數字一模一樣，就代表兩支真的在同一個行程裡。**
   （這是最直接的證明，不需要接電腦。）

6. 玩幾波：點怪物扣血、打倒掉金幣。記下目前的 `gold`。

7. 點 CE 小球展開面板。**遊戲在面板開著時仍然可以玩** —— 這一點要特別確認。

8. `4 Bytes (u32)` → `Exact Value` → 輸入目前金幣 → **New Scan**。

9. 再打倒一隻怪（獎勵是不規則的 7–23，所以沒有「剛好加 N」的捷徑）→
   輸入新的金幣數 → **Next Scan**。重複到剩幾筆為止。

10. **Results** 分頁點那一列 → 跳到 **Address** 分頁 → 把值改成 `99999`。
   遊戲 HUD 會在 0.2 秒內變動，商店所有按鈕同時亮起。

11. 勾 **F**（凍結）→ 進商店連買五次。金幣完全不會扣 ——
    Rust 的凍結執行緒每 100 ms 把它寫回去。
    同時注意 `javaGold` 會漂走：同一個數字、兩個位置。

12. **Hex** 分頁貼上那個位址，`score`／`wave`／`atk`／`armor` 就排在旁邊。

> **懶人路線**：不想瞄準的話改掃 HP。HP 會自己往下掉，所以
> `Unknown Initial` → **New Scan** → 等兩秒 → `Decreased` → **Next Scan**，
> 重複四五輪就收斂了，全程不用打字。

### 想試零權限的嵌入模式

裝 `ce-sample-embedded-v0.3.1.apk` 就好，不需要任何權限、不需要另一支 App。
它示範的是同一個引擎被編進宿主 App 裡的情況（`TYPE_APPLICATION_PANEL` ＋ 宿主自己的 windowToken）。

---

## 真機上可能遇到的狀況

| 現象 | 原因與處理 |
|---|---|
| 安裝失敗 `INSTALL_FAILED_UID_CHANGED` | 之前裝過沒有 `sharedUserId` 的版本。兩支都解除安裝再裝。 |
| 安裝失敗 `INSTALL_FAILED_SHARED_USER_INCOMPATIBLE` | 兩支簽章不同。請用同一批 dist 裡的檔案，不要混用不同次建置的產物。 |
| CE 首頁顯示 `game ... NOT SHARED` | 同上，uid 沒共用成功。 |
| 按 Start engine 沒反應／小球沒出現 | 浮動視窗權限沒給，或被 ROM 另外擋住（見下一列）。 |
| 小球出現但切到遊戲就消失 | 國產 ROM（MIUI／ColorOS／Funtouch／EMUI）通常另外有「**後台彈出介面**」或「**顯示懸浮窗**」的獨立開關，以及會殺前景服務的省電策略。請把 CE 加入「自啟動」白名單並關掉針對它的電池最佳化。 |
| 掃描很慢或狀態列出現 `(capped)` | 正常。整個行程的位址空間很大，`ScanLimits` 會在 20 萬筆／32 MiB snapshot 處截斷以免拖垮手機。先用 `Exact Value` 而不是 `Unknown Initial` 會快很多。 |
| 凍結一陣子後失效 | 若你凍的是 `javaGold` 那種普通 Java 欄位，ART 的搬移式 GC 把物件搬走了 —— 這是 in-process 方案的本質限制，不是 bug，遊戲裡的 **Force GC** 按鈕就是用來演示這件事的。改凍 `gold`（direct ByteBuffer）就不會。 |
| 手機是 32 位元的舊機 | 已附 `armeabi-v7a`，可以跑。若真的載不到庫，overlay 會靜默不啟用而不是讓 App 掛掉。 |

## 已知會清掉狀態的操作

- 對**任一支** App 執行「強制停止」，會殺掉整個共用行程，掃描結果與凍結一起沒。
- 重新安裝任一支 APK 也一樣。
- 位址表存在 CE 的 `filesDir`，下次 Start 會自動載回來；掃描結果則不保留。

---

## 校驗碼

每支 APK 的 SHA-256 在同目錄的 `SHA256SUMS.txt`，由 `pack.sh` 每次建置時重新產生。

```bash
sha256sum -c SHA256SUMS.txt
```

三支 APK 的簽章憑證必須相同 —— 這是 `sharedUserId` 的硬性條件，`pack.sh` 會擋下不一致的建置：

```
CN=Android Debug, O=Android, C=US
SHA-256  2a23a8bcc7b056c02bd395823508c1ac69b4a7405b554420b49f1b77f1cdb104
```

## 重新產生

```bash
cd android
./pack.sh                # 建置 + 驗證 + 打包到 dist/
./pack.sh --install      # 順便裝到連著的裝置（會先解除安裝，順序也對）
./pack.sh --test         # 先跑 cargo test
./pack.sh --help
```

`pack.sh` 會擋下兩件會讓整個設計「安靜失效」的事：遊戲 APK 混進引擎，以及兩支簽章不一致。

架構說明（含 SVG + SMIL 動畫圖解）在 `../docs/architecture.html`。
