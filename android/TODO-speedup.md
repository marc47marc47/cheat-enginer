# TODO — Speedhack(跨平台變速器)

透過 cheat-engine 讓目標遊戲時間可加速／減速。機制:hook 目標讀取的時鐘函式,
縮放其流逝速率。**桌面 + Android 都要支援**。計畫全文見
`~/.claude/plans/melodic-bouncing-swing.md`。

- 縮放數學:`virt = virt0 + (real_now - real0) * factor`;改 factor 時重新錨定 →
  連續、單調不減。factor>1 加速、<1 減速、=1 等於沒 hook。
- 關鍵架構差異:**Android 行程內**(patch 自己 GOT,易)/**桌面跨行程**
  (要注入程式碼進目標,難)。

---

## Stages

### Phase A — 共用核心 ＋ Android

- [x] **A0 追蹤檔 ＋ 骨架** — 本檔建立;`src/lib.rs` 加 `pub mod speedhack;`;
      `src/speedhack/{mod,clock}.rs` 骨架編過。
      _驗:`cargo build` 通過。✓_
- [x] **A1 縮放虛擬時鐘(純邏輯)** — `src/speedhack/clock.rs`:`SpeedClock`
      seqlock(讀無鎖、寫少)+ 分段線性縮放。
      _驗:`cargo test speedhack` 6/6 通過(恆等、2× delta 加倍、0.5× 減半、
      改 factor 不跳變、跨變更單調、併發不撕裂)。全套 69 passed 無回歸。✓_
- [x] **A2 Android GOT hook** — `src/speedhack/android.rs`(`cfg(android)`):
      `dl_iterate_phdr` 列舉模組 → 解析 `.rela.plt`/`.rela.dyn` 找 `clock_gettime`
      → `mprotect` + 覆寫 GOT slot、存原始指標。stub 只縮放 `CLOCK_MONOTONIC`
      (Handler/uptime 用的),其餘放行。純資料指標改寫,不需產生機器碼、不需
      cache flush。**排除自己的 libce_engine.so / vdso / linker**。
      _驗:arm64-v8a / armeabi-v7a / x86_64 三 ABI 全編過;v7a(Elf32)install 回 0。✓_
- [x] **A3 JNI 接線** — `src/android/mod.rs` 加 `nativeSpeedInstall/Set/Factor`,
      全包 `guard`、factor clamp 0.1–8,行程全域 static。
      _驗:`cargo ndk` 編過;`llvm-readelf --dyn-syms` 三個 `Java_..._nativeSpeed*`
      符號都在。✓_
- [x] **A4 Overlay UI** — `NativeBridge.java` 宣告 3 個 native;`ChoiceStrip` 加
      `OnSelect` callback;`OverlayPanel` 第 5 個分頁 Speed，chips
      {0.25x,0.5x,1x,2x,4x},選時先 lazy install 再 set、狀態顯示「now N.NNx」,
      不支援的 ABI 灰掉。零 manifest/權限改動。
      _驗:`:overlay:compileDebugJavaWithJavac` 過;實機分頁與 chips 正常渲染。✓_
- [x] **A5 Android 實機驗證** — `./pack.sh --install` → dungeon-tap(standalone
      共用行程)→ Speed 分頁確認「now 4.00x」綠字。以固定牆鐘窗量玩家 HP 下降
      (= 怪物攻擊 cadence = 時間縮放):
      　0.25× → 4s 內 100→100(沒挨打,慢動作)
      　1× → 2s 內 100→92（−8，≈4 HP/s）
      　4× → 2s 內 93→66（−27，≈13.5 HP/s）
      4×/1× ≈ **3.4×**(含 ±1 傷害隨機,約等於 4×);0.25× 幾乎凍結。**無 crash/ANR**。
      **vDSO 風險沒發生** —— hook 透過 `clock_gettime` caller GOT 生效。
      引擎自身在 4× 下掃描/面板全程正常(自我排除有效)。✓
      _量測教訓:overlay 面板每次重開都回 Scan 分頁,自動化必須每次先點 Speed 分頁
      再點 chip —— 第一版腳本漏點,誤以為 4× 沒效果,實為根本沒設成。_

### Phase B — 桌面跨行程

- [x] **B0 桌面 demo 目標** — `examples/tick_target.rs`:QPC(Win)驅動 game clock、
      牆鐘 `sleep` 驅動刷新速率、自報 pid。當桌面版的「dungeon-tap」。
      _驗:`cargo run --example tick_target` 1× 時 game clock ≈ 牆鐘。✓_
- [x] **B1 Windows 跨行程 speedhack** — 改用 **DLL 注入**(比手寫機器碼 stub 乾淨,
      零機器碼):`speedhook-payload/` 編成 `ce_speedhook.dll`,`src/speedhack/windows.rs`
      以 `VirtualAllocEx`+`WriteProcessMemory`+`CreateRemoteThread(LoadLibraryW)` 注入。
      payload 在目標內走 Toolhelp 列舉模組、解析 PE IAT、把 `QueryPerformanceCounter`
      slot 用 `VirtualProtect` 改指向 `hook_qpc`(呼叫真 QPC 再縮放)。factor 經
      named file-mapping(`SpeedClock` `#[repr(C)]`)由 scanner 寫、payload 讀。可逆
      (factor=1.0 即等於沒 hook)。
      _驗:`cargo build --example speed_inject`+payload 編過;端到端注入見 B4。✓_
- [ ] **B2 Linux 跨行程 speedhack** — 實作完成、`cargo check` 過 gnu+musl,**未在 Linux
      實機驗證**(此開發機是 Windows,只能 check 不能跑)。做法與 Windows 對稱、改用
      **注入 `.so`**(非手寫 trampoline):`src/speedhack/linux.rs` = scanner 端,POSIX shm
      (`shm_open`+`mmap`,`/ce_speedhook_clock`)共用 `SpeedClock` + `ptrace` 遠端 `dlopen`
      注入器;`speedhook-payload-linux/` = 注入的 `.so`,`.init_array` 建構子在 dlopen 當下
      跑,map 共用 clock 後用 android 那套 device-proven 的 ELF GOT-patch 攔自己行程的
      `clock_gettime`(排除自己/vdso/linker)。已知簡化假設(需 Linux 上驗證):x86_64 only;
      注入器假設 target 與 scanner 同一份 libc 檔(重用 dlopen offset);用「回傳到位址 0 觸發
      SIGSEGV」偵測遠端呼叫完成。**下一步**:在 Linux 上 `cargo build --release` payload +
      對一支 `clock_gettime`-driven 的 demo(可仿 tick_target 的 Linux 分支)實測注入與 4×/
      0.25×,並修 dlopen 解析/暫存器 ABI 的實機問題。
      _驗:`cargo check --target x86_64-unknown-linux-gnu`/`-musl` 皆過;payload crate 亦過。_
- [x] **B3 TUI Speed 控制** — `src/ui/app.rs` 加 **Speed 分頁(F4)**,對目前 attach 的
      目標操作:presets `1-5`(0.25/0.5/1/2/4x)、`+/-` 微調 0.05、`r` 回 1x、`Space/p`
      **真凍結暫停(factor 0)**——桌面跨行程,凍住目標時鐘不會影響 TUI 自己(不像
      Android 會凍住 UI)。首次操作時自動注入 payload(`speed_payload_path()`:先找 exe
      旁、再找 dev tree),之後 `set_factor`。`speedhack::clamp` 重新允許 0=freeze(桌面用;
      Android Java 端永遠不送 0)。
      _驗:`cargo test` **72 passed**(新增 speed_snaps_and_clamps / pause_toggles /
      speed_screen_renders 三測);release 編過;注入/set_factor 機制本身見 B4。TUI 互動鍵
      入無法在 Windows 自動化(crossterm 讀 console API 非 stdin),故以單元測 + B4 機制佐證。✓_
- [x] **B4 桌面驗證** — `tick_target` → `speed_inject <pid> <factor>` 注入,固定牆鐘窗
      量 game clock 推進速率:**0.25×→實測 0.25×**、1×→1.09×、2×→2.14×、4×→4.30×
      (高倍率小幅超出是 log 每 50ms 才寫一行的取樣偏差,非變速誤差)。切換連續無
      跳變、還原乾淨、process 全程存活。Linux(B2)待做。✓

### Phase C — 收尾

- [ ] **C0 文件** — `android/README.md` + 桌面 README 補 Speed 功能與限制。

---

## Issues / 待決

- [open] **自我 hook(Android)** — 必排除 `libce_engine.so`,否則引擎自身凍結
  (100ms)/掃描執行緒計時被縮放。A2 用模組 base 比對跳過自己。**A5 要實測**
  「凍結仍每 100ms」來確認排除成功。
- [resolved] **vDSO 繞過(Android)** — 疑慮沒發生。A5 實機證明 hook 透過各模組對
  `clock_gettime` 的 caller GOT 生效(libutils `systemTime` 等 import 的是 libc 符號,
  patch 在 caller 端就攔住,不必碰 vDSO)。dungeon-tap 4× 實測 ~3.4× 加速。
  若日後某目標的計時路徑真的繞過 → 再補 hook `gettimeofday`。
- [open] **行程全域副作用** — 縮放單調時鐘也影響 ART GC/輸入逾時/ANR(Android)、
  目標自身邏輯(桌面)。預設 1.0、clamp 0.1–8;極端值可能不穩,是 speedhack 本質。
- [resolved] **跨行程注入的碼(桌面)** — 原怕要手寫每架構 stub 機器碼。實作改用
  **DLL 注入**規避:`hook_qpc` 是編譯好的 Rust,payload 在目標內自己 patch IAT,零
  機器碼、無架構相依。anti-cheat 遊戲仍會擋(本專案只針對自製 demo 目標)。
- [resolved] **桌面 hook 自我遞迴 → stack overflow** — payload 一開始 patch **每個**
  模組的 IAT,含 `kernel32`/`kernelbase`;而 `kernel32!QueryPerformanceCounter` 內部
  又經自己 IAT 呼叫 `kernelbase!QPC` → 也被導向 `hook_qpc` → `hook_qpc→真QPC→又進
  hook_qpc` 無限遞迴,`0xc00000fd` 爆堆疊、目標秒閃退。修:`skip_module` 跳過
  `ce_speedhook.dll`/`kernel32`/`kernelbase`/`ntdll`/`api-ms-win-*`/`ext-ms-*`,只 hook
  目標 app 自己的模組,系統實作鏈保持原樣。
- [resolved] **named section 名稱生命週期** — payload `MapViewOfFile` 後就
  `CloseHandle(mapping)`;view 能保住 section 頁面但**保不住名稱**,scanner#1 退出後
  名稱釋放 → scanner#2 同名 `CreateFileMappingW` 建的是**全新** section(還被 reinit
  成 1.0),payload 卻仍讀 scanner#1 的孤兒 section → factor 卡在第一次設的值不再變。
  修:payload **保留 mapping handle**(target 生命週期內不關),名稱持續註冊,後續每
  次 scanner 都共用同一 section。
- [resolved] **Choreographer/vsync 遊戲 —— emulator「沒效果」的真因** — 在 x86_64
  emulator(Android 15)實測「加速沒效果」,深入診斷(logcat 計數 + strace 主執行緒)
  得到完整真相:
  　1. hook 安裝正常(patched 60 slots 含 libart/libutils)、`clock_gettime` 每秒被呼叫
  　　 ~4100 次、UI 確認 `now 4.00x`。**縮放本身完全正常。**
  　2. 但 strace 顯示遊戲主執行緒 `epoll_pwait(timeout=-1)` 阻塞,靠 **vsync fd
  　　 (Choreographer DisplayEventReceiver,SurfaceFlinger 經 socket 送)每 ~16ms
  　　 (60Hz)喚醒**。沒有 timerfd、epoll timeout 全 ≤0。
  　3. 因此 `FRAME_MS=16` + 固定位移(`x += vx`)的**移動動畫被 vsync 鎖 60fps,任何
  　　 時鐘/計時器縮放都碰不到**(vsync 事件來自另一行程,GOT hook 攔不到)。
  　4. 但 `ATTACK_MS=1500` 這類**長週期 postDelayed**(攻擊/傷害/冷卻)的 due-check 用
  　　 scaled uptimeMillis、在 vsync 頻率下被提早觸發 → **乾淨縮放 4×**。
  **實測驗證**:HP 掉血 1×=2.83 HP/s、4×=11.5 HP/s → **4.06×**。變速器對「排程遊戲邏輯」
  確實有效,只是對「vsync 鎖定的視覺移動」無效。這也解釋 A5(量 HP=攻擊,有效)與
  emulator 初測(量移動,無效)的矛盾 —— 兩者其實都對。
  **嘗試過但放棄的修法**:hook `epoll_wait`/`epoll_pwait`/`poll`/`timerfd_settime` 把 timeout
  ÷factor —— 對此遊戲無效(vsync 走 fd-ready 不走 timeout;timerfd_settime 遊戲時 0 次呼叫,
  由 raw syscall arm),且會把**整個行程**所有 poll 等待都縮放(binder/input/GC),風險大
  收益零 → 已移除,只保留 `clock_gettime` hook。要讓移動也可見加速,遊戲需改成 frame-rate
  independent(`x += vx * dt`,dt 取自 uptimeMillis)—— 那是改 demo 而非引擎。UI blurb 已
  更新講明「時間排程邏輯會變速、vsync 動畫維持 60fps、看 HP 掉血確認」。
- [open] **armeabi-v7a(Elf32)** — Android hook 只處理 Elf64 → `install()` 在 v7a
  回 false、UI 灰掉。
- [open] **可逆性** — factor=1.0 即關閉;完整 uninstall(還原 GOT/IAT)Android 列
  nice-to-have、桌面必須(否則目標留殘留)。
- [resolved] **併發讀寫 SpeedClock** — 用 seqlock;A1 的 `concurrent_reads_stay_monotonic`
  測過:單一遞增 real 時鐘下,4 個 reader 併發、factor 被 hammer 5000 次,虛擬時間
  始終單調不減、無撕裂。第一版測用了亂序 real 時間軸誤判失敗,已修正為單一時鐘。

---

## Log

- **2026-09-10** A0 建骨架:`speedhack` 模組 + 這份追蹤檔。`cargo build` 過。
- **2026-09-10** A1 完成 `SpeedClock`(seqlock + 分段線性縮放)。`cargo test speedhack`
  6/6;全套 `cargo test` **69 passed / 0 failed**(原 63 + 新 6),零回歸。
  修掉一個自己寫錯的併發測(亂序 real 時間軸 → 改單一遞增時鐘)。
- **2026-09-10** A2–A4 完成:Android GOT hook(三 ABI 編過)、3 個 JNI 匯出、
  Speed 分頁(ChoiceStrip + OnSelect)。`:overlay` Java 編過。
- **2026-09-10** A5 實機通過:dungeon-tap standalone,4× 量得 ~3.4× 加速、
  0.25× 慢動作、1× 正常,無 crash/ANR,vDSO 疑慮沒發生。**Phase A(共用核心 +
  Android)全數完成。** 下一步 Phase B(桌面跨行程)或先 C0 文件。
  過程修的 bug:`Elf64_Dyn/Rela/Sym` 自定義(libc 只有 alias);function→usize cast
  用 `as *const ()`;`real_now_ns` cfg 分流;量測腳本每次要先點 Speed 分頁。
- **2026-09-10** B0/B1/B4 完成:桌面 Windows 跨行程 speedhack。`tick_target` demo
  目標 + `ce_speedhook.dll`(注入式,payload 內部 patch PE IAT 的 QPC)+
  `src/speedhack/windows.rs` 注入器 + named file-mapping 共用 `SpeedClock`。端到端注入
  `tick_target` 實測:**0.25×→0.25×**、1×→1.09×、2×→2.14×、4×→4.30×,切換連續、還原
  乾淨、無 crash。過程修兩個真 bug:① patch 到系統模組造成 QPC 自我遞迴 → stack
  overflow(`skip_module` 跳過 kernel32/kernelbase/ntdll/api-ms-win-*/自己);② named
  section 名稱在 scanner 退出後被釋放,payload 讀到孤兒 section → factor 卡死(payload
  保留 mapping handle 維持名稱)。剩 B2(Linux)、B3(TUI 接線)、C0(文件)。
