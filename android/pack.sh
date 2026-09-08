#!/usr/bin/env bash
#
# pack.sh — 產出可側載的開發版 APK。
#
#   ./pack.sh                建置 + 驗證 + 打包到 dist/
#   ./pack.sh --test         先跑 cargo test
#   ./pack.sh --clean        先 gradle clean
#   ./pack.sh --install      打包完直接裝到連著的裝置（會先解除安裝）
#   ./pack.sh --release-check  另外驗證 release APK 不含引擎（慢，會跑 assembleRelease）
#   ./pack.sh --verbose      印出完整的 gradle task 輸出
#
# 工具鏈位置可用環境變數覆寫：JAVA_HOME、GRADLE、ANDROID_HOME、ADB。
#
# 這支腳本的驗證不是裝飾。有兩件事一旦錯掉，整個設計會「安靜地」失效
# ——App 全部照常啟動，只是掃不到任何東西：
#
#   1. 遊戲 APK 混進了 libce_engine.so。兩個 package 有兩個 ClassLoader，
#      第二次 System.loadLibrary 會丟 "already opened by ClassLoader"。
#   2. 兩支 APK 的簽章不同。sharedUserId 需要相同憑證，否則安裝就會被拒，
#      或更糟——先前裝過的舊版讓 uid 已經定型。
#
# 所以這兩項檢查失敗時腳本直接退出，不會產出 dist/。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST="$SCRIPT_DIR/dist"

VERSION="0.3.1"
CE_PKG="dev.marc.ce.app"
GAME_PKG="dev.marc.ce.game"

# 產出檔名。rename.sh 會跟著顯示名稱一起改這三行。
CE_APK_BASE="cheat-engine"
GAME_APK_BASE="dungeon-tap"
SAMPLE_APK_BASE="ce-sample-embedded"

DO_TEST=0
DO_CLEAN=0
DO_INSTALL=0
DO_RELEASE_CHECK=0
VERBOSE=0

# ---------------------------------------------------------------- 參數

while [ $# -gt 0 ]; do
  case "$1" in
    --test)          DO_TEST=1 ;;
    --clean)         DO_CLEAN=1 ;;
    --install)       DO_INSTALL=1 ;;
    --release-check) DO_RELEASE_CHECK=1 ;;
    -v|--verbose)    VERBOSE=1 ;;
    -h|--help)
      sed -n '2,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *)
      echo "未知參數：$1（--help 看用法）" >&2
      exit 2 ;;
  esac
  shift
done

# ---------------------------------------------------------------- 輸出

BOLD=''; DIM=''; RED=''; GREEN=''; YELLOW=''; RESET=''
if [ -t 1 ]; then
  BOLD=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'
  GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RESET=$'\033[0m'
fi

step() { printf '\n%s==>%s %s%s%s\n' "$GREEN" "$RESET" "$BOLD" "$1" "$RESET"; }
info() { printf '    %s\n' "$1"; }
note() { printf '    %s%s%s\n' "$DIM" "$1" "$RESET"; }
warn() { printf '    %s! %s%s\n' "$YELLOW" "$1" "$RESET"; }
ok()   { printf '    %s✓%s %s\n' "$GREEN" "$RESET" "$1"; }
die()  { printf '\n%s✗ %s%s\n\n' "$RED" "$1" "$RESET" >&2; exit 1; }

# ---------------------------------------------------------------- 工具鏈

# Windows 的 Java 工具吃不下 MSYS 的 /c/... 路徑，需要轉回 C:/...
winpath() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

