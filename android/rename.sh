#!/usr/bin/env bash
#
# rename.sh — 把整個專案改成你自己的 package 名稱與 App 顯示名稱。
#
#   ./rename.sh --base com.acme.mem \
#               --ce-label "Acme Memory" \
#               --game-label "Acme Arena"
#
#   ./rename.sh --base com.acme.mem --dry-run    只顯示會改什麼，不動檔案
#   ./rename.sh --show                           顯示目前的名稱
#
# 選項：
#   --base <package>     新的 package 前綴。其餘名稱由它推導：
#                          <base>.app      Cheat Engine 的 applicationId
#                          <base>.game     遊戲的 applicationId
#                          <base>.sample   嵌入模式示範的 applicationId
#                          <base>.overlay  AAR 的 Java package
#                          <base>.shared       sharedUserId
#                          <base>.shared.proc  android:process
#   --ce-label <名稱>    Cheat Engine 的顯示名稱
#   --game-label <名稱>  遊戲的顯示名稱
#   --sample-label <名稱>
#   --shared-user-id <id>  改用指定的 sharedUserId，不跟著 --base 推導
#   --process <名稱>       改用指定的 android:process，不跟著 --base 推導
#   --dry-run            預演
#   --yes                跳過確認
#
# sharedUserId 與 process 名稱預設由 --base 推導成 <base>.shared 與
# <base>.shared.proc，多數情況不需要另外指定。要獨立指定的時機是：
# 這兩支 App 得加入一個「已經存在」的共用身分（例如你另一組已上架的
# App 已經用了某個 sharedUserId），那時 package 名稱與共用身分不必一致。
#
# 注意 sharedUserId 是「名稱」，不是數字：
#   android:sharedUserId="dev.marc.ce.shared"   ← 我們宣告的，固定不變
#   uid 10220                                    ← 系統安裝時分配的，會變
# App 無法指定 uid。共用行程靠的是兩支宣告了相同的「名稱」。
#
# ---------------------------------------------------------------------------
# 為什麼需要一支腳本，而不是全域搜尋取代
#
# 名稱散在六種地方，其中一種漏掉不會在編譯時報錯，而是在執行期才炸：
#
#   1. Gradle 的 namespace / applicationId
#   2. Java 的目錄結構、package 宣告與 import
#   3. Manifest 的 sharedUserId、android:process、<queries>、provider 全名
#   4. Java 裡寫死的字串（ACTION_STOP、要啟動的遊戲 package）
#   5. ★ Rust 的 JNI 匯出符號 —— Java_<base>_overlay_NativeBridge_xxx
#      JNI 用「函式名稱」對應 Java 方法，Java package 一改，24 個 Rust 匯出
#      符號就全部對不上。編譯完全正常，一呼叫就 UnsatisfiedLinkError。
#   6. sharedUserId 一改，uid 就是新的 —— 裝置上的舊版必須先解除安裝。
#
# 這支腳本會一次改完，並且是可逆的：再跑一次 --base <舊名稱> 就換回來。
#
# ---------------------------------------------------------------------------
# 為什麼 sharedUserId 設在這裡，而不是 pack.sh
#
# 因為它是「身分」，不是建置選項 —— 跟 applicationId 同一類，安裝當下就被
# 寫進系統、之後不能改。設在打包腳本上會讓版控裡的 manifest 說謊：用
# Android Studio 或 gradle installDebug 建出來的 APK，跟用 pack.sh 建出來的
# 會有不同的共用身分，而症狀是「兩支都裝得起來、掃描卻找不到東西」，正是
# 這個專案最想根除的那種無聲失敗。
#
# 所以分工是：rename.sh 設定身分，pack.sh 驗證身分一致 —— 就跟
# keystore.sh 產生金鑰、pack.sh 驗證三支簽章相同是同一個形狀。
# ---------------------------------------------------------------------------

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_JNI="$REPO_ROOT/src/android/mod.rs"

