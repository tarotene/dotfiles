# guard-core — gh 投稿系 guard の共通判定エンジン(Rust)

実装: `crates/guard-core`(ライブラリ)
利用者(`gh` 投稿系の guard すべて): `crates/attribution-guard`([`attribution-guard.md`](attribution-guard.md))、
`crates/decision-colocation`([`decision-colocation.md`](decision-colocation.md))、
`crates/adr-number`([`adr-numbering.md`](adr-numbering.md))、`crates/pr-title-guard`、
`crates/pr-confirm-guard`([`pr-confirm-guard.md`](pr-confirm-guard.md))、
`crates/feedback-target-guard`、`crates/repo-create-guard`([`repo-create-guard.md`](repo-create-guard.md))
Issue: #415(ADR-0024 Stage 4a)、#391(members 分割・`--agent` で 1 バイナリ化)

bash 版 `config/claude/hooks/attribution-guard.sh` は「1 つの判定エンジン」で、
stack-base-guard.sh / pr-title-guard.sh / pr-confirm-guard.sh /
feedback-target-guard.sh / decision-colocation-guard.sh / adr-number.sh /
repo-create-guard.sh がそれを `source` し、`is_target_at` / `decide_tokens` 等を
関数の後勝ちで上書きして再利用していた。`guard-core` はそのエンジンを Rust の
純関数 + 型として切り出したもの。上書きされていた継ぎ目は「引数として渡す
関数」に、グローバル変数(`TOK` / `CMD_NOHD` / `HD_BODIES` / `TARGET_KIND` /
`F_*`)は戻り値の型になった。振る舞いは bash 版と 1 対 1(移植であって
リファクタではない)。

## 対応表: bash の関数 → Rust

