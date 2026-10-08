#!/usr/bin/env bash
# create-repo.sh — repo-charter §8 を 1 本にまとめた、新規リポジトリ作成の入口。
#
# 閉語彙チェック(§1)→ `gh repo create` → `gh repo edit --add-topic` →
# 型別の governance 播種。素の `gh repo create` は repo-create-guard が deny する
# (docs/claude/repo-create-guard.md)。guard が強制したいのは「手順に載せること」
# なので、手順そのものを実行するこのスクリプトを正規の入口にする。guard は Bash の
# コマンド文字列しか見ないため、このスクリプト内の `gh repo create` は deny に
# 当たらず、バイパス用の環境変数は要らない(#754。rulesets-write-guard に対する
# apply-rulesets と同じ形、#707)。
set -euo pipefail

SELF="$(realpath "${BASH_SOURCE[0]}")"
GH="${CREATE_REPO_GH_BIN:-gh}"
SKILLS_DIR="${CREATE_REPO_SKILLS_DIR:-$HOME/.claude/skills}"
AUDIT_DIR="${GITHUB_AUDIT_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/github-audit}"

usage() {
  cat <<'EOF'
usage: create-repo.sh --owner OWNER --repo REPO --description TEXT --class naming-CLASS
                      --topic TOPIC [--topic TOPIC ...] [--lifecycle lifecycle-CLASS]
                      [--public] [--type rust|typst|astro|core --dest PATH]
                      [--dry-run] [-- <seed.sh に渡すオプション>]
       create-repo.sh --selftest

  --description   charter の目的 1 文(≤120 字)。README の H1 直後の段落と一字一句同じにする
  --class         naming-codename | naming-coined | naming-descriptive | naming-pj | naming-site
  --topic         技術領域・ドメインを表す topic(最低 1 つ)
  --lifecycle     lifecycle-timeboxed | lifecycle-study(任意、0〜1 個)
  --type          governance の型。rust/typst/astro は各 seed.sh、core は apply-repo-settings.sh
                  + copy-files.sh。--type を付けたら --dest(ローカルの checkout)も要る
  --public        公開リポジトリにする(既定は --private)
  --dry-run       gh を呼ばず、検査の結果と、走らせるコマンドを表示する
EOF
}

die() {
  echo "create-repo.sh: $*" >&2
  exit 2
}

# ---- 閉語彙チェック(repo-charter §1、ADR-0020 / ADR-0026) ---------------

# TSV(`#` 始まりと空行を除く)の第 1 列に $1 があるか。ファイルが無ければ偽。
tsv_has() {
  local key="$1" file="$2"
  [ -f "$file" ] || return 1
  awk -F'\t' -v k="$key" '!/^#/ && NF && $1 == k { found = 1 } END { exit !found }' "$file"
}

vocab_check() {
  local repo="$1" class="$2"
  case "$class" in
    naming-pj)
      [[ "$repo" =~ ^pj-[a-z0-9]+(-[a-z0-9]+)*$ ]] \
        || die "naming-pj は pj-<小文字・数字>(-…)* の形だけです: $repo"
      ;;
    naming-coined)
      [[ "$repo" =~ ^[a-z0-9]+$ ]] || die "naming-coined は小文字・数字の 1 トークンです: $repo"
      ;;
    naming-codename)
      [[ "$repo" =~ ^[a-z0-9]+$ ]] || die "naming-codename は小文字・数字の 1 トークンです: $repo"
      tsv_has "$repo" "$AUDIT_DIR/codename-registry.tsv" \
        || tsv_has "$repo" "$AUDIT_DIR/codename-registry.local.tsv" \
        || die "codename '$repo' がレジストリに未登録です。先にレジストリへ追記してください(PUBLIC は dotfiles への PR、PRIVATE は codename-registry.local.tsv)"
      ;;
    naming-descriptive)
      [[ "$repo" =~ ^[a-z0-9]+(-[a-z0-9]+)+$ ]] \
        || die "naming-descriptive は <対象>-<種別語> の形です: $repo"
      tsv_has "${repo##*-}" "$AUDIT_DIR/descriptive-species.tsv" \
        || die "種別語 '${repo##*-}' が descriptive-species.tsv にありません。完了しうる行為を表す語なら naming-pj へ、恒久的な器の語なら先にそのファイルの改訂 PR を立ててください"
      ;;
    naming-site)
      [[ "$repo" =~ ^[a-z0-9]+(\.[a-z0-9]+)+$ ]] || die "naming-site は FQDN の形です: $repo"
      tsv_has "$repo" "$AUDIT_DIR/site-domains.tsv" \
        || tsv_has "$repo" "$AUDIT_DIR/site-domains.local.tsv" \
        || die "ドメイン '$repo' が site-domains に未登録です"
      ;;
    *) die "--class は naming-codename|naming-coined|naming-descriptive|naming-pj|naming-site のどれかです: $class" ;;
  esac
}