NEW_BASE=""
NEW_CE_LABEL=""
NEW_GAME_LABEL=""
NEW_SAMPLE_LABEL=""
NEW_SUID=""
NEW_PROC=""
DRY_RUN=0
ASSUME_YES=0
SHOW_ONLY=0

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
usage() { sed -n '2,41p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --base)         NEW_BASE="$2"; shift ;;
    --ce-label)     NEW_CE_LABEL="$2"; shift ;;
    --game-label)   NEW_GAME_LABEL="$2"; shift ;;
    --sample-label) NEW_SAMPLE_LABEL="$2"; shift ;;
    --shared-user-id) NEW_SUID="$2"; shift ;;
    --process)        NEW_PROC="$2"; shift ;;
    --dry-run)      DRY_RUN=1 ;;
    --yes|-y)       ASSUME_YES=1 ;;
    --show)         SHOW_ONLY=1 ;;
    -h|--help)      usage; exit 0 ;;
    *) die "未知參數：$1（--help 看用法）" ;;
  esac
  shift
done

# ---------------------------------------------------------------- 讀出現況

# 從 :overlay 的 namespace 反推目前的 base，所以這支腳本可以重複執行，
# 也可以用來改回舊名稱。
read_attr() { sed -n "s/.*$1=\"\([^\"]*\)\".*/\1/p" "$2" | head -1; }

CUR_OVERLAY_NS="$(sed -n 's/.*namespace = "\(.*\)"/\1/p' "$SCRIPT_DIR/overlay/build.gradle.kts" | head -1)"
[ -n "$CUR_OVERLAY_NS" ] || die "讀不到 overlay 的 namespace，專案結構可能已經被改過了"
CUR_BASE="${CUR_OVERLAY_NS%.overlay}"
[ "$CUR_BASE" != "$CUR_OVERLAY_NS" ] || die "overlay 的 namespace 不是 <base>.overlay 的形式：$CUR_OVERLAY_NS"

CE_MANIFEST="$SCRIPT_DIR/cheatengine/src/main/AndroidManifest.xml"
GAME_MANIFEST="$SCRIPT_DIR/game/src/main/AndroidManifest.xml"

# 從 manifest 讀「實際宣告的值」，而不是從 base 推導 —— 這樣先前用
# --shared-user-id 指定過的自訂值才看得出來。
CUR_SUID="$(read_attr 'android:sharedUserId' "$CE_MANIFEST")"
CUR_PROC="$(read_attr 'android:process' "$CE_MANIFEST")"
GAME_SUID="$(read_attr 'android:sharedUserId' "$GAME_MANIFEST")"
GAME_PROC="$(read_attr 'android:process' "$GAME_MANIFEST")"
if [ "$CUR_SUID" != "$GAME_SUID" ] || [ "$CUR_PROC" != "$GAME_PROC" ]; then
  warn "兩支 manifest 的共用身分目前不一致，這支腳本會把它們對齊："
  note "  CE   sharedUserId=$CUR_SUID  process=$CUR_PROC"
  note "  Game sharedUserId=$GAME_SUID  process=$GAME_PROC"
fi

CUR_CE_LABEL="$(read_attr 'android:label' "$CE_MANIFEST")"
CUR_GAME_LABEL="$(read_attr 'android:label' "$SCRIPT_DIR/game/src/main/AndroidManifest.xml")"
CUR_SAMPLE_LABEL="$(read_attr 'android:label' "$SCRIPT_DIR/sample/src/main/AndroidManifest.xml")"

if [ "$SHOW_ONLY" -eq 1 ]; then
  step "目前的名稱"
  info "base            $CUR_BASE"
  info "Cheat Engine    $CUR_BASE.app        「$CUR_CE_LABEL」"
  info "遊戲            $CUR_BASE.game       「$CUR_GAME_LABEL」"
  info "嵌入模式示範    $CUR_BASE.sample     「$CUR_SAMPLE_LABEL」"
  info "AAR             $CUR_BASE.overlay"
  info "sharedUserId    $CUR_SUID"
  info "android:process $CUR_PROC"
  printf '\n'
  exit 0
fi

# 只想改共用身分或顯示名稱時，--base 可以省略
[ -n "$NEW_BASE" ] || NEW_BASE="$CUR_BASE"