| bash(attribution-guard.sh ほか) | Rust | 備考 |
|---|---|---|
| `split_heredoc` → `CMD_NOHD` / `HD_BODIES` | `shell::split_heredoc(&str) -> HeredocSplit { command, bodies }` | |
| `tokenize`(NUL 区切り出力、失敗で非 0) | `shell::tokenize(&str) -> Option<Vec<String>>` | unmatched quote・0 件で `None` |
| `is_sep` / `CMD_SEPS` | `shell::is_sep(&str) -> bool` / `shell::CMD_SEPS` | |
| `decide()` 前半(split → tokenize → `TOK`) | `command::parse(&str) -> Option<ParsedCommand { tokens, heredoc_bodies }>` | `None` = 判定不能で通す |
| `is_target_at`(上書き)+ starts/kinds ループ | `ParsedCommand::ranges(is_target_at) -> Vec<Range<K>>` | `is_target_at: FnMut(&[String], usize) -> Option<K>`。`K` が `TARGET_KIND` |
| `decide` / `decide_stack` / `decide_pr_title` / `decide_colocation` の範囲ループ | `command::first_deny(cmd, is_target_at, judge) -> Option<String>` | `judge: FnMut(&Range<K>) -> Option<String>`(最初の deny を返す) |
| `TOK[@]:s:e - s` / `s > 0`(#668 の注意書き) | `Range { kind, start, tokens, heredoc_bodies }` | |
| `adr-number.sh` の `command_ran_pr_create` | `!parse(cmd)?.ranges(f).is_empty()` | 移植済み: `crates/adr-number` |
| `base="${TOK[i]##*/}"; [[ $base == gh ]]` | `command::is_gh(&str)` | |
| `is_target_at` の定型(`gh pr create` 等) | `command::gh_command_at(tokens, i, &["pr", "create"])` | 語が足りなければ偽(bash の `((i + k < n))`) |
| `[[ ${tok[i]} == *'<<'* ]]`(`has_hd`) | `gh::has_heredoc(&[String])` | |
| `--body`/`-b`/`--body=`/`--body-file`/`-F`/`--body-file=` のループ | `gh::BodyFlags::scan(&[String]) -> BodyFlags { have_flag, texts }` | |
| `[[ -f $p && -r $p ]] && "$(cat -- "$p")"` | `gh::read_body_file(&str) -> Option<String>` | 末尾改行を落とす |
| `text="$(printf '%s\n' …)"` 以降(heredoc 追加・`$(`/`` ` `` で判定不能・空白のみで判定不能) | `gh::assemble_body_text(texts, has_hd, heredoc_bodies) -> Option<String>` | |
| pr-confirm-guard.sh `extract_body` / feedback-target-guard.sh `decide_tokens` 前半 | `gh::extract_body(tokens, heredoc_bodies) -> Option<String>` | `BodyFlags` + `assemble_body_text`(2 guard が共有) |
| `parse_pr_title_tokens` / `parse_tokens`(`--title`/`--base`/`--repo` 等) | `gh::scan_value_flags(tokens, &[ValueFlag { names, eq_prefix }]) -> Vec<FlagValue { present, value }>` | 1 パス・後勝ち。下記の注意 |
| `-X` / `--method` / `--method=` | `gh::api_method(&[String]) -> Option<String>` | 大文字化は呼び出し側(`${method^^}`) |
| `t="/${tok[i]#/}"; [[ $t =~ $RE ]]` | `gh::api_path(tokens, is_judged: FnMut(&str) -> bool) -> Option<String>` | |
| `-f`/`--field`/`-F`/`--raw-field` の `body=` / `--input` | `gh::ApiBody::scan(&[String]) -> ApiBody { have_flag, texts }` | `--input <file>` は jq の `.body // empty` と同じ規則 |
| `grep -qE "No-Attribution:[[:space:]]*[^[:space:]'\"\`)]"` / `INDEP_RE` | `marker::has_reasoned_tag(text, "No-Attribution:")` | 行単位(改行をまたがない) |
| `ATTRIBUTION_RE` | `marker::has_lead_then_name(text, leads, name)` | |
| `owner_repo`(git remote -v + awk + sed) | `repo::owner_repo(&Path) -> Option<String>` | 純関数部は `repo::parse_owner_repo_url` / `repo::remote_v_github_url` |
| `default_branch`(origin/HEAD → `gh repo view`) | `repo::default_branch_or_gh(project, nwo) -> Option<String>` | ローカルだけなら `hook_io::git::default_branch` |
| `main()` の `jq -r '.tool_name // empty'` / `.tool_input.command` | `hook::ToolCall::parse(agent, stdin)` / `.is_bash()` / `.bash_command()` | Copilot は `.toolName == "bash"` / `.toolArgs` |
| `decide_mcp` の `.tool_input.body // .tool_input.comment` | `ToolCall::tool_input_alt(&["body", "comment"])` | |
| 任意の `jq -r '.a.b // empty'` | `hook::jq_r_path(&Value, &["a", "b"]) -> Result<Option<String>, JqError>` | jq がエラーになる形は `Err` |
| `emit_deny`(`jq -n` の整形)/ Copilot の `emit_deny_copilot` | `hook::deny_output(agent, reason) -> String` | 末尾改行込み、jq と同じバイト列 |
| adapter の `--agent` 相当 | `hook::agent_from_args(&[String]) -> hook_io::Agent` | 既定は Claude |
| `has_marker` / `deny_reason` / `decide_tokens` / `decide_api_tokens` / `decide` / `decide_mcp`(attribution 固有) | `attribution_guard::Attribution` のメソッド | guard-core ではなく `crates/attribution-guard` |
| `is_target_at`(attribution 版: cli/api) | `attribution_guard::is_target_at` / `TargetKind` | `repo_create_guard::is_target_at` が同じ型で api を分ける |

## 後続の移植の型

```rust
use guard_core::{command, gh, hook};

fn pr_create_or_edit(t: &[String], i: usize) -> Option<Kind> {
    if command::gh_command_at(t, i, &["pr", "create"]) { Some(Kind::Create) }
    else if command::gh_command_at(t, i, &["pr", "edit"]) { Some(Kind::Edit) }
    else { None }
}

let reason = command::first_deny(cmd, pr_create_or_edit, |r| {
    let f = gh::scan_value_flags(r.tokens, &[
        gh::ValueFlag { names: &["--base", "-B"], eq_prefix: "--base=" },
        gh::ValueFlag { names: &["--repo", "-R"], eq_prefix: "--repo=" },
    ]);
    judge(r.kind, &f, gh::extract_body(r.tokens, r.heredoc_bodies))
});
if let Some(r) = reason { print!("{}", hook::deny_output(agent, &r)); }
```

- **値フラグは 1 パスで読む。** bash の各 guard は 1 つの `case` ループで全フラグを
  見ており、値を取るフラグだけが次のトークンを消費する。フラグごとに独立に走査すると
  `--title -R` の `-R` を `--repo` と誤読するなど bash と結果が変わる。
  stack-base-guard.sh の `parse_pr_tokens`(本文・値フラグ・読み飛ばしリスト・
  位置引数 `F_TARGET` を 1 ループで扱う)は `scan_value_flags` に収まらないので、
  `BodyFlags` / `read_body_file` を部品にして自前のループで移すこと。
- **オラクル**: `crates/attribution-guard/tests/cmd/` の 64 件(bash 版から生成)が、
  heredoc・コマンド位置・本文抽出・`gh api` の境界を固定している。
- 依存は `hook-io` と `serde_json` だけ(起動時間を損なわない、regex 無し)。

## bash の挙動が疑わしい箇所(移植では保った)

- `split_heredoc` は `<<<`(herestring)も heredoc として扱う(`<<<EOF` /
  `<<< "x"` が 2 文字目からの `<<` で一致する)。bash 版のコメントは「除外される」
  と書いていた。`cat <<<EOF` の次行以降が本体扱いになり、そこにある投稿コマンドが
  コマンド位置から外れる(false pass 方向)。
- bash 版冒頭のコメントは unmatched quote と heredoc を「範囲文字列全体をフォール
  バックで検査」と書くが、実装はそうなっていない(unmatched quote は素通し、
  heredoc は本体を本文候補に足すだけ)。
- `tokenize` はクォートの痕跡を残さないので、`';'` のようにクォートされた区切り
  文字も区切りトークン扱いになり、直後がコマンド位置になる。
