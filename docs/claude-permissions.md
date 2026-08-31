# claude-permissions — `permissions.allow` を宣言的に配る

`~/.claude/settings.json` の `permissions.allow` は、Claude Code が自分の判断で
"許可プロンプトを毎回出さずに実行してよいコマンド" を宣言する場所。手で書いた
ルールは他機の同期対象にならず、`home-manager switch` のたびに検証されないので、
`registerHooks`（`docs/codex-plan-review.md`）と同じ「activation 時の冪等 jq マージ」
パターンを allow にも敷いた。

配備は `home/modules/claude.nix` の `registerPermissions` / `permissionRules` /
`home.activation.registerClaudePermissions`。`registerClaudeHooks` とは別の
activation script として `writeBoundary` の後に並べており、片方のロジックが
壊れても他方に影響しない。

## ルール構文

Claude Code の permission rule は `Tool` または `Tool(specifier)` の形（例:
`Bash(git commit *)`）。裸のコマンド文字列（`git commit *`）は Bash ツールの
許可ルールとして認識されない — これは一度実際に取り違えて摩擦の原因になった。

## 既知の制約: ルール文字列を変えると旧ルールは残る

`registerPermissions` の存在判定は `.permissions.allow` に**同一文字列**が
含まれているかだけ（`registerHooks` の「command 一致だけの存在判定」と同型）。
つまり:

- ルールを追加するのは安全（無ければ足す、あれば何もしない）
- ルールの**文字列そのものを直す**場合（例: `Bash(git commit *)` を
  `Bash(git commit -s *)` に絞り直す）は、旧ルールが `.permissions.allow` に
  残り続ける。手で `jq 'del(.permissions.allow[] | select(. == "..."))'` するか、
  該当行を `~/.claude/settings.json` から直接消してから `home-manager switch`
  すること
- `permissions.allow` 配下でも `permissions.defaultMode` や他のキーには一切触らない

## 何を入れているか / 入れていないか

破壊的でない読み取り・検査系と、Add / Commit / Create PR という主目的に直接効く
4 件（`git add` / `git commit` / `git push` / `gh pr create`）だけを入れている。
`gh pr edit` / `gh issue create` / `gh issue edit` のような外部への書き込みは
入れていない — `.claude/skills/aocs-draft/SKILL.md` のような各リポジトリのスキルが
明示的な人の確認を要求している操作を、allow で無言に迂回させたくないため。