# ---------------------------------------------------------------- 驗證

# Java package 規則：至少兩段、每段小寫字母開頭。多一層檢查省得在
# 建置到一半才發現。
validate_package() {
  local pkg="$1" seg
  case "$pkg" in
    *.*) ;;
    *) die "package 至少要兩段（例如 com.acme），拿到的是：$pkg" ;;
  esac
  local IFS='.'
  for seg in $pkg; do
    [ -n "$seg" ] || die "package 有空白段落：$pkg"
    case "$seg" in
      [a-z]*) ;;
      *) die "package 的每一段都要以小寫字母開頭，「$seg」不符合" ;;
    esac
    case "$seg" in
      *[!a-z0-9_]*) die "package 只能用小寫字母、數字與底線，「$seg」不符合" ;;
    esac
    case " int new class package private public static void final if for do try " in
      *" $seg "*) die "「$seg」是 Java 保留字，不能當 package 段落" ;;
    esac
  done
}
validate_package "$NEW_BASE"

# --shared-user-id / --process 沒給就跟著 base 走
[ -n "$NEW_SUID" ] || NEW_SUID="${CUR_SUID/#$CUR_BASE/$NEW_BASE}"
[ -n "$NEW_PROC" ] || NEW_PROC="${CUR_PROC/#$CUR_BASE/$NEW_BASE}"

# 這兩條規則違反了會安裝失敗或安靜地不共用，訊息都指不到重點，所以先擋。
case "$NEW_SUID" in
  *[!0-9]*) ;;
  *) die "sharedUserId 是「名稱」不是數字，你給的是 $NEW_SUID。

        很容易混淆的兩件事：
          android:sharedUserId   我們宣告的名稱，例如 dev.marc.ce.shared
          Linux uid              系統在安裝時分配的號碼，例如 10220

        uid 由系統決定，App 無法指定，而且每次重新安裝、每台裝置都不一樣
        （這個專案開發過程中就出現過 10215 / 10218 / 10219 / 10220）。
        兩支 APK 共用行程靠的是「名稱相同」，系統再把同一個名稱對應到
        同一個 uid —— 你要固定的是名稱，不是號碼。

        （唯一有固定號碼的是 android.uid.system 這類平台預留名稱，
        那需要平台簽章並安裝在系統分割區，側載的 App 用不到。）" ;;
esac
case "$NEW_SUID" in
  *.*) ;;
  *) die "sharedUserId 必須含有 '.'（Android 的硬性規定）：$NEW_SUID" ;;
esac
case "$NEW_PROC" in
  :*) die "android:process 不能以 ':' 開頭 —— 那是 package 私有的行程，
        永遠不可能被另一支 APK 共用。請用全域名稱。" ;;
  *.*) ;;
  *) die "全域 android:process 名稱必須含有 '.'：$NEW_PROC" ;;
esac

if [ "$NEW_BASE" = "$CUR_BASE" ] && [ "$NEW_SUID" = "$CUR_SUID" ] \
   && [ "$NEW_PROC" = "$CUR_PROC" ] \
   && [ -z "$NEW_CE_LABEL$NEW_GAME_LABEL$NEW_SAMPLE_LABEL" ]; then
  die "沒有任何東西要改"
fi

case "$NEW_BASE" in
  *_*) warn "package 含底線，JNI 符號會用 _1 轉義（腳本已處理，但不建議）" ;;
esac

# JNI 符號的名稱轉換規則：底線先變 _1，再把點變底線。
jni_mangle() { printf '%s' "$1" | sed 's/_/_1/g; s/\./_/g'; }

CUR_JNI="$(jni_mangle "$CUR_BASE.overlay")"
NEW_JNI="$(jni_mangle "$NEW_BASE.overlay")"

CUR_PATH="$(printf '%s' "$CUR_BASE" | tr '.' '/')"
NEW_PATH="$(printf '%s' "$NEW_BASE" | tr '.' '/')"

# ---------------------------------------------------------------- 計畫