find_gradle() {
  if [ -n "${GRADLE:-}" ]; then printf '%s' "$GRADLE"; return; fi
  if [ -x "$SCRIPT_DIR/gradlew" ]; then printf '%s' "$SCRIPT_DIR/gradlew"; return; fi
  if [ -f "$SCRIPT_DIR/gradlew.bat" ]; then printf '%s' "$SCRIPT_DIR/gradlew.bat"; return; fi
  if command -v gradle >/dev/null 2>&1; then printf '%s' "$(command -v gradle)"; return; fi
  # 這台機器上的本機發行版；沒有 wrapper 就靠它
  local g
  for g in /c/devtools/gradle/*/bin/gradle.bat; do
    [ -f "$g" ] && printf '%s' "$g" && return
  done
  return 1
}

find_sdk() {
  if [ -n "${ANDROID_HOME:-}" ]; then printf '%s' "$ANDROID_HOME"; return; fi
  if [ -n "${ANDROID_SDK_ROOT:-}" ]; then printf '%s' "$ANDROID_SDK_ROOT"; return; fi
  # local.properties 是這個專案唯一保證存在的來源
  if [ -f "$SCRIPT_DIR/local.properties" ]; then
    local d
    d="$(sed -n 's/^sdk\.dir=//p' "$SCRIPT_DIR/local.properties" | tr -d '\r' | tail -1)"
    if [ -n "$d" ]; then printf '%s' "$d"; return; fi
  fi
  return 1
}

# build-tools 版本目錄名是字串排序不可靠，取 mtime 最新的那個
find_build_tool() {
  local name="$1" sdk="$2" p
  for p in "$sdk"/build-tools/*/"$name" "$sdk"/build-tools/*/"$name.bat" "$sdk"/build-tools/*/"$name.exe"; do
    [ -f "$p" ] && printf '%s' "$p" && return
  done
  return 1
}

step "工具鏈"

GRADLE_BIN="$(find_gradle)" || die "找不到 gradle。設定 GRADLE=<路徑> 或安裝 gradle 到 PATH。"
info "gradle   $GRADLE_BIN"

SDK="$(find_sdk)" || die "找不到 Android SDK。設定 ANDROID_HOME 或在 android/local.properties 寫 sdk.dir。"
info "sdk      $SDK"

if [ -z "${JAVA_HOME:-}" ]; then
  for j in /c/devtools/jdk-17 /c/devtools/jdk17 "/c/Program Files/Java/jdk-17"; do
    [ -d "$j" ] && export JAVA_HOME="$j" && break
  done
fi
[ -n "${JAVA_HOME:-}" ] || die "JAVA_HOME 未設定，且找不到 JDK 17。"
info "java     $JAVA_HOME"
export JAVA_HOME

ADB_BIN="${ADB:-$SDK/platform-tools/adb.exe}"
[ -f "$ADB_BIN" ] || ADB_BIN="${ADB:-$SDK/platform-tools/adb}"

AAPT_BIN="$(find_build_tool aapt "$SDK")"      || AAPT_BIN=""
APKSIGNER_BIN="$(find_build_tool apksigner "$SDK")" || APKSIGNER_BIN=""
[ -n "$AAPT_BIN" ]      && info "aapt     $AAPT_BIN"
[ -n "$APKSIGNER_BIN" ] && info "apksigner $APKSIGNER_BIN"

# 預設安靜：33 行 UP-TO-DATE 沒有人要看。-q 仍會完整印出失敗原因。
gradle_run() {
  local quiet=(-q)
  [ "$VERBOSE" -eq 1 ] && quiet=()
  ( cd "$SCRIPT_DIR" && "$GRADLE_BIN" --console=plain "${quiet[@]}" "$@" )
}

# ---------------------------------------------------------------- Rust 測試

if [ "$DO_TEST" -eq 1 ]; then
  step "cargo test（桌面端，確認引擎沒有回歸）"
  ( cd "$REPO_ROOT" && cargo test )
  ok "引擎測試通過"
fi

# ---------------------------------------------------------------- 建置

if [ "$DO_CLEAN" -eq 1 ]; then
  step "gradle clean"
  gradle_run clean
fi

step "建置開發版 APK"
note "cargoNdk 會先交叉編譯 arm64-v8a / armeabi-v7a / x86_64 三個 ABI"
gradle_run :cheatengine:assembleDebug :game:assembleDebug :sample:assembleDebug

