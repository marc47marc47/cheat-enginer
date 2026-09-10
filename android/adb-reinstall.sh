#!/usr/bin/env bash
# Repack an APK/APKS and reinstall it through adb.
#
# 用法:
#   ./adb-reinstall.sh -i <input.apk 或 .apks> [-o out] [-s <serial>]
#
# 裝置選擇(-s / 自動 / 提示,在跑 repack 之前就先決定好):
#   -s <serial>   指定 adb 目標裝置(等同 adb -s),例:-s emulator-5554
#   不給 -s 時:只有一台就自動選;多台且在互動終端 → 一開始就列出讓你挑;
#              多台但非互動(被 pipe)→ 要求用 -s 指定;沒有裝置 → 直接報錯。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INPUT=""
OUTPUT=""
DEVICE=""
SERIAL=""

die() { printf '\n✗ %s\n' "$*" >&2; exit 1; }
info() { printf '    %s\n' "$*"; }

while [ $# -gt 0 ]; do
  case "$1" in
    -i) INPUT="${2:-}"; shift 2 ;;
    -o) OUTPUT="${2:-}"; shift 2 ;;
    -s|--device) DEVICE="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '2,10p' "$0" | sed 's/^#\{0,1\} \{0,1\}//'
      exit 0
      ;;
    *) die "未知參數：$1" ;;
  esac
done

# 決定要安裝到哪一台裝置(存進 SERIAL,之後所有 adb 都帶 -s "$SERIAL")。
resolve_device() {
  local serials=() models=() line s st model
  while IFS= read -r line; do
    line="${line%$'\r'}"
    case "$line" in ''|'List of devices attached'*) continue ;; esac
    s="${line%%[[:space:]]*}"
    st="$(printf '%s\n' "$line" | awk '{print $2}')"
    [ "$st" = "device" ] || continue          # 跳過 offline / unauthorized
    model="$(printf '%s\n' "$line" | grep -oE 'model:[^ ]+' | cut -d: -f2 || true)"
    serials+=("$s"); models+=("${model:-?}")
  done < <(adb devices -l 2>/dev/null)

  # -s 明指:驗證它真的在線上且就緒
  if [ -n "$DEVICE" ]; then
    local i
    for i in "${!serials[@]}"; do
      [ "${serials[$i]}" = "$DEVICE" ] && { SERIAL="$DEVICE"; info "裝置  $SERIAL(${models[$i]},由 -s 指定)"; return; }
    done
    die "指定的裝置不在線上/未就緒:$DEVICE（ready:${serials[*]:-無}）"
  fi

  case "${#serials[@]}" in
    0) die "沒有連線的 Android 裝置。接上實機或啟動模擬器後再試(adb devices 應看得到 state=device)。" ;;
    1) SERIAL="${serials[0]}"; info "裝置  $SERIAL(${models[0]},唯一一台,自動選擇)" ;;
    *)
      if [ ! -t 0 ]; then
        die "偵測到多台裝置,非互動模式無法提示,請用 -s <serial> 指定:${serials[*]}"
      fi
      printf '偵測到多台裝置,請選擇要安裝的目標:\n' >&2
      local i
      for i in "${!serials[@]}"; do
        printf '  %d) %-20s %s\n' "$((i + 1))" "${serials[$i]}" "${models[$i]}" >&2
      done
      local choice
      while :; do
        printf '輸入編號 [1-%d]（Enter 取消）:' "${#serials[@]}" >&2
        read -r choice || die "已取消"
        [ -n "$choice" ] || die "已取消"
        case "$choice" in *[!0-9]*) printf '  請輸入數字\n' >&2; continue ;; esac
        if [ "$choice" -ge 1 ] && [ "$choice" -le "${#serials[@]}" ]; then
          SERIAL="${serials[$((choice - 1))]}"
          info "裝置  $SERIAL(${models[$((choice - 1))]})"
          break
        fi
        printf '  超出範圍\n' >&2
      done
      ;;
  esac
}

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

resolve_device   # 先選好裝置(沒裝置/選錯就在這裡直接失敗,不用等 repack 才發現)

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

adb -s "$SERIAL" wait-for-device
adb -s "$SERIAL" uninstall "$PACKAGE" >/dev/null 2>&1 || true

if [ "${#APK_FILES[@]}" -eq 1 ]; then
  adb -s "$SERIAL" install -r -d -g "${APK_FILES[0]}"
else
  # 大型 Unity split bundle 用 push-then-install，避免 streaming install
  # 在數百 MB 的 asset pack 上長時間沒有任何輸出。
  adb -s "$SERIAL" install-multiple --no-streaming -r -d -g "${APK_FILES[@]}"
fi

info "重新安裝完成：$PACKAGE → $SERIAL"