step "改名計畫"
printf '    %-22s %s\n' "package base"    "$CUR_BASE  →  $NEW_BASE"
printf '    %-22s %s\n' "Java 目錄"       "$CUR_PATH  →  $NEW_PATH"
printf '    %-22s %s\n' "JNI 符號前綴"    "Java_${CUR_JNI}_NativeBridge_  →  Java_${NEW_JNI}_NativeBridge_"
printf '    %-22s %s\n' "sharedUserId"    "$CUR_SUID  →  $NEW_SUID"
printf '    %-22s %s\n' "android:process" "$CUR_PROC  →  $NEW_PROC"
[ -n "$NEW_CE_LABEL" ]     && printf '    %-22s %s\n' "CE 顯示名稱"     "「$CUR_CE_LABEL」  →  「$NEW_CE_LABEL」"
[ -n "$NEW_GAME_LABEL" ]   && printf '    %-22s %s\n' "遊戲顯示名稱"    "「$CUR_GAME_LABEL」  →  「$NEW_GAME_LABEL」"
[ -n "$NEW_SAMPLE_LABEL" ] && printf '    %-22s %s\n' "示範顯示名稱"    "「$CUR_SAMPLE_LABEL」  →  「$NEW_SAMPLE_LABEL」"

# 要動到的文字檔。build/ 底下是產物，不碰。
mapfile -t TEXT_FILES < <(
  {
    find "$SCRIPT_DIR" \
      \( -name build -o -name .gradle -o -name dist -o -name jniLibs \) -prune -o \
      -type f \( -name '*.java' -o -name '*.kts' -o -name '*.xml' \
                 -o -name '*.md' -o -name '*.html' -o -name '*.sh' \) \
      ! -name 'rename.sh' -print
    printf '%s\n' "$RUST_JNI"
  } | sort -u
)

step "會被修改的檔案"
CHANGED=()
for f in "${TEXT_FILES[@]}"; do
  [ -f "$f" ] || continue
  if grep -qF -e "$CUR_BASE" -e "Java_${CUR_JNI}_" "$f" 2>/dev/null \
     || { [ -n "$NEW_CE_LABEL" ]     && grep -qF "$CUR_CE_LABEL" "$f" 2>/dev/null; } \
     || { [ -n "$NEW_GAME_LABEL" ]   && grep -qF "$CUR_GAME_LABEL" "$f" 2>/dev/null; } \
     || { [ -n "$NEW_SAMPLE_LABEL" ] && grep -qF "$CUR_SAMPLE_LABEL" "$f" 2>/dev/null; }; then
    CHANGED+=("$f")
    printf '    %s\n' "${f#"$REPO_ROOT"/}"
  fi
done
[ ${#CHANGED[@]} -gt 0 ] || die "沒有任何檔案含有舊名稱，先確認專案狀態"
info "共 ${#CHANGED[@]} 個檔案"

if [ "$DRY_RUN" -eq 1 ]; then
  printf '\n%s預演結束，沒有動任何檔案。%s\n\n' "$DIM" "$RESET"
  exit 0
fi

if [ "$ASSUME_YES" -ne 1 ]; then
  printf '\n    這會直接改寫上列檔案。建議先 commit 或備份。\n'
  printf '    繼續？[y/N] '
  read -r reply
  case "$reply" in y|Y|yes|YES) ;; *) die "已取消" ;; esac
fi

# ---------------------------------------------------------------- 執行

step "1／6　停掉 Gradle daemon 並清除建置產物"

# 必須在搬目錄「之前」做。daemon 會抓著 build/ 底下的檔案 handle，
# Windows 上那會讓 mv 直接拿到 Permission denied。
# 而且產生過的 R / BuildConfig 還帶著舊 package，留著只會給出誤導的編譯錯誤。
if command -v gradle >/dev/null 2>&1 || [ -x "$SCRIPT_DIR/gradlew" ]; then
  ( cd "$SCRIPT_DIR" && { [ -x ./gradlew ] && ./gradlew --stop || gradle --stop; } ) >/dev/null 2>&1 || true
