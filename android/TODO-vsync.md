# TODO — Speed 分頁「解除 vsync 鎖」實驗開關(Android)

讓 dungeon-tap 的**移動動畫**也能跟著變速倍率變快/變慢。背景見
`android/TODO-speedup.md` 的 Choreographer/vsync 條目:移動被硬體 vsync 鎖 60fps,
時鐘 hook 縮放的 frame step 因主執行緒睡在 `epoll_pwait(-1)` 而無法提早跑。

兩個**純 Java**(overlay 內、不動 native 引擎)實驗 lever,放進 Speed 分頁勾選:
1. **Frame-pacing pulse** — 背景緒每 ~1ms 戳主 Looper 使其不睡 → 已縮放、早到期的 step 以
   縮放後頻率 dispatch → 移動 4×/0.25×。**真正有效**,代價吃 CPU。
2. **Refresh-rate 請求** — 設 overlay 視窗 `preferredRefreshRate`,best-effort 讓顯示降 Hz →
   遊戲 vsync 變慢。只在顯示器支援該模式時生效(emulator 多半 60Hz 單模式 → no-op)。

計畫全文見 `~/.claude/plans/melodic-bouncing-swing.md`。

---

## Stages

- [x] **S0 追蹤檔** — 建本檔(Stages/Issues/Log 三區塊,種入議題)。
      _驗:檔案存在、格式對。_
- [x] **S1 Controller:frame-pacing pulse** — `OverlayController` 加
      `setFramePacing(boolean)`/`isFramePacing()` + daemon pacer thread
      (`mainHandler.post(NOOP); sleep(1ms)` 迴圈)+ `volatile` 旗標;teardown 停 pacer join 清理。
      _驗:`:overlay:compileDebugJavaWithJavac` 過;on/off 不崩、無執行緒外洩。_
- [x] **S2 Controller:refresh-rate** — 加 `setPreferredRefreshRate(float)`/`preferredRefreshRate()`
      ,存 `desiredRefreshRate`;panel attach 時 `updateViewLayout(panel, lp)`(比照現有 bubble
      寫法),`showPanel` 每次套回。
      _驗:`:overlay` 編過;設 hz 不崩(生效與否看顯示模式)。_
- [x] **S3 Speed 分頁 UI** — `OverlayPanel.buildSpeedTab()` 接在 `speedStatus` 後加 CheckBox
      「Unlock frame pacing (exp.·high CPU)」→ `setFramePacing`;Refresh ChoiceStrip
      「Auto/30/60/90/120」→ `setPreferredRefreshRate`;各一行 DIM 說明;重建分頁時用
      `controller.isFramePacing()`/`preferredRefreshRate()` 還原狀態。
      _驗:`:overlay:compileDebugJavaWithJavac` 過;實機分頁渲染正常、勾選/chips 有反應。_
- [x] **S4 實機驗證(emulator)** — pack+install,4× 勾 pulse 錄影量幀間運動 ~4×、取消回 ~1×;
      0.25×+pulse 慢動作;Refresh 30 觀察(預期 no-op、記錄);無 crash/ANR、CPU 取消後回落。
      _驗:數字取自實測、寫進 Log。_

---

## Issues / 待決

- [open] **pacer CPU / ANR** — 背景 1ms 脈衝 + 主執行緒更忙;主執行緒不阻塞(input/vsync 仍
  處理)故 ANR 風險低,但標 experimental。teardown 必停(`setFramePacing(false)` + join),避免
  執行緒外洩。
- [open] **Lever 2 在單模式顯示 no-op** — emulator 多半只有 60Hz 單模式 → `preferredRefreshRate`
  無效。UI 已講明「best-effort; only if the display supports that mode」,屬預期。
- [open] **行程全域節奏** — pacing 讓整個 process 的 Handler 都以縮放後節奏跑(attack/HUD 本
  就縮放,移動新解鎖)——一致、正確,非 bug。
- [open] **面板重建狀態還原** — overlay 面板每次重開會重建 View → pacing/refresh 狀態存在
  `OverlayController`,`buildSpeedTab()` 時還原勾選/chips。
