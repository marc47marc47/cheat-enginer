#!/usr/bin/env bash
# build.sh — release-build 每一個產物:
#   桌面引擎 (exe + cdylib)、注入 payload (Windows .dll / Linux .so)、
#   Android 原生庫 (.so ×3 ABI) + 三支 APK。
#
#   ./build.sh                建置全部(含 Android)
#   ./build.sh --skip-android 只建桌面 + 兩個 payload(快)
#   ./build.sh -h
#
# 這是 `cargo build --release` 的總管:一次把整個專案(跨 Windows / Linux / Android)
# 所有可交付檔案都以 release 模式建出來。桌面用 host 的 cargo;Linux payload .so 在
# 非 Linux host 上用 cargo-zigbuild 交叉連結;Android 交給 android/pack.sh(cargo-ndk +
# gradle)。任何一項需要的產物建置失敗 → 整個腳本以非 0 退出。
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DO_ANDROID=1
for a in "$@"; do
  case "$a" in
    --skip-android) DO_ANDROID=0 ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^#\{0,1\} \{0,1\}//'; exit 0 ;;
    *) echo "未知參數:$a(-h 看用法)" >&2; exit 2 ;;
  esac
done

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) HOST=windows ;;
  Linux)                HOST=linux ;;
  Darwin)               HOST=macos ;;
  *)                    HOST=unknown ;;
esac

if [ -t 1 ]; then B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; R=$'\033[31m'; Z=$'\033[0m';
else B=; G=; Y=; R=; Z=; fi
step() { printf '\n%s==> %s%s\n' "$B" "$*" "$Z"; }
ok()   { printf '    %s✓%s %s\n' "$G" "$Z" "$*"; }
warn() { printf '    %s!%s %s\n' "$Y" "$Z" "$*"; }
bad()  { printf '    %s✗%s %s\n' "$R" "$Z" "$*"; }
note() { printf '      %s\n' "$*"; }

FAILED=0

# ---------------------------------------------------------------------------
# 1) 桌面引擎:bin(TUI exe)+ lib(cdylib)+ examples
# ---------------------------------------------------------------------------
step "桌面引擎  cargo build --release(lib + bin + examples)"
if cargo build --release --lib --bins --examples; then
  ok "桌面引擎"
else
  bad "桌面引擎建置失敗"; FAILED=1
fi

# ---------------------------------------------------------------------------
# 2) Windows payload  ce_speedhook.dll(cfg(windows),只在 Windows host 有意義)
# ---------------------------------------------------------------------------
step "Windows payload  ce_speedhook.dll"
if [ "$HOST" = windows ]; then
  if ( cd speedhook-payload && cargo build --release ); then
    ok "ce_speedhook.dll"
  else
    bad "Windows payload 建置失敗"; FAILED=1
  fi
else
  warn "非 Windows host → 略過(payload 是 cfg(windows))"
fi

# ---------------------------------------------------------------------------
# 3) Linux payload  libce_speedhook_linux.so
#    Linux host 原生建;其他 host 有 cargo-zigbuild 就交叉連結,否則只 cargo check。
# ---------------------------------------------------------------------------
LINUX_TGT=x86_64-unknown-linux-gnu
step "Linux payload  libce_speedhook_linux.so($LINUX_TGT)"
if [ "$HOST" = linux ]; then
  if ( cd speedhook-payload-linux && cargo build --release ); then
    ok "libce_speedhook_linux.so"
  else
    bad "Linux payload 建置失敗"; FAILED=1
  fi
elif command -v cargo-zigbuild >/dev/null 2>&1; then
  note "用 cargo-zigbuild 交叉連結(host=$HOST)"
  if ( cd speedhook-payload-linux && cargo zigbuild --release --target "$LINUX_TGT" ); then
    ok "libce_speedhook_linux.so"
  else
    bad "Linux payload 交叉建置失敗"; FAILED=1
  fi
else
  warn "無 cargo-zigbuild 且非 Linux host → 只做 cargo check(不產出 .so)"
  if ( cd speedhook-payload-linux && cargo check --release --target "$LINUX_TGT" ); then
    note "check 過(需 Linux host 或安裝 cargo-zigbuild 才能真的連出 .so)"
  else
    bad "Linux payload cargo check 失敗"; FAILED=1
  fi
fi

# ---------------------------------------------------------------------------
# 4) Android:原生庫(.so ×3 ABI)+ 三支 APK,交給 pack.sh(cargo-ndk + gradle)
# ---------------------------------------------------------------------------
if [ "$DO_ANDROID" = 1 ]; then
  step "Android  原生庫(.so ×3 ABI)+ APK(android/pack.sh)"
  if [ -x android/pack.sh ]; then
    if ( cd android && ./pack.sh ); then
      ok "Android(dist/*.apk + jniLibs/*/libce_engine.so)"
    else
      bad "Android 建置失敗(見上方 pack.sh 輸出)"; FAILED=1
    fi
  else
    bad "找不到 android/pack.sh"; FAILED=1
  fi
else
  step "Android  --skip-android,略過"
fi

# ---------------------------------------------------------------------------
# 產物清單
# ---------------------------------------------------------------------------
if [ "$HOST" = windows ]; then EXE=.exe; DYLIB=ce_engine.dll; else EXE=; DYLIB=libce_engine.so; fi
step "產物(release)"
listone() {
  if [ -f "$1" ]; then printf '    %8s  %s\n' "$(du -h "$1" 2>/dev/null | cut -f1)" "$1"
  else                 printf '    %8s  %s\n' "—" "$1 (未產出)"; fi
}
listglob() {
  local found=0 f
  for f in $1; do [ -f "$f" ] && { listone "$f"; found=1; }; done
  [ "$found" = 0 ] && printf '    %8s  %s\n' "—" "$1 (未產出)"
}
echo "桌面:"
listone "target/release/cheat-enginer$EXE"
listone "target/release/$DYLIB"
listone "target/release/examples/tick_target$EXE"
listone "target/release/examples/speed_inject$EXE"
echo "Payload:"
listone "speedhook-payload/target/release/ce_speedhook.dll"
listone "speedhook-payload-linux/target/$LINUX_TGT/release/libce_speedhook_linux.so"
if [ "$DO_ANDROID" = 1 ]; then
  echo "Android:"
  listglob "android/dist/*.apk"
  listglob "android/overlay/src/main/jniLibs/*/libce_engine.so"
fi

echo
if [ "$FAILED" = 0 ]; then
  printf '%s==> 全部完成%s\n' "$G" "$Z"
else
  printf '%s==> 有項目失敗(見上方 ✗)%s\n' "$R" "$Z"
fi
exit "$FAILED"
