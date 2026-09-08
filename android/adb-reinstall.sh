#!/usr/bin/env bash
# Repack an APK/APKS and reinstall it through adb.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INPUT=""
OUTPUT=""

die() { printf '\n✗ %s\n' "$*" >&2; exit 1; }
info() { printf '    %s\n' "$*"; }

while [ $# -gt 0 ]; do
  case "$1" in
    -i) INPUT="${2:-}"; shift 2 ;;
    -o) OUTPUT="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '1,24p' "$0"
      exit 0
      ;;
    *) die "未知參數：$1（repack 參數請放在 -- 後面）" ;;
  esac
done

[ -n "$INPUT" ] || die "用法：$0 -i <input.apk 或 input.apks> [-- repack參數]"
[ -f "$INPUT" ] || die "找不到輸入檔：$INPUT"

case "$INPUT" in
  *.apks) DEFAULT_OUTPUT="${INPUT%.apks}-ce.apks" ;;
  *.apk)  DEFAULT_OUTPUT="${INPUT%.apk}-ce.apk" ;;
  *)      die "輸入必須是 .apk 或 .apks" ;;
esac
[ -n "$OUTPUT" ] || OUTPUT="$DEFAULT_OUTPUT"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/adb-reinstall.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

info "執行 repack.sh"
if [ -n "$OUTPUT" ]; then
  bash "$SCRIPT_DIR/repack.sh" -i "$INPUT" -o "$OUTPUT"
else
  bash "$SCRIPT_DIR/repack.sh" -i "$INPUT"
fi

if [[ "$OUTPUT" == *.apks ]]; then
  unzip -q "$OUTPUT" '*.apk' -d "$WORK/apks" || die "解開輸出 .apks 失敗"
  mapfile -t APK_FILES < <(find "$WORK/apks" -type f -name '*.apk' | sort)
  [ "${#APK_FILES[@]}" -gt 0 ] || die ".apks 裡沒有 APK"
  BASE="$(find "$WORK/apks" -type f -name base.apk -print -quit)"
else
  APK_FILES=("$OUTPUT")
  BASE="$OUTPUT"
fi

[ -n "$BASE" ] && [ -f "$BASE" ] || die "找不到 base.apk"

APKANALYZER="${APKANALYZER:-}"
if [ -z "$APKANALYZER" ] && [ -n "${ANDROID_HOME:-}" ]; then
  APKANALYZER="$(find "$ANDROID_HOME/cmdline-tools" "$ANDROID_HOME/tools" -type f -name apkanalyzer.bat 2>/dev/null | sort -V | tail -1 || true)"
fi
if [ -z "$APKANALYZER" ]; then
  for p in /c/devtools/Android/Sdk/cmdline-tools/*/bin/apkanalyzer.bat /c/devtools/Android/Sdk/tools/bin/apkanalyzer.bat; do
    [ -f "$p" ] && APKANALYZER="$p" && break
  done
fi
[ -n "$APKANALYZER" ] || die "找不到 apkanalyzer（可設定 APKANALYZER）"

PACKAGE="$($APKANALYZER manifest application-id "$(cygpath -w "$BASE" 2>/dev/null || printf '%s' "$BASE")" 2>/dev/null | tr -d '\r')"
[ -n "$PACKAGE" ] || die "無法讀取 package name"
info "package  $PACKAGE"

adb wait-for-device
adb uninstall "$PACKAGE" >/dev/null 2>&1 || true

if [ "${#APK_FILES[@]}" -eq 1 ]; then
  adb install -r -d -g "${APK_FILES[0]}"
else
  # 大型 Unity split bundle 用 push-then-install，避免 streaming install
  # 在數百 MB 的 asset pack 上長時間沒有任何輸出。
  adb install-multiple --no-streaming -r -d -g "${APK_FILES[@]}"
fi

info "重新安裝完成：$PACKAGE"