- **2026-09-10** Android emulator「加速沒效果」調查結案(x86_64 / Android 15)。診斷:
  hook 正常、`clock_gettime` 4100/s、`now 4.00x`,但 strace 證明主執行緒 epoll_pwait(-1)
  靠 **vsync fd(60Hz)** 喚醒 → 固定位移的移動被 vsync 鎖死不縮放;而 postDelayed 的
  攻擊 cadence **實測 4.06×**(HP 1×=2.83 vs 4×=11.5 HP/s)。變速器一直有效,只是量錯
  proxy(看移動)。試過 epoll/poll/timerfd wait-hook 皆無效(vsync 走 fd、timerfd 由 raw
  syscall arm)且有全行程副作用 → 移除,回到乾淨的 clock-only hook。UI blurb 改為誠實說明。
  重 build 裝上 emulator。詳見 Issues 的 vsync 條目。
- **2026-09-10** B2 實作(未實機驗證)。與 Windows 對稱的 Linux 跨行程 speedhack:
  `mod.rs` 加 linux 分派(android 續用行程內 CLOCK、桌面 Linux 走 shm);`linux.rs` scanner
  (POSIX shm 共用 SpeedClock + ptrace 遠端 dlopen 注入器);`speedhook-payload-linux/` 注入
  的 .so(.init_array 建構子 + android 那套 ELF GOT-patch,factor 取自 shm)。全部 `cargo
  check` 過 host+gnu+musl,payload crate 亦過;**Windows 開發機無法實機跑,標 UNVERIFIED**。
  剩 B3(TUI 接線)、C0(文件)。
- **2026-09-10** B3 完成:桌面 TUI 加 Speed 分頁(F4)。presets 1-5、+/- 微調 0.05、r 回
  1x、Space/p 真凍結暫停(桌面跨行程,factor 0 不影響 TUI 自己)。首次操作自動注入
  payload。`speedhack::clamp` 重新允許 0=freeze(桌面用;Android 端用 0.15x 因同行程凍結
  會凍住 overlay)。`cargo test` 72 passed(+3 新測)、release 編過。Android 特有的
  frame-pacing/refresh 實驗開關屬 vsync 解法,桌面(QPC 直接縮放、dt 遊戲即生效)不需要。
  **Phase B(桌面)除 B2 Linux 實機驗證外全數完成。** 剩 C0 文件。