else
  for g in /c/devtools/gradle/*/bin/gradle.bat; do
    [ -f "$g" ] && ( cd "$SCRIPT_DIR" && "$g" --stop ) >/dev/null 2>&1 || true
  done
fi
rm -rf "$SCRIPT_DIR"/*/build "$SCRIPT_DIR/.gradle" "$SCRIPT_DIR/dist"
ok "已停止 daemon 並清除 build/ 與 dist/"

step "2／6　搬移 Java 目錄"

# mv 在 Windows 上偶爾會被防毒或殘留 handle 擋下，所以留一條複製後刪除的退路。
move_dir() {
  local src="$1" dst="$2"
  if mv "$src" "$dst" 2>/dev/null; then return 0; fi
  cp -r "$src" "$dst" && rm -rf "$src"
}

if [ "$NEW_BASE" != "$CUR_BASE" ]; then
  for module in overlay cheatengine game sample; do
    src="$SCRIPT_DIR/$module/src/main/java/$CUR_PATH"
    [ -d "$src" ] || continue
    dst="$SCRIPT_DIR/$module/src/main/java/$NEW_PATH"
    mkdir -p "$(dirname "$dst")"
    # 新舊路徑可能有共同前綴（com.acme.a → com.acme.b），那時要逐一搬葉節點
    if [ -d "$dst" ]; then
      for leaf in "$src"/*; do
        [ -e "$leaf" ] && move_dir "$leaf" "$dst/$(basename "$leaf")"
      done
      rmdir "$src" 2>/dev/null || true
    else
      move_dir "$src" "$dst" \
        || die "搬不動 $src\n        通常是有程式抓著檔案（IDE、Gradle daemon、防毒）。關掉再試一次。"
    fi
    info "$module: $CUR_PATH → $NEW_PATH"
    # 清掉搬空之後留下的空目錄
    find "$SCRIPT_DIR/$module/src/main/java" -type d -empty -delete 2>/dev/null || true
  done
  ok "四個模組的原始碼目錄都已搬移"
else
  note "base 沒變，不需要搬目錄"
fi

step "3／6　改寫 package 名稱與 JNI 符號"

# 順序很重要：JNI 符號要在 base 被換掉之前先處理，否則
# Java_dev_marc_ce_overlay_ 這種底線形式已經對不上了。
python_rewrite() {
  python - "$@" <<'PY'
import io, sys
cur_base, new_base, cur_jni, new_jni = sys.argv[1:5]
cur_ce, new_ce, cur_game, new_game, cur_sample, new_sample = sys.argv[5:11]
files = sys.argv[11:]

pairs = [("Java_%s_" % cur_jni, "Java_%s_" % new_jni)]
if cur_base != new_base:
    pairs.append((cur_base, new_base))
for old, new in ((cur_ce, new_ce), (cur_game, new_game), (cur_sample, new_sample)):
    if new and old and old != new:
        pairs.append((old, new))

touched = 0
for path in files:
    try:
        s = io.open(path, encoding="utf-8").read()
    except (UnicodeDecodeError, OSError):
        continue
    out = s
    for old, new in pairs:
        out = out.replace(old, new)
    if out != s:
        io.open(path, "w", encoding="utf-8", newline="\n").write(out)
        touched += 1
print(touched)
PY
}

# 目錄搬過了，重新展開檔案清單
mapfile -t TEXT_FILES < <(
  {
    find "$SCRIPT_DIR" \
      \( -name build -o -name .gradle -o -name dist -o -name jniLibs \) -prune -o \
      -type f \( -name '*.java' -o -name '*.kts' -o -name '*.xml' \
                 -o -name '*.md' -o -name '*.html' -o -name '*.sh' \) \
      ! -name 'rename.sh' -print
    printf '%s\n' "$RUST_JNI"
  } | sort -u
)

TOUCHED="$(python_rewrite \
  "$CUR_BASE" "$NEW_BASE" "$CUR_JNI" "$NEW_JNI" \
  "$CUR_CE_LABEL"     "$NEW_CE_LABEL" \
  "$CUR_GAME_LABEL"   "$NEW_GAME_LABEL" \
  "$CUR_SAMPLE_LABEL" "$NEW_SAMPLE_LABEL" \
  "${TEXT_FILES[@]}")"
ok "改寫了 $TOUCHED 個檔案"

step "4／6　寫入共用身分"

# 一定要在通用替換「之後」才做，而且用逐屬性改寫而不是字串取代：
# <base>.shared 是 <base>.shared.proc 的前綴，盲目取代會把 process 名稱切壞。
set_attr() {
  local file="$1" attr="$2" value="$3"
  sed -i "s|$attr=\"[^\"]*\"|$attr=\"$value\"|g" "$file"
}
for m in "$CE_MANIFEST" "$GAME_MANIFEST"; do
  set_attr "$m" 'android:sharedUserId' "$NEW_SUID"
  set_attr "$m" 'android:process' "$NEW_PROC"
done
info "sharedUserId    $NEW_SUID"
info "android:process $NEW_PROC"
ok "兩支 manifest 的共用身分已對齊"

step "5／6　更新 pack.sh 的產出檔名"

# APK 檔名是純裝飾，但改名之後還叫 cheat-engine-v0.1.apk 就很怪。
slugify() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'     | sed 's/[^a-z0-9]\+/-/g; s/^-//; s/-$//'
}
update_apk_base() {
  local var="$1" label="$2"
  [ -n "$label" ] || return 0
  local slug; slug="$(slugify "$label")"
  [ -n "$slug" ] || return 0
  sed -i "s|^$var=\".*\"|$var=\"$slug\"|" "$SCRIPT_DIR/pack.sh"
  info "$var → $slug"
}
update_apk_base CE_APK_BASE     "$NEW_CE_LABEL"
update_apk_base GAME_APK_BASE   "$NEW_GAME_LABEL"
update_apk_base SAMPLE_APK_BASE "$NEW_SAMPLE_LABEL"
if [ -z "$NEW_CE_LABEL$NEW_GAME_LABEL$NEW_SAMPLE_LABEL" ]; then
  note "沒有改顯示名稱，檔名維持不變"
fi

step "6／6　驗證沒有殘留"

leftovers=0
for f in "${TEXT_FILES[@]}"; do
  [ -f "$f" ] || continue
  if [ "$NEW_BASE" = "$CUR_BASE" ]; then continue; fi
  if grep -qF "$CUR_BASE" "$f" 2>/dev/null; then
    warn "仍含舊名稱：${f#"$REPO_ROOT"/}"
    leftovers=$((leftovers + 1))
  fi
done
if [ "$leftovers" -eq 0 ]; then
  ok "沒有任何檔案殘留舊的 package 名稱"
else
  die "有 $leftovers 個檔案沒改乾淨，請人工檢查（檔案已經被修改，可用 git 還原）"
fi

# JNI 符號與 Java package 必須完全對上，這是最容易靜默失效的一項
jni_count="$(grep -c "Java_${NEW_JNI}_NativeBridge_" "$RUST_JNI" || true)"
old_jni_count="$(grep -c "Java_${CUR_JNI}_NativeBridge_" "$RUST_JNI" || true)"
if [ "$old_jni_count" -ne 0 ]; then
  die "src/android/mod.rs 還有 $old_jni_count 個舊的 JNI 符號"
fi
ok "$jni_count 個 JNI 符號已更新為 Java_${NEW_JNI}_NativeBridge_*"

printf '\n%s完成%s\n\n' "$BOLD" "$RESET"
info "新名稱："
info "  Cheat Engine    $NEW_BASE.app"
info "  遊戲            $NEW_BASE.game"
info "  sharedUserId    $NEW_SUID"
info "  android:process $NEW_PROC"
printf '\n'
warn "接下來："
note "  1. 裝置上的舊版必須解除安裝 —— package 名稱與 uid 都變了"
note "     adb uninstall $CUR_BASE.app ; adb uninstall $CUR_BASE.game"
note "  2. ./pack.sh    重新建置並驗證（會檢查三支簽章一致、遊戲不含引擎）"
note "  3. 要改回來就再跑一次：./rename.sh --base $CUR_BASE"
printf '\n'