# ---- 実行 -------------------------------------------------------------------

DRY=false
run() {
  if $DRY; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

main() {
  local owner="" repo="" desc="" class="" lifecycle="" type="" dest="" visibility="--private"
  local -a topics=() passthrough=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --owner) owner="${2:?}"; shift 2 ;;
      --repo) repo="${2:?}"; shift 2 ;;
      --description) desc="${2:?}"; shift 2 ;;
      --class) class="${2:?}"; shift 2 ;;
      --topic) topics+=("${2:?}"); shift 2 ;;
      --lifecycle) lifecycle="${2:?}"; shift 2 ;;
      --type) type="${2:?}"; shift 2 ;;
      --dest) dest="${2:?}"; shift 2 ;;
      --public) visibility="--public"; shift ;;
      --dry-run) DRY=true; shift ;;
      -h | --help) usage; exit 0 ;;
      --) shift; passthrough=("$@"); break ;;
      *) usage >&2; die "不明な引数: $1" ;;
    esac
  done

  [ -n "$owner" ] && [ -n "$repo" ] && [ -n "$desc" ] && [ -n "$class" ] \
    || { usage >&2; die "--owner / --repo / --description / --class は必須です"; }
  [ "${#desc}" -le 120 ] || die "--description は 120 字以内です(${#desc} 字)"
  [ "${#topics[@]}" -ge 1 ] || die "--topic が最低 1 つ要ります(技術領域・ドメイン)"
  case "$lifecycle" in
    "" | lifecycle-timeboxed | lifecycle-study) ;;
    *) die "--lifecycle は lifecycle-timeboxed | lifecycle-study のどちらかです: $lifecycle" ;;
  esac
  case "$type" in
    "" | rust | typst | astro | core) ;;
    *) die "--type は rust | typst | astro | core のどれかです: $type" ;;
  esac
  { [ -z "$type" ] || [ -n "$dest" ]; } || die "--type を付けるときは --dest(ローカルの checkout)も要ります"

  vocab_check "$repo" "$class"
  echo "ok: 閉語彙チェック($class: $repo)"

  local slug="$owner/$repo"
  if ! $DRY && "$GH" repo view "$slug" > /dev/null 2>&1; then
    echo "skip: $slug は既に存在します(作成を飛ばして続きから進めます)"
  else
    run "$GH" repo create "$slug" "$visibility" --description "$desc"
  fi

  local -a edit=("$GH" repo edit "$slug" --add-topic "$class")
  [ -z "$lifecycle" ] || edit+=(--add-topic "$lifecycle")
  local t
  for t in "${topics[@]}"; do edit+=(--add-topic "$t"); done
  run "${edit[@]}"

  case "$type" in
    "") echo "note: --type 無し — governance の播種は行いません(repo-charter §8 を参照)" ;;
    rust | typst | astro)
      local skill="$type-repo-governance"
      [ "$type" != astro ] || skill="astro-site-governance"
      run "$SKILLS_DIR/$skill/scripts/seed.sh" --owner "$owner" --repo "$repo" --dest "$dest" "${passthrough[@]}"
      ;;
    core)
      run "$SKILLS_DIR/repo-governance-common/scripts/apply-repo-settings.sh" --owner "$owner" --repo "$repo"
      run "$SKILLS_DIR/repo-governance-common/scripts/copy-files.sh" --owner "$owner" --repo "$repo" --dest "$dest"
      echo "next: $dest で commit + push してから、"
      echo "      apply-rulesets.sh $slug --from-dir $dest/.github/rulesets --unverified-contexts"
      ;;
  esac
  echo "next: 'github-audit charters naming' が $slug で ok になることを確かめる(repo-charter §9)"
}

# ---- --selftest -------------------------------------------------------------

