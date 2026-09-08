#!/usr/bin/env bash
#
# repack.sh — 拆解一支 APK、（選擇性地）改它、再重組並重新簽章。
#
#   ./repack.sh -i android/dist/dungeon-tap-v0.3.1.apk
#       → 產出 android/dist/dungeon-tap-v0.3.1-ce.apk（已注入引擎）
#
#   -i 必填。預設就會注入 cheat-enginer 引擎並讓它執行期自動啟動——這正是
#   repack 的用途。只想換簽章、不注入，才加 --no-inject。
#
# 選項：
#   -i FILE          輸入 APK（必填）
#   -o FILE          輸出 APK（預設：<輸入去掉.apk>-repack.apk）
#   --edit-cmd "C"   對拆解後的目錄跑一段指令；環境變數 WORK 指向該目錄
#                    例：--edit-cmd 'sed -i s/Dungeon Tap/Hacked Tap/ $WORK/res/values/strings.xml'
#   （預設注入）把引擎塞進去並讓它「執行期自動啟動」，塞三樣：
#                      · lib/<abi>/libce_engine.so       （原生引擎）
#                      · classesN.dex（驅動它的 overlay class）
#                      · <provider OverlayInstallProvider> （App 一啟動就自我安裝）
#                    ＝ 用二進位注入做出 embedded 模式，不需 sharedUserId、零權限。
#   --no-inject      不注入，只拆解→重組→重簽（換簽章 / 驗證管線用）
#   --engine FILE    引擎來源 APK（預設：android/dist/cheat-engine-*.apk）
#   --inject-overlay 明確要求注入（預設已開，相容保留）
#   --so FILE        用指定的 loose libce_engine.so 覆寫 arm64-v8a（預設：
#                    ../target/aarch64-linux-android/release/libce_engine.so 若存在）
#   --pause          拆解後暫停，讓你手動改 work 目錄，按 Enter 再繼續
#   --keystore FILE  簽章用的 keystore（預設：signing.properties → debug keystore）
#   --ks-pass PASS   keystore 密碼（預設：android，或 signing.properties 裡的值）
#   --ks-alias NAME  金鑰別名（預設：androiddebugkey，或 signing.properties 裡的值）
#   --keep           保留拆解出來的 work 目錄（預設用完刪掉）
#   -h, --help
#
# ---------------------------------------------------------------------------
# 這隻腳本整合的工具（＝前面那張流程圖）
#
#   apktool d   →  拆解：binary manifest→文字、dex→smali、res 還原
#   （你的編輯）→  改 manifest / smali / 塞 lib/<abi>/*.so
#   apktool b   →  重組：smali→dex、res→arsc、打包成未簽章 APK
#   zipalign    →  16 KB 對齊
#   apksigner   →  重新簽章（v2/v3）
#
# 工具位置可用環境變數覆寫：APKTOOL(指向 apktool.jar)、JAVA_HOME、
# ANDROID_HOME、ZIPALIGN、APKSIGNER。
#
# 重點：重簽之後簽章一定跟原版不同，裝之前要先 adb uninstall 舊的。
# 對「你自己有原始碼的 App」，改一行 gradle 重建比這條路乾淨太多——
# 這隻腳本是給「沒有原始碼、只有 APK」的情況用的。
# ---------------------------------------------------------------------------

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INPUT=""
OUTPUT=""
APKS_MODE=0
APKS_STAGE=""
APKS_BASE_ORIGINAL=""
APKS_INPUT_ORIGINAL=""
APKS_HAS_TOC=0
EDIT_CMD=""
INJECT_OVERLAY=1
ENGINE_SRC=""
SO_OVERRIDE=""
PAUSE=0
KEEP=0
KS_FILE=""
KS_PASS=""
KS_ALIAS=""

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
usage() { sed -n '2,42p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

# ---------------------------------------------------------------- 參數

while [ $# -gt 0 ]; do
  case "$1" in
    -i)          INPUT="$2"; shift ;;
    -o)          OUTPUT="$2"; shift ;;
    --edit-cmd)  EDIT_CMD="$2"; shift ;;
    --inject-overlay) INJECT_OVERLAY=1 ;;
    --no-inject|--passthrough) INJECT_OVERLAY=0 ;;
    --engine)    ENGINE_SRC="$2"; shift ;;
    --so)        SO_OVERRIDE="$2"; shift ;;
    --pause)     PAUSE=1 ;;
    --keystore)  KS_FILE="$2"; shift ;;
    --ks-pass)   KS_PASS="$2"; shift ;;
    --ks-alias)  KS_ALIAS="$2"; shift ;;
    --keep)      KEEP=1 ;;
    -h|--help)   usage; exit 0 ;;
    *) die "未知參數：$1（--help 看用法）" ;;
  esac
  shift