CE_APK="$SCRIPT_DIR/cheatengine/build/outputs/apk/debug/cheatengine-debug.apk"
GAME_APK="$SCRIPT_DIR/game/build/outputs/apk/debug/game-debug.apk"
SAMPLE_APK="$SCRIPT_DIR/sample/build/outputs/apk/debug/sample-debug.apk"

for f in "$CE_APK" "$GAME_APK" "$SAMPLE_APK"; do
  [ -f "$f" ] || die "建置完成但找不到 $f"
done
ok "三支 APK 都產出了"

# ---------------------------------------------------------------- 驗證

step "驗證"

# --- 1. 遊戲不可以含引擎 ---------------------------------------------------
gradle_run :game:verifyGameHasNoEngine
ok "遊戲 APK 不含 libce_engine.so"

if [ "$DO_RELEASE_CHECK" -eq 1 ]; then
  gradle_run :sample:verifyOverlayNotInRelease
  ok "sample 的 release APK 不含引擎"
fi

# --- 2. ABI 覆蓋 -----------------------------------------------------------
if [ -n "$AAPT_BIN" ]; then
  abis="$("$AAPT_BIN" list "$(winpath "$CE_APK")" | grep '\.so$' | cut -d/ -f2 | sort -u | tr '\n' ' ')"
  info "CE 的 ABI：$abis"
  case "$abis" in
    *arm64-v8a*) ok "包含 arm64-v8a（實體手機幾乎都是這個）" ;;
    *) die "CE APK 沒有 arm64-v8a，裝到手機上會靜默停用 overlay。" ;;
  esac
  [[ "$abis" == *armeabi-v7a* ]] || warn "沒有 armeabi-v7a，32 位元舊機會拿不到原生庫"
else
  warn "找不到 aapt，略過 ABI 檢查"
fi

# --- 3. 簽章必須相同 -------------------------------------------------------
# sharedUserId 的硬性條件。不同憑證會得到 INSTALL_FAILED_SHARED_USER_INCOMPATIBLE，
# 而錯誤訊息完全指不到重點。
if [ -n "$APKSIGNER_BIN" ]; then
  cert_of() {
    "$APKSIGNER_BIN" verify --print-certs "$(winpath "$1")" 2>/dev/null \
      | grep -m1 -i 'certificate SHA-256 digest' | awk '{print $NF}'
  }
  ce_cert="$(cert_of "$CE_APK")"
  game_cert="$(cert_of "$GAME_APK")"
  [ -n "$ce_cert" ] || die "讀不到 CE 的簽章憑證"
  if [ "$ce_cert" = "$game_cert" ]; then
    ok "兩支 APK 簽章相同 ${ce_cert:0:16}…"
  else
    die "簽章不同，sharedUserId 一定會失敗：
        CE   $ce_cert
        Game $game_cert"
  fi
else
  warn "找不到 apksigner，略過簽章比對——這是共用 uid 的必要條件，請自行確認"
fi

# --- 4. arm64 的 16 KB page 對齊 -------------------------------------------
# Android 15 起 64 位元裝置要求 16 KB page 支援。
READELF=""
for r in "$SDK"/ndk/*/toolchains/llvm/prebuilt/*/bin/llvm-readelf.exe \
         "$SDK"/ndk/*/toolchains/llvm/prebuilt/*/bin/llvm-readelf; do
  [ -f "$r" ] && READELF="$r" && break
done
SO64="$SCRIPT_DIR/overlay/src/main/jniLibs/arm64-v8a/libce_engine.so"
if [ -n "$READELF" ] && [ -f "$SO64" ]; then
  align="$("$READELF" -l "$(winpath "$SO64")" 2>/dev/null | awk '/ LOAD /{print $NF; exit}')"
  if [ "$align" = "0x4000" ]; then
    ok "arm64-v8a 為 16 KB page 對齊（$align）"
  else
    warn "arm64-v8a 的 LOAD 對齊是 $align，不是 0x4000；Android 15 裝置可能載不起來"
  fi