selftest() {
  local fails=0 tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  mkdir -p "$tmp/audit" "$tmp/skills/repo-governance-common/scripts" "$tmp/skills/rust-repo-governance/scripts"
  printf '# c\nacme\tdesc\t2026-01-01\n' > "$tmp/audit/codename-registry.tsv"
  printf 'priv\tdesc\t2026-01-01\n' > "$tmp/audit/codename-registry.local.tsv"
  printf '# s\narchive\t器\ncards\t集合\n' > "$tmp/audit/descriptive-species.tsv"
  printf 'example.org\tx\n' > "$tmp/audit/site-domains.tsv"
  cat > "$tmp/gh" <<'STUB'
#!/usr/bin/env bash
printf 'gh %s\n' "$*" >> "$STUB_LOG"
[ "$1 $2" = "repo view" ] && exit "${STUB_VIEW_RC:-1}"
exit 0
STUB
  local s
  for s in repo-governance-common/scripts/apply-repo-settings.sh repo-governance-common/scripts/copy-files.sh rust-repo-governance/scripts/seed.sh; do
    printf '#!/usr/bin/env bash\nprintf "%s %%s\\n" "$*" >> "$STUB_LOG"\n' "$(basename "$s")" > "$tmp/skills/$s"
  done
  chmod +x "$tmp/gh" "$tmp"/skills/*/scripts/*.sh

  # case <name> <want-rc> <log-pattern | !log-pattern | -> <args…>
  case_run() {
    local name="$1" want_rc="$2" want_log="$3"
    shift 3
    : > "$tmp/log"
    local rc=0
    STUB_LOG="$tmp/log" CREATE_REPO_GH_BIN="$tmp/gh" CREATE_REPO_SKILLS_DIR="$tmp/skills" \
      GITHUB_AUDIT_CONFIG_DIR="$tmp/audit" bash "$SELF" "$@" > "$tmp/out" 2>&1 || rc=$?
    local ok=true
    [ "$rc" = "$want_rc" ] || ok=false
    case "$want_log" in
      -) ;;
      '!'*) if grep -q -- "${want_log#!}" "$tmp/log"; then ok=false; fi ;;
      *) grep -q -- "$want_log" "$tmp/log" || ok=false ;;
    esac
    if $ok; then
      echo "ok   $name"
    else
      echo "FAIL $name (rc=$rc)"
      sed 's/^/     /' "$tmp/out"
      fails=$((fails + 1))
    fi
  }

  local base=(--owner o --description "目的" --topic cli)
  case_run "codename (PUBLIC registry)" 0 "gh repo create o/acme" --repo acme --class naming-codename "${base[@]}"
  case_run "codename (PRIVATE local registry)" 0 "gh repo create o/priv" --repo priv --class naming-codename "${base[@]}"
  case_run "unregistered codename: no gh call" 2 "!gh repo" --repo zzz --class naming-codename "${base[@]}"
  case_run "descriptive: species ok" 0 "gh repo create o/foo-cards" --repo foo-cards --class naming-descriptive "${base[@]}"
  case_run "descriptive: unknown species refused" 2 "!gh repo" --repo foo-cleanup --class naming-descriptive "${base[@]}"
  case_run "pj ok" 0 "gh repo create o/pj-x" --repo pj-x --class naming-pj "${base[@]}"
  case_run "pj without prefix refused" 2 "!gh repo" --repo x --class naming-pj "${base[@]}"
  case_run "site registered" 0 "gh repo create o/example.org" --repo example.org --class naming-site "${base[@]}"
  case_run "topic is required" 2 "!gh repo" --repo acme --class naming-codename --owner o --description d
  case_run "lifecycle topic is added" 0 "add-topic lifecycle-study" --repo acme --class naming-codename --lifecycle lifecycle-study "${base[@]}"
  case_run "core: settings then files" 0 "apply-repo-settings.sh --owner o --repo acme" --repo acme --class naming-codename --type core --dest "$tmp" "${base[@]}"
  case_run "rust: seed.sh gets passthrough" 0 "seed.sh --owner o --repo acme --dest $tmp --canonical-crate c" \
    --repo acme --class naming-codename --type rust --dest "$tmp" "${base[@]}" -- --canonical-crate c
  case_run "--type needs --dest" 2 "!gh repo" --repo acme --class naming-codename --type core "${base[@]}"
  case_run "dry-run calls no gh" 0 "!gh repo" --repo acme --class naming-codename --dry-run "${base[@]}"

  # 既存リポジトリ(gh repo view が成功)では作成を飛ばし、topic の反映へ進む
  : > "$tmp/log"
  STUB_VIEW_RC=0 STUB_LOG="$tmp/log" CREATE_REPO_GH_BIN="$tmp/gh" CREATE_REPO_SKILLS_DIR="$tmp/skills" \
    GITHUB_AUDIT_CONFIG_DIR="$tmp/audit" bash "$SELF" --repo acme --class naming-codename "${base[@]}" > /dev/null 2>&1 || true
  if ! grep -q "gh repo create" "$tmp/log" && grep -q "gh repo edit o/acme" "$tmp/log"; then
    echo "ok   existing repo: create skipped, topics still applied"
  else
    echo "FAIL existing repo: create skipped, topics still applied"
    fails=$((fails + 1))
  fi

  if [ "$fails" -eq 0 ]; then
    echo "selftest: all ok"
  else
    echo "selftest: $fails failed"
    return 1
  fi
}

case "${1:-}" in
  --selftest) selftest ;;
  *) main "$@" ;;
esac
