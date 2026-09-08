#!/usr/bin/env bash
#
# keystore.sh — 管理三支 APK 共用的簽章金鑰。
#
#   ./keystore.sh new [選項]     產生一把新金鑰並設為使用中
#   ./keystore.sh use <路徑>     改用一把既有的 keystore
#   ./keystore.sh show           顯示目前設定與憑證指紋
#   ./keystore.sh clear          移除設定，退回 AGP 預設的 debug keystore
#
# new 的選項（沒給的會互動詢問或用預設值）：
#   --out <檔案>      預設 ./release.jks
#   --alias <別名>    預設 shared
#   --cn <名稱>       憑證的 CN，例如你註冊的公司或個人名稱
#   --org <組織>      憑證的 O
#   --country <代碼>  兩碼國別，預設 TW
#   --validity <天>   預設 10950（30 年）
#   --storepass / --keypass <密碼>   不給則互動輸入
#
# ---------------------------------------------------------------------------
# 為什麼三支 APK 一定要同一把金鑰
#
# sharedUserId 的成立條件是「同一個 uid + 同一張憑證」。憑證不同，安裝時會被
# 擋下（INSTALL_FAILED_SHARED_USER_INCOMPATIBLE），而錯誤訊息完全指不到重點。
# 所以這支腳本寫的是「一份」設定，:cheatengine、:game、:sample 三個模組共用。
#
# 換金鑰 = 換身分。裝置上已經裝好的舊版必須先解除安裝，Google Play 上已上架
# 的 App 則根本不能換（除非走 Play App Signing 的金鑰輪替流程）。所以在正式
# 發佈之前就把金鑰定下來，而且務必備份 —— 弄丟了沒有任何補救方式。
# ---------------------------------------------------------------------------

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROPS="$SCRIPT_DIR/signing.properties"

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
usage() { sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

winpath() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

# keytool 預設用系統語系輸出。中文 Windows 上那是 Big5，透過管線讀出來是亂碼，
# grep 的關鍵字也跟著失效。一律強制英文。
KEYTOOL_LOCALE=(-J-Duser.language=en -J-Duser.country=US)

find_java_tool() {
  local name="$1"
  if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/$name" ];     then printf '%s' "$JAVA_HOME/bin/$name"; return; fi
  if [ -n "${JAVA_HOME:-}" ] && [ -f "$JAVA_HOME/bin/$name.exe" ]; then printf '%s' "$JAVA_HOME/bin/$name.exe"; return; fi
  command -v "$name" >/dev/null 2>&1 && { printf '%s' "$(command -v "$name")"; return; }
  local j
  for j in /c/devtools/jdk-17 /c/devtools/jdk17 "/c/Program Files/Java/jdk-17"; do
    [ -f "$j/bin/$name.exe" ] && printf '%s' "$j/bin/$name.exe" && return
    [ -x "$j/bin/$name" ]     && printf '%s' "$j/bin/$name"     && return
  done
  return 1
}

read_prop() { sed -n "s/^$1=//p" "$PROPS" 2>/dev/null | tr -d '\r' | tail -1; }

# ---------------------------------------------------------------- show

cmd_show() {
  step "目前的簽章設定"
  if [ ! -f "$PROPS" ]; then
    info "signing.properties 不存在"
    note "三支 APK 目前用 AGP 預設的 debug keystore（~/.android/debug.keystore）"
    note "那把金鑰足以側載測試，但不能用於正式發佈。"
    return 0
  fi

  local store alias
  store="$(read_prop storeFile)"
  alias="$(read_prop keyAlias)"
  info "設定檔   $PROPS"
  info "keystore $store"
  info "別名     $alias"

  local abs="$store"
  case "$store" in /*|?:*) ;; *) abs="$SCRIPT_DIR/$store" ;; esac
  if [ ! -f "$abs" ]; then
    warn "找不到 keystore 檔案：$abs"
    warn "建置會失敗。用 ./keystore.sh use <路徑> 指到正確位置。"
    return 1
  fi

  local keytool
  if keytool="$(find_java_tool keytool)"; then
    local pass; pass="$(read_prop storePassword)"
    printf '\n'
    "$keytool" "${KEYTOOL_LOCALE[@]}" -list -v \
      -keystore "$(winpath "$abs")" -alias "$alias" -storepass "$pass" 2>/dev/null \
      | grep -E "^Owner:|^Valid from:|SHA256:" | sed 's/^/    /' \
      || warn "讀不到憑證內容（密碼或別名不對？）"
  fi
}

# ---------------------------------------------------------------- write

write_props() {
  local store="$1" storepass="$2" alias="$3" keypass="$4"
  cat > "$PROPS" <<EOF
# 由 keystore.sh 產生。已被 .gitignore 排除 —— 絕對不要提交這個檔案。
#
# :cheatengine、:game、:sample 三個模組都會讀這份設定。sharedUserId 要求
# 三支 APK 用同一張憑證，所以這裡只有「一份」。
storeFile=$store
storePassword=$storepass
keyAlias=$alias
keyPassword=$keypass
EOF
  chmod 600 "$PROPS" 2>/dev/null || true
}

after_change_notice() {
  printf '\n'
  warn "換金鑰等於換身分。接下來一定要做的事："
  note "  1. 裝置上舊版必須解除安裝，否則會得到 INSTALL_FAILED_UPDATE_INCOMPATIBLE"
  note "     adb uninstall <ce package>; adb uninstall <game package>"
  note "  2. 重新建置：./pack.sh    （會驗證三支簽章一致）"
  note "  3. 備份 keystore 與密碼。弄丟了沒有任何補救方式。"
}

# ---------------------------------------------------------------- new

cmd_new() {
  local out="$SCRIPT_DIR/release.jks"
  local alias="shared" cn="" org="" country="TW" validity="10950"
  local storepass="" keypass=""

  while [ $# -gt 0 ]; do
    case "$1" in
      --out)       out="$2"; shift ;;
      --alias)     alias="$2"; shift ;;
      --cn)        cn="$2"; shift ;;
      --org)       org="$2"; shift ;;
      --country)   country="$2"; shift ;;
      --validity)  validity="$2"; shift ;;
      --storepass) storepass="$2"; shift ;;
      --keypass)   keypass="$2"; shift ;;
      *) die "new：未知選項 $1" ;;
    esac
    shift
  done

  local keytool
  keytool="$(find_java_tool keytool)" || die "找不到 keytool。請設定 JAVA_HOME 指向 JDK。"

  [ -f "$out" ] && die "$out 已經存在。換個 --out，或先自行移走（不要覆蓋既有金鑰）。"

  step "產生新的簽章金鑰"

  if [ -z "$cn" ]; then
    printf '    憑證名稱（CN，例如你註冊的公司或個人名稱）：'
    read -r cn
    [ -n "$cn" ] || die "CN 不能空白"
  fi
  [ -n "$org" ] || org="$cn"

  if [ -z "$storepass" ]; then
    printf '    keystore 密碼（至少 6 碼，不會顯示）：'
    read -rs storepass; printf '\n'
    printf '    再輸入一次：'
    read -rs confirm; printf '\n'
    [ "$storepass" = "$confirm" ] || die "兩次輸入不一致"
  fi
  [ ${#storepass} -ge 6 ] || die "keystore 密碼至少要 6 碼"
  # 金鑰密碼與 keystore 密碼相同是最常見也最不容易出錯的做法
  [ -n "$keypass" ] || keypass="$storepass"

  local dname="CN=$cn, O=$org, C=$country"
  info "別名     $alias"
  info "DN       $dname"
  info "有效期   $validity 天（約 $((validity / 365)) 年）"

  # stderr 是 keytool 的「已產生…」訊息，我們自己會印，這裡不用重複
  "$keytool" "${KEYTOOL_LOCALE[@]}" -genkeypair \
    -keystore "$(winpath "$out")" \
    -alias "$alias" \
    -keyalg RSA -keysize 4096 \
    -validity "$validity" \
    -dname "$dname" \
    -storepass "$storepass" \
    -keypass "$keypass" \
    -storetype PKCS12 \
    >/dev/null 2>&1

  ok "已產生 $out"

  # 設定檔裡存相對路徑，整個目錄搬家也不會壞
  local rel="$out"
  case "$out" in "$SCRIPT_DIR"/*) rel="${out#"$SCRIPT_DIR"/}" ;; esac
  write_props "$rel" "$storepass" "$alias" "$keypass"
  ok "已寫入 signing.properties（三個模組共用）"

  after_change_notice
}

# ---------------------------------------------------------------- use

cmd_use() {
  local path="${1:-}"
  [ -n "$path" ] || die "用法：./keystore.sh use <keystore 路徑> [--alias 別名]"
  shift || true

  local alias="shared" storepass="" keypass=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --alias)     alias="$2"; shift ;;
      --storepass) storepass="$2"; shift ;;
      --keypass)   keypass="$2"; shift ;;
      *) die "use：未知選項 $1" ;;
    esac
    shift
  done

  [ -f "$path" ] || die "找不到 $path"

  step "改用既有的 keystore"

  if [ -z "$storepass" ]; then
    printf '    keystore 密碼（不會顯示）：'
    read -rs storepass; printf '\n'
  fi
  [ -n "$keypass" ] || keypass="$storepass"

  # 先驗證真的打得開，免得留下一份會讓建置失敗的設定
  local keytool
  if keytool="$(find_java_tool keytool)"; then
    "$keytool" "${KEYTOOL_LOCALE[@]}" -list \
      -keystore "$(winpath "$path")" -alias "$alias" -storepass "$storepass" >/dev/null 2>&1 \
      || die "打不開 keystore：密碼或別名 '$alias' 不對"
    ok "密碼與別名驗證通過"
  else
    warn "找不到 keytool，略過驗證"
  fi

  local abs rel
  abs="$(cd "$(dirname "$path")" && pwd)/$(basename "$path")"
  rel="$abs"
  case "$abs" in "$SCRIPT_DIR"/*) rel="${abs#"$SCRIPT_DIR"/}" ;; esac
  write_props "$rel" "$storepass" "$alias" "$keypass"
  ok "已寫入 signing.properties"

  after_change_notice
}

# ---------------------------------------------------------------- clear

cmd_clear() {
  step "移除簽章設定"
  if [ -f "$PROPS" ]; then
    rm -f "$PROPS"
    ok "已刪除 signing.properties"
    note "keystore 檔案本身沒有動，三支 APK 會退回 debug keystore。"
    after_change_notice
  else
    info "本來就沒有設定，不需要動作"
  fi
}

# ---------------------------------------------------------------- 進入點

case "${1:-show}" in
  new)   shift; cmd_new "$@" ;;
  use)   shift; cmd_use "$@" ;;
  show)  cmd_show ;;
  clear) cmd_clear ;;
  -h|--help|help) usage ;;
  *) echo "未知指令：$1" >&2; echo >&2; usage >&2; exit 2 ;;
esac
