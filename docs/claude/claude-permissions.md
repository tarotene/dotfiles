# claude-permissions — `permissions.allow` を宣言的に配る

`~/.claude/settings.json` の `permissions.allow` は、Claude Code が自分の判断で
"許可プロンプトを毎回出さずに実行してよいコマンド" を宣言する場所。手で書いた
ルールは他機の同期対象にならず、`home-manager switch` のたびに検証されないので、
`registerHooks`（`docs/claude/codex-plan-review.md`）と同じ「activation 時の冪等 jq マージ」
パターンを allow にも敷いた。

配備は `home/modules/claude.nix` の `registerPermissions` / `permissionRules` /
`home.activation.registerClaudePermissions`。`registerClaudeHooks` とは別の
activation script として `writeBoundary` の後に並べており、片方のロジックが
壊れても他方に影響しない。

## ルール構文

Claude Code の permission rule は `Tool` または `Tool(specifier)` の形（例:
`Bash(git commit *)`）。裸のコマンド文字列（`git commit *`）は Bash ツールの
許可ルールとして認識されない — これは一度実際に取り違えて摩擦の原因になった。

## ルールの撤回: `retiredPermissionRules`

追加の存在判定は `.permissions.allow` に**同一文字列**が含まれているかだけ
（`registerHooks` の「command 一致だけの存在判定」と同型）。かつては「ルール
文字列を直すと旧ルールが残り続ける」が既知の制約で、手動 jq が必要だったが、
現在は削除も宣言的に行える:

- ルールの**文字列そのものを直す・撤回する**場合は、旧文字列を
  `retiredPermissionRules`（`home/modules/claude.nix`）に移す。activation が
  `--retire` パスで全ホストの `.permissions.allow` から該当文字列を削除する
  （無ければ何もしない = 冪等）
- 追加は従来どおり `permissionRules` へ（無ければ足す、あれば何もしない）
- どちらのパスも `permissions.defaultMode` や allow 以外のキーには一切触らない

最初の適用例が `Bash(git -C * add *)` / `commit` / `status` / `diff` の 4 件。
サブコマンドより前の `*` は `-c` / `--exec-path` 等のオプション挿入(= 任意
コード実行)も素通しするとして Claude Code が毎セッション警告し、しかも中間
`*` はワイルドカードとして機能せず実際にはマッチしていなかった。代替は
git-worktree-allow hook（検証つきのプログラム的許可 —
`docs/claude/git-worktree-allow.md`）。

同じ撤回パターンを `.hooks.<event>` にも敷いたのが `registerHooks` の
`retiredHookEntries`、`statusLine` にも敷いたのが `syncStatusLine` の
`retiredStatusLineCommands`(いずれも `home/modules/claude.nix`。詳細は
`docs/claude/herdr-sidebar-metadata.md`)。この文書の「削除も宣言的に行える」は
**forward switch にしか効かない**ことに注意: `home-manager switch --rollback`
は撤回機構自体を含む前の generation の activation を再実行するので、撤回リストが
まだ無い generation に戻れば「エントリだけ残る」問題が再発する
(`docs/operations.md` に孤児チェックの手順がある)。これは `retiredPermissionRules`
にも等しく当てはまる、activation を generation ごとに固定する home-manager の
モデル自体の制約であり、settings.json 側の imperative merge を採る限り避けられない。

## 何を入れているか / 入れていないか

破壊的でない読み取り・検査系と、Add / Commit / Create PR という主目的に直接効く
4 件（`git add` / `git commit` / `git push` / `gh pr create`）だけを入れている。
`gh pr edit` / `gh issue create` / `gh issue edit` のような外部への書き込みは
入れていない — `.claude/skills/aocs-draft/SKILL.md` のような各リポジトリのスキルが
明示的な人の確認を要求している操作を、allow で無言に迂回させたくないため。
