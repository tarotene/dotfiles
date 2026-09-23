# gh-edit-allow — 自セッションが作成した PR/Issue の編集を検証付きで allow する

`crates/gh-edit-allow`(Rust、ADR-0024)の設計根拠(#392)。

## 動機

`permissions.allow` には `Bash(gh pr create *)` が入っている一方で、
`gh pr edit` / `gh issue edit` / `gh issue create` は入っていなかった。
そのため、PR を出すのはプロンプト無しなのに、出した PR のタイトルや本文を
直すたびに確認プロンプトが出る、という非対称があった。

以前は `home/modules/claude.nix` のコメントが、書き込み系を入れない理由として
`aocs-draft` スキルを挙げていた。しかしこのスキルは本リポジトリには無い。
`claude-permissions.md` が別リポジトリの例として引いていたものだった。

ワイルドカード(`Bash(gh pr edit *)`)は足さない。これを足すと「他人の PR も
編集できる」ことまで許すことになるためである。

## 仕組み

1 バイナリで 2 役を持ち、`hook_event_name` で分岐する。

| イベント | 役 | 動作 |
|---|---|---|
| PostToolUse(`Bash`、`if` 無し) | 記録 | `gh pr create` / `gh issue create` の `tool_response.stdout` に出た `https://github.com/<o>/<r>/(pull\|issues)/<N>` を台帳に積む |
| PreToolUse(`Bash`、`if: Bash(gh *)`) | 判定 | 対象が台帳にあれば `permissionDecision: allow`。無ければ何も出さない |

- **台帳**: `~/.claude/gh-edit-allow/state/<session_id>.ledger` に、1 行 1 件で
  `pr owner/repo N` の形式で書く。実装は `hook-io::SessionLedger`
  (`stack-base-guard.sh` の `state_file()` と同型)。台帳はセッションごとに空から始まる。
- **判定の対象**:
  - `gh pr edit <N|URL>` / `gh issue edit <N|URL>...`: 対象がすべて台帳にあれば allow する。
    番号で指定された場合、リポジトリは `-R` の値、無ければ cwd の
    `remote.origin.url`(ローカルの git config)で決める。
  - `gh issue create`: 同じリポジトリに対して、このセッションが既に PR/Issue を
    作っていれば allow する。そのリポジトリで作者として動いていることの証跡として扱う。
- **判定しない(素通しする)入力**:
  - 複合コマンド・展開・リダイレクト・glob(`shell.rs`)。クォート内の
    `|` や `>` は本文によく出るので許すが、`$` はダブルクォート内でも展開される
    ため拒否する。
  - 未知のフラグ。値を取るかどうか分からないと、位置引数を取り違えるため。
  - ブランチ名での指定、引数無し(現ブランチの PR を指す)。これらは
    ネットワーク無しでは解決できない。
- **ネットワーク照会はしない**(`gh pr view --json author` 等)。台帳に無い番号は
  編集対象として表現できないので、往復ゼロで「他人の PR は対象外」が成り立つ。
- **skip**: `SKIP_GH_EDIT_ALLOW=1` を設定するか、`~/.claude/gh-edit-allow/skip`
  を置くと hook 全体が止まる(`stack-base-guard` と同じ形)。
  別リポジトリのスキルが人の確認を明示的に要求している作業では、これで止める。

## 先行例との差分

- **`git-worktree-allow`**(`docs/claude/git-worktree-allow.md`): 「検証付きの
  allow。不一致は deny ではなく素通し」という型をそのまま踏襲した。
- **`stack-base-guard`**: PreToolUse の時点で head ブランチを楽観的に記録するため、
  失敗した create も積んでしまう。一方、番号は作成が成功するまで存在しない。
  そこで本 hook は PostToolUse で `tool_response` を読む。この
  リポジトリで `tool_response` を読む最初の hook になる(`adr-number.sh:20-24` の
  「先行例なし」を参照)。
- **積極化ポリシー**: 実体はこの hook だけにし、AGENTS.md に規範文は足さない
  (#392)。ADR-0012 は規範をゲートに置き換えたが、本件は摩擦を取り除くことが
  目的なので、規範文を増やさないという仮説を採る。

## 限界

- 前のセッションで作った自分の PR/Issue は対象にならない(台帳がセッション単位のため)。
  その場合は従来どおり確認プロンプトを経由する。
- MCP の GitHub ツール経由の作成・編集は見ない(`gh` の Bash 呼び出しだけが対象)。
- GitHub Enterprise のホスト(`-R ghe.example.com/o/r`)は対象外。

## 検証

`cargo test -p gh-edit-allow` で次を検証する。

- 単体テスト: 語分割、gh の引数解析、記録と判定。
- `tests/cmd/*.toml`(trycmd): 実バイナリに stdin JSON を与え、台帳ディレクトリの
  before/after を比べる。