- [resolved-partial] **frame-pacing 在 emulator 上不足 4×** — 三種喚醒機制實測(bg sleep(1)、
  main-thread 自我 repost、bg nanoTime busy-wait 2ms):理論成立(讓主 Looper 不睡→已縮放的
  step 提早 dispatch),但 x86_64 emulator CPU 受限,勾選後 4× 移動只增 ~1.2×(幀間運動總和
  233→278),**非 4×**。錄影本身只有 24–33fps → emulator 主執行緒跑不動額外的 step+draw。
  機制合理,真機(60fps、CPU 較足)可能較接近 4×,但**未在真機驗證**。無 crash。
- [resolved] **refresh-rate lever 在 emulator 必 no-op** — `dumpsys display` 確認內建螢幕
  `supportedModes` 只有單一 `{id=1, fps=60}`,沒有其他模式可切 → `preferredRefreshRate`
  無效。wiring 正確、設值不崩;真機若有 30/90/120 模式才會生效(降 Hz 讓移動變慢)。
- [resolved] **真正可靠解:遊戲改 frame-rate independent(已採用)** — `ArenaView.step()` 改成
  `x += vx * dt`(dt = 縮放的 `SystemClock.uptimeMillis()` 差、以 FRAME_MS 為基準、clamp 250ms)。
  每個 vsync 幀位移隨倍率縮放 → **移動實測跟著倍率變**:4×→2.19×、0.25×→0.34×(對比改前 4×=1.0×
  完全不動)。零 CPU 代價、不受 vsync/CPU 限制、emulator 上就看得到。4× 量到 2.19 而非 4.0 是
  撞牆重繪畫素的次線性量測假影,模型位移確為 4×。UI blurb 已改為「movement…runs faster or slower」。
- [open] **與 factor 的關係** — pacing 在 factor=1 無視覺變化(step 仍 60/s);只在 factor≠1
  才顯現。UI 說明點出「配合倍率使用」。

---

## Log

- **2026-09-10** S0 建追蹤檔。承接 `TODO-speedup.md` 的 vsync 結論:時鐘 hook 對排程玩法
  (攻擊)實測 4.06×,但移動被 vsync 鎖 → 本檔加兩個實驗 lever 嘗試解鎖移動。下一步 S1。
- **2026-09-10** S1–S3 完成。`OverlayController` 加 `setFramePacing/isFramePacing`
  (daemon pacer:`mainHandler.post(noop)`+sleep(1ms)、`detachWindows` 停)、
  `setPreferredRefreshRate/preferredRefreshRate`(`updateViewLayout` + `showPanel` 套回)。
  `OverlayPanel.buildSpeedTab` 加 CheckBox(frame pacing)+ Refresh ChoiceStrip(Auto/30/60/90/120)
  + 兩行說明,狀態由 controller 還原。`:overlay:compileDebugJavaWithJavac` **BUILD SUCCESSFUL**。
  下一步 S4 實機驗證。

- **2026-09-10** S4 實機(emulator)驗證,誠實結果:兩個 lever 都做出來、編過、UI 正常、無 crash,
  但**都無法在此 emulator 乾淨解鎖移動**。frame-pacing 三機制實測 4× 移動只增 ~1.2×(幀間運動
  233→278;錄影僅 24–33fps,emulator CPU 跑不動額外 step+draw)——機制合理、真機可能較好但未驗。
  refresh-rate:`dumpsys display` 證實螢幕單一 60Hz 模式 → 必 no-op。**結論**:vsync 鎖定移動在
  這台 emulator 上無法用行程內開關乾淨解鎖;可靠解是遊戲改 `x += vx*dt`(frame-rate independent),
  已列 Issue 待使用者定奪。
- **2026-09-10** 採用可靠解:把 dungeon-tap `ArenaView.step()` 改成 frame-rate independent
  (`x += vx * dt`,dt 取自縮放 uptimeMillis)。實測移動 **4×→2.19×、0.25×→0.34×**(改前 4×=1.0×
  不動)→ 怪物肉眼跟著倍率變快/變慢,零 CPU 代價、emulator 就看得到。實驗開關保留(改前的
  fixed-per-frame 遊戲仍可用),blurb 已更新說明本 demo 已 frame-rate independent、不需開關。
  `:game`+`:overlay` 編過、pack 裝上 emulator。**vsync 移動解鎖任務完成。**