done

[ -n "$INPUT" ] || { usage >&2; die "缺少 -i <輸入 APK 或 APKS>"; }
[ -f "$INPUT" ] || die "找不到輸入檔：$INPUT"

# .apks 是 ZIP split bundle。apktool 只處理 base.apk，其餘 split 保留，
# 最後全部以同一張憑證重簽後再封裝回 .apks。
case "$INPUT" in
  *.apks)
    APKS_MODE=1
    APKS_INPUT_ORIGINAL="$INPUT"
    unzip -Z1 "$INPUT" | grep -Fxq 'toc.pb' && APKS_HAS_TOC=1 || true
    APKS_STAGE="$(mktemp -d "${TMPDIR:-/tmp}/repack-apks.XXXXXX")"
    unzip -q "$INPUT" -d "$APKS_STAGE" || die ".apks 解壓失敗"
    APKS_BASE_ORIGINAL="$APKS_STAGE/base.apk"
    [ -f "$APKS_BASE_ORIGINAL" ] || die ".apks 裡找不到根目錄 base.apk"
    INPUT="$APKS_BASE_ORIGINAL"
    ;;
esac

# 預設輸出：<輸入去掉 .apk>-repack.apk，放在同一個目錄
if [ -z "$OUTPUT" ]; then
  if [ "$INJECT_OVERLAY" -eq 1 ]; then SUFFIX="-ce"; else SUFFIX="-repack"; fi
  if [ "$APKS_MODE" -eq 1 ]; then
    OUTPUT="${INPUT%.apk}-ce.apks"
    # INPUT 已改成暫存 base.apk，使用原始 bundle 名稱。
    OUTPUT="${APKS_INPUT_ORIGINAL%.apks}-ce.apks"
  else case "$INPUT" in
    *.apk) OUTPUT="${INPUT%.apk}${SUFFIX}.apk" ;;
    *)     OUTPUT="${INPUT}${SUFFIX}.apk" ;;
  esac; fi
fi

# ---------------------------------------------------------------- 工具

winpath() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

find_java() {
  if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ];     then printf '%s' "$JAVA_HOME/bin/java"; return; fi
  if [ -n "${JAVA_HOME:-}" ] && [ -f "$JAVA_HOME/bin/java.exe" ]; then printf '%s' "$JAVA_HOME/bin/java.exe"; return; fi
  for j in /c/devtools/jdk-17 "/c/Program Files/Java/jdk-17"; do
    [ -f "$j/bin/java.exe" ] && printf '%s' "$j/bin/java.exe" && return
  done
  command -v java >/dev/null 2>&1 && { printf '%s' "$(command -v java)"; return; }
  return 1
}

find_apktool() {
  [ -n "${APKTOOL:-}" ] && [ -f "$APKTOOL" ] && { printf '%s' "$APKTOOL"; return; }
  for p in "$HOME/kt/apktool.jar" "$SCRIPT_DIR/apktool.jar" ./apktool.jar; do
    [ -f "$p" ] && printf '%s' "$p" && return
  done
  # PATH 上的 apktool 包裝腳本
  command -v apktool >/dev/null 2>&1 && { printf '%s' "apktool"; return; }
  return 1
}

