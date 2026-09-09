# git-worktree-allow — `git -C <worktree>` を検証つきで許可する PreToolUse hook

herdr worktree を**外から**駆動する `git -C <worktree> <サブコマンド>` を、
permission rule ではなく PreToolUse hook でプログラム的に許可する。
実装は `config/claude/hooks/git-worktree-allow.sh`(配備先 `~/.claude/hooks/`)、
登録は `home/modules/claude.nix` の `registerHooks`。

## なぜルールではなく hook か

かつては `Bash(git -C * add *)` / `commit` / `status` / `diff` の 4 ルールを
`permissions.allow` に配っていた。これには 2 つの問題があった:

1. **危険**: サブコマンドより前の `*` は、その位置に挿し込まれた任意のオプションも
   マッチさせる。git の `-c` や `--exec-path` は任意コード実行に繋がるため、
   Claude Code は起動のたびに全ルールへ警告を出す。
2. **無効**: Claude Code のルールマッチは末尾 `*` の前方一致だけで、中間 `*` は
   ワイルドカードとして機能しない。つまりこの 4 ルールは実際には何もマッチして
   いなかった(警告だけが残った)。

permission rule の構文では「任意の worktree パス + 特定サブコマンド」は安全に
表現できない。一方 PreToolUse hook は生のコマンド文字列を検証してから
`permissionDecision: "allow"` を返せる(公式仕様:
<https://code.claude.com/docs/en/hooks-guide.md>)。非該当なら無出力で exit 0 し、
通常の permission フロー(プロンプト)にフォールスルーする。hook の allow は
deny ルールを上書きしない(最も制限的な決定が勝つ)ので、安全側に倒れている。

旧 4 ルールは `retiredPermissionRules`(`home/modules/claude.nix`)に移してあり、
各ホストの次回 switch で `~/.claude/settings.json` から自動削除される
(`docs/claude/claude-permissions.md` の retirement 節)。

## 許可条件

以下を**すべて**満たすときだけ allow。ひとつでも欠ければ無言でフォールスルー:

1. **単一の git 呼び出し**であること — `;` `&` `|` `$(` バッククォート・
   リダイレクト・改行を含む複合コマンドは即座に対象外。hook は文字列全体を
   1 回だけ見るので、複合コマンドを許可すると別コマンドの同乗を許してしまう。
2. 形が `git -C <dir> <サブコマンド> …` に**厳密一致** — `git` の直後のトークンが
   `-C` そのもの。`-c` などのグローバルオプションを挟む形は不可。
3. `<dir>` が `-` 始まりでない**実在ディレクトリ**で、realpath(symlink 解決後)が
   `~/.herdr/worktrees/` 配下。root そのものは不可、symlink による越境も
   解決後判定で弾かれる。
4. `<サブコマンド>` が許可リスト内: `status` `diff` `log` `show` `add` `commit`
   `push`。素の `git …` ルール群(`permissionRules`)と同じ守備範囲に、worktree
   から PR を push する動線(`push`)を加えた集合。
5. 残りの引数に git を別の実行体へ向けられるものがない: `--receive-pack` /
   `--upload-pack` / `--exec-path` / `ext::` transport。これは防壁ではなく
   belt-and-suspenders — 脅威モデルは敵対的入力ではなく Claude 自身の生成コマンド
   であり、素の `Bash(git push *)` ルールと同等以上の水準を保つための措置。

トークン分割は素朴な空白分割。クォートを含むトークン(パスに空白がある等)は
実在チェックや許可リスト照合に自然に落ちるので、誤許可側には倒れない —
その場合は通常どおりプロンプトが出るだけ。

## 縮退と検査

- jq 不在・stdin 不正は黙って exit 0(ADR-0005 の binary-existence gating に倣う)。
- `git-worktree-allow.sh --selftest` が許可・拒否の全境界(オプション注入・
  複合コマンド・範囲外パス・symlink 越境・範囲外サブコマンド)を回帰テストする。
  CI の shellcheck 対象。

## 登録形

`registerHooks` の `register()` に省略可能な第 5 引数 `if` を追加し、
`PreToolUse` / matcher `Bash` / `"if": "Bash(git -C *)"` で登録する。`if` は
ハンドラレベルの絞り込み(正式仕様)で、`git -C` 以外の Bash 呼び出しでは
hook プロセス自体が spawn されない。