else
  note "略過 page 對齊檢查（找不到 llvm-readelf）"
fi

# ---------------------------------------------------------------- 打包

step "打包到 dist/"

rm -rf "$DIST"
mkdir -p "$DIST"
cp "$CE_APK"     "$DIST/$CE_APK_BASE-v$VERSION.apk"
cp "$GAME_APK"   "$DIST/$GAME_APK_BASE-v$VERSION.apk"
cp "$SAMPLE_APK" "$DIST/$SAMPLE_APK_BASE-v$VERSION.apk"

( cd "$DIST" && sha256sum *.apk | sed 's/\*//' > SHA256SUMS.txt )

for f in "$DIST"/*.apk; do
  size="$(du -h "$f" | cut -f1)"
  printf '    %-34s %s\n' "$(basename "$f")" "$size"
done
ok "SHA256SUMS.txt 已產生"

if [ -f "$SCRIPT_DIR/dist-README.md" ]; then
  cp "$SCRIPT_DIR/dist-README.md" "$DIST/安裝說明.md"
fi

# ---------------------------------------------------------------- 安裝

if [ "$DO_INSTALL" -eq 1 ]; then
  step "安裝到裝置"

  [ -f "$ADB_BIN" ] || die "找不到 adb（設定 ADB=<路徑>）"
  devices="$("$ADB_BIN" devices | awk 'NR>1 && $2=="device" {print $1}')"
  [ -n "$devices" ] || die "沒有連上的裝置。接上手機並開啟 USB 偵錯，或啟動模擬器。"
  info "裝置：$(echo "$devices" | tr '\n' ' ')"

  # package 的 uid 在安裝當下就固定了。舊版若沒有 sharedUserId，覆蓋安裝會得到
  # INSTALL_FAILED_UID_CHANGED；先移除是唯一可靠的作法。
  note "先解除安裝——package 的 uid 在安裝時就定型，不能事後補 sharedUserId"
  "$ADB_BIN" uninstall "$CE_PKG"   >/dev/null 2>&1 || true
  "$ADB_BIN" uninstall "$GAME_PKG" >/dev/null 2>&1 || true

  # 先裝帶 .so 的那支：共用 uid 的 ABI 是一起協調的，讓有原生庫的先決定。
  info "安裝 $CE_PKG"
  "$ADB_BIN" install -r "$(winpath "$DIST/$CE_APK_BASE-v$VERSION.apk")" >/dev/null
  info "安裝 $GAME_PKG"
  "$ADB_BIN" install -r "$(winpath "$DIST/$GAME_APK_BASE-v$VERSION.apk")" >/dev/null

  # 開發用的捷徑，省去手動去設定裡點「顯示在其他應用程式上層」
  "$ADB_BIN" shell appops set "$CE_PKG" SYSTEM_ALERT_WINDOW allow >/dev/null 2>&1 || true

  ce_uid="$("$ADB_BIN" shell dumpsys package "$CE_PKG"   | grep -m1 -o 'dev\.marc\.ce\.shared/[0-9]*' || true)"
  game_uid="$("$ADB_BIN" shell dumpsys package "$GAME_PKG" | grep -m1 -o 'dev\.marc\.ce\.shared/[0-9]*' || true)"
  if [ -n "$ce_uid" ] && [ "$ce_uid" = "$game_uid" ]; then
    ok "共用 uid 成立：$ce_uid"
  else
    warn "兩支的 sharedUser 不一致（CE=$ce_uid Game=$game_uid）——掃描會找不到東西"
  fi

  ok "已安裝。開啟 Cheat Engine → Start engine → Open Dungeon Tap"
  note "兩個畫面印出的 pid 相同，就代表真的在同一個行程裡"
fi

printf '\n%s完成%s  %s\n\n' "$BOLD" "$RESET" "$DIST"