find_sdk() {
  [ -n "${ANDROID_HOME:-}" ] && { printf '%s' "$ANDROID_HOME"; return; }
  [ -n "${ANDROID_SDK_ROOT:-}" ] && { printf '%s' "$ANDROID_SDK_ROOT"; return; }
  if [ -f "$SCRIPT_DIR/local.properties" ]; then
    local d; d="$(sed -n 's/^sdk\.dir=//p' "$SCRIPT_DIR/local.properties" | tr -d '\r' | tail -1)"
    [ -n "$d" ] && { printf '%s' "$d"; return; }
  fi
  [ -d "/c/devtools/Android/Sdk" ] && { printf '%s' "/c/devtools/Android/Sdk"; return; }
  return 1
}

find_build_tool() {
  local name="$1" sdk="$2" d p
  # 由新到舊挑 build-tools：較新的版本才有 zipalign -P（16KB 對齊）等旗標。
  for d in $(ls -1 "$sdk"/build-tools 2>/dev/null | sort -Vr); do
    for p in "$sdk/build-tools/$d/$name" "$sdk/build-tools/$d/$name.bat" "$sdk/build-tools/$d/$name.exe"; do
      [ -f "$p" ] && printf '%s' "$p" && return
    done
  done
  return 1
}

step "工具"

JAVA="$(find_java)" || die "找不到 java。設定 JAVA_HOME 指向 JDK 17。"
info "java      $JAVA"

APKTOOL="$(find_apktool)" || die "找不到 apktool。
        下載：curl -fsSL -o ~/kt/apktool.jar \\
              https://github.com/iBotPeaches/Apktool/releases/download/v2.9.3/apktool_2.9.3.jar
        或設定 APKTOOL=<路徑到 apktool.jar>"
# 統一成可呼叫的形式
if [ "$APKTOOL" = "apktool" ]; then
  apktool_run() { apktool "$@"; }
  info "apktool   （PATH 上的包裝腳本）"
else
  apktool_run() { "$JAVA" -jar "$(winpath "$APKTOOL")" "$@"; }
  info "apktool   $APKTOOL"
fi

SDK="$(find_sdk)" || die "找不到 Android SDK（要用它的 zipalign / apksigner）。設定 ANDROID_HOME。"
ZIPALIGN="${ZIPALIGN:-$(find_build_tool zipalign "$SDK")}"  || die "找不到 zipalign"
APKSIGNER="${APKSIGNER:-$(find_build_tool apksigner "$SDK")}" || die "找不到 apksigner"
info "zipalign  $ZIPALIGN"
info "apksigner $APKSIGNER"

# ---------------------------------------------------------------- 簽章金鑰

# 順序：--keystore 明指 > android/signing.properties > debug keystore
resolve_key() {
  if [ -n "$KS_FILE" ]; then
    info "keystore  $KS_FILE（--keystore）"
    return
  fi
  local props="$SCRIPT_DIR/signing.properties"
  if [ -f "$props" ]; then
    local sf; sf="$(sed -n 's/^storeFile=//p' "$props" | tr -d '\r' | tail -1)"
    case "$sf" in /*|?:*) : ;; *) sf="$SCRIPT_DIR/$sf" ;; esac
    KS_FILE="$sf"
    [ -z "$KS_PASS" ]  && KS_PASS="$(sed -n 's/^storePassword=//p' "$props" | tr -d '\r' | tail -1)"
    [ -z "$KS_ALIAS" ] && KS_ALIAS="$(sed -n 's/^keyAlias=//p' "$props" | tr -d '\r' | tail -1)"
    info "keystore  $KS_FILE（signing.properties）"
    return
  fi
  # AGP 的 debug keystore
  KS_FILE="$HOME/.android/debug.keystore"
  [ -f "$KS_FILE" ] || die "找不到任何 keystore（--keystore 指定，或先跑 keystore.sh）"
  [ -z "$KS_PASS" ]  && KS_PASS="android"
  [ -z "$KS_ALIAS" ] && KS_ALIAS="androiddebugkey"
  info "keystore  $KS_FILE（debug keystore）"
}
resolve_key

# ---------------------------------------------------------------- 流程

WORK="$(mktemp -d "${TMPDIR:-/tmp}/repack.XXXXXX")"
UNSIGNED="$WORK.unsigned.apk"
ALIGNED="$WORK.aligned.apk"
cleanup() {
  [ "$KEEP" -eq 1 ] || rm -rf "$WORK" "$UNSIGNED" "$ALIGNED" "$APKS_STAGE"
}
trap cleanup EXIT

# 1. 拆解 ----------------------------------------------------------------
step "1／5　apktool d — 拆解"
rm -rf "$WORK"
apktool_run d -f --no-src -o "$(winpath "$WORK")" "$(winpath "$INPUT")" >/dev/null 2>"$WORK.log" \
  || { cat "$WORK.log" >&2; die "apktool 拆解失敗"; }
ok "拆解到 $WORK"
note "manifest→文字、res 還原；原始 dex 保留不回編"

# --- overlay 注入 -------------------------------------------------------
inject_overlay() {
  # 找引擎來源 APK
  local eng="$ENGINE_SRC"
  if [ -z "$eng" ]; then
    eng="$(ls -t "$SCRIPT_DIR"/dist/cheat-engine-*.apk 2>/dev/null | head -1)"
  fi
  [ -n "$eng" ] && [ -f "$eng" ] || die "找不到引擎 APK。先 ./pack.sh 產生，或用 --engine <apk> 指定。"
  info "引擎來源  $eng"

  # ① dex：目標 APK 使用 --no-src 拆解，原始 dex 不回編；把引擎 dex
  #    追加成新的 classesN.dex，避免大型/混淆 dex 的 16-bit overflow。
  local next=2 d base count=0
  while [ -f "$WORK/classes${next}.dex" ]; do next=$((next + 1)); done
  while IFS= read -r d; do
    base="$(basename "$d")"
    unzip -p "$eng" "$base" > "$WORK/classes${next}.dex" || die "抽取引擎 $base 失敗"
    count=$((count + 1)); next=$((next + 1))
  done < <(unzip -Z1 "$eng" | grep -E '^classes([2-9][0-9]*)?\.dex$' | sort -V)
  [ "$count" -gt 0 ] || die "引擎 APK 沒有 classes*.dex"
  ok "追加 $count 個 overlay dex（原始 dex 不重組）"

  # ② .so：每個 ABI 一份
  local edec="$WORK.engine" abi so=0 libentry
  for abi in arm64-v8a armeabi-v7a x86_64; do
    libentry="lib/$abi/libce_engine.so"
    if unzip -Z1 "$eng" | grep -Fxq "$libentry"; then
      mkdir -p "$edec/lib/$abi"
      unzip -p "$eng" "$libentry" > "$edec/$libentry" || die "抽取引擎 $libentry 失敗"
    fi
  done
  for abi in arm64-v8a armeabi-v7a x86_64; do
    if [ -f "$edec/lib/$abi/libce_engine.so" ]; then
      mkdir -p "$WORK/lib/$abi"
      cp "$edec/lib/$abi/libce_engine.so" "$WORK/lib/$abi/"
      so=$((so + 1))
    fi
  done
  ok "注入 libce_engine.so × $so 個 ABI"

  # ②′ 用指定的 loose .so 覆寫 arm64-v8a（--so；預設抓 target/release 的新建產物）
  local so_src="$SO_OVERRIDE"
  if [ -z "$so_src" ]; then
    local def_ndk="$SCRIPT_DIR/overlay/src/main/jniLibs/arm64-v8a/libce_engine.so"
    local def_cargo="$SCRIPT_DIR/../target/aarch64-linux-android/release/libce_engine.so"
    if [ -f "$def_ndk" ]; then
      so_src="$def_ndk"
    elif [ -f "$def_cargo" ]; then
      so_src="$def_cargo"
    fi
  fi
  if [ -n "$so_src" ]; then
    [ -f "$so_src" ] || die "--so 指定的檔案不存在：$so_src"
    mkdir -p "$WORK/lib/arm64-v8a"
    cp "$so_src" "$WORK/lib/arm64-v8a/libce_engine.so"
    ok "arm64-v8a 改用指定的 .so：$so_src（$(wc -c < "$so_src") bytes）"
  fi

  # ③ manifest：加 provider（自動啟動點）。用 python 改，避免 sed 跳脫地獄。
  python - "$WORK/AndroidManifest.xml" <<'PYIN'
import io, os, re, sys
mf = sys.argv[1]
s = io.open(mf, encoding="utf-8").read()
m = re.search(r'package="([^"]*)"', s)
if not m:
    sys.exit("read package failed")
pkg = m.group(1)
# 注入的 .so 被 apktool 壓縮、而目標原本 extractNativeLibs=false 的話，
# 安裝會失敗（Failed to extract native libraries）。強制 true 讓系統解出來，
# 對「本來沒有原生庫、被我們塞進 .so」的目標一定要這樣。
import re as _re
if _re.search(r'android:extractNativeLibs="[^"]*"', s):
    s = _re.sub(r'android:extractNativeLibs="[^"]*"', 'android:extractNativeLibs="true"', s, count=1)
else:
    s = s.replace("<application ", '<application android:extractNativeLibs="true" ', 1)
nl = chr(10)
lines = [
    '        <provider android:name="dev.marc.ce.overlay.OverlayInstallProvider"',
    '            android:authorities="' + pkg + '.ceoverlay"',
    '            android:exported="false" android:initOrder="100"/>',
]
prov = nl.join(lines) + nl
if "OverlayInstallProvider" in s:
    print("  provider already present, skip")
else:
    s = s.replace("</application>", prov + "    </application>", 1)
    io.open(mf, "w", encoding="utf-8", newline=nl).write(s)
    print("  provider added, authority=" + pkg + ".ceoverlay")
PYIN
  ok "manifest 已註冊 OverlayInstallProvider（App 一啟動就自我安裝）"
  rm -rf "$edec"
}

# 2. 編輯 ----------------------------------------------------------------
step "2／5　編輯"
if [ "$INJECT_OVERLAY" -eq 1 ]; then
  info "注入 cheat-enginer 引擎…"
  inject_overlay
fi
if [ -n "$EDIT_CMD" ]; then
  info "跑 --edit-cmd（\$WORK=$WORK）"
  ( export WORK; bash -c "$EDIT_CMD" ) || die "--edit-cmd 失敗"
  ok "編輯指令完成"
elif [ "$PAUSE" -eq 1 ]; then
  warn "已拆解到：$WORK"
  info "去改 AndroidManifest.xml / smali/ / lib/ ，改好回來按 Enter 繼續…"
  read -r _
  ok "繼續"
elif [ "$INJECT_OVERLAY" -eq 0 ]; then
  note "--no-inject：只拆解→重組→重簽，不放入引擎（換簽章 / 驗證管線）"
fi

# 2b. 確保注入的 .so 未壓縮 --------------------------------------------------
# extractNativeLibs="false" 要求 .so 以 STORED 打包；zipalign 也只能對齊 STORED
# entry。apktool 依 apktool.yml 的 doNotCompress 決定壓不壓，預設只列 resources.arsc，
# 所以注入 lib/ 後要把副檔名 so 加進去，否則 apktool b 會 deflate、載入時 mmap 失敗。
if ls "$WORK"/lib/*/*.so >/dev/null 2>&1; then
  YML="$WORK/apktool.yml"
  if [ -f "$YML" ] && ! grep -qE '^-[[:space:]]+so[[:space:]]*$' "$YML"; then
    if grep -q '^doNotCompress:' "$YML"; then
      sed -i '/^doNotCompress:/a - so' "$YML"
    else
      printf 'doNotCompress:\n- so\n' >> "$YML"
    fi
    ok "apktool.yml：so 已加入 doNotCompress（.so 以 STORED 打包）"
  fi
fi

# 3. 重組 ----------------------------------------------------------------
step "3／5　apktool b — 重組"
apktool_run b -f -o "$(winpath "$UNSIGNED")" "$(winpath "$WORK")" >/dev/null 2>"$WORK.blog" \
  || { cat "$WORK.blog" >&2; die "apktool 重組失敗（大型/split/混淆的 App 常卡在這）"; }
ok "重組出未簽章 APK"

# 4. 對齊 ----------------------------------------------------------------
step "4／5　zipalign — 對齊"
"$ZIPALIGN" -f -P 16 4 "$(winpath "$UNSIGNED")" "$(winpath "$ALIGNED")" \
  || die "zipalign 失敗"
ok "16 KB 對齊完成"

# 5. 簽章 ----------------------------------------------------------------
step "5／5　apksigner — 重新簽章"
if [ "$APKS_MODE" -eq 1 ]; then
  local_signed="$WORK/base-signed.apk"
  "$APKSIGNER" sign --ks "$(winpath "$KS_FILE")" --ks-pass "pass:$KS_PASS" --ks-key-alias "$KS_ALIAS" --out "$(winpath "$local_signed")" "$(winpath "$ALIGNED")" || die "base.apk 簽章失敗"
  cp "$local_signed" "$APKS_STAGE/base.apk"
  while IFS= read -r split; do
    [ "$split" = "$APKS_STAGE/base.apk" ] && continue
    signed="$WORK/$(basename "$split").signed.apk"
    "$APKSIGNER" sign --ks "$(winpath "$KS_FILE")" --ks-pass "pass:$KS_PASS" --ks-key-alias "$KS_ALIAS" --out "$(winpath "$signed")" "$(winpath "$split")" || die "split APK 簽章失敗：$(basename "$split")"
    cp "$signed" "$split"
  done < <(find "$APKS_STAGE" -type f -name '*.apk' | sort)
  rm -f "$OUTPUT"
  bundle_out="$(cd "$(dirname "$OUTPUT")" && pwd)/$(basename "$OUTPUT")"
  (cd "$APKS_STAGE" && zip -q -r "$(winpath "$bundle_out")" .) || die ".apks 封裝失敗"
  ok "已簽章並封裝 → $OUTPUT"
else
"$APKSIGNER" sign \
  --ks "$(winpath "$KS_FILE")" \
  --ks-pass "pass:$KS_PASS" \
  --ks-key-alias "$KS_ALIAS" \
  --out "$(winpath "$OUTPUT")" \
  "$(winpath "$ALIGNED")" \
  || die "apksigner 簽章失敗（密碼或別名不對？）"

cert="$("$APKSIGNER" verify --print-certs "$(winpath "$OUTPUT")" 2>/dev/null \
  | grep -m1 -i 'certificate SHA-256 digest' | awk '{print $NF}')"
ok "已簽章 → $OUTPUT"
[ -n "$cert" ] && note "憑證 ${cert:0:16}…"
fi

printf '\n%s完成%s  %s\n' "$BOLD" "$RESET" "$OUTPUT"
warn "簽章已改。安裝前先移除舊版："
if [ "$APKS_MODE" -eq 1 ]; then
  if [ "$APKS_HAS_TOC" -eq 1 ]; then
    note "  adb uninstall <package>  &&  bundletool install-apks --apks=$OUTPUT"
  else
    note "  此 APKS 没有 toc.pb，请使用 adb install-multiple 安装其中全部 APK"
  fi
else
  note "  adb uninstall <package>  &&  adb install $OUTPUT"
fi
[ "$KEEP" -eq 1 ] && note "work 目錄保留於：$WORK"
printf '\n'
