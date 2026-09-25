# claude-permissions — `permissions.allow` を宣言的に配る

`~/.claude/settings.json` の `permissions.allow` は、Claude Code が自分の判断で
"許可プロンプトを毎回出さずに実行してよいコマンド" を宣言する場所。手で書いた
ルールは他機の同期対象にならず、`home-manager switch` のたびに検証されないので、
`registerHooks`（`docs/claude/copilot-plan-review.md`）と同じ「activation 時の冪等 jq マージ」
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
コード実行)も素通しするとして Claude Code が毎セッション警告する。代替は
git-worktree-allow hook(検証つきのプログラム的許可 —
`docs/claude/git-worktree-allow.md`)。

同じ経路は宣言由来のルールに限らない。`Bash(ps -p * -o pid,cmd)` は実行時の
許可プロンプトで個別ホストの settings.json に直接足された野良ルールだったが、
中間 `*` 警告は同様に発生し、撤回リストへ加えるだけで全ホストから消せた。

`Bash(npx --prefix * playwright *)` も同型の野良ルールだったが、こちらは
`config/claude/commands/promote-permissions.md` の generic 判定パターンにも
「昇格すべき」として登録されていた。撤回リストだけでは `/promote-permissions`
実行のたびに再び足されるため、昇格パターン側も同じ PR で削除した。

これらを個別に3回撤回した(#453/#460/#461)末に判明したのは、「中間 `*`
はワイルドカードとして機能せず実際にはマッチしていなかった」という、この
文書がかつて書いていた理解が誤りだったこと。Claude Code の Wildcard
patterns(<https://code.claude.com/docs/en/permissions>、取得 2026-09-25)は
「`*` はルール中どこにでも置け、`Bash(git * main)` は `git merge main` にも
`git -c core.fsmonitor=<script> diff main` にも実際にマッチする」と明記して
いる — 起動時の警告は事後の検出でしかなく、ルール自体は適用されたままだった。

#461 で個別撤回をやめ、機構に還元した。中間 `*`(`(` か空白の直後の `*` に、
空白を挟んで `)` 以外の文字が続くパターン)は次の 3 層で塞ぐ:

- **宣言リスト**(`permissionRules`): `home/modules/claude.nix` の
  `hasMidWildcard` で `nix flake check`/`nix build` の eval 時に
  `assert` する。中間 `*` を含むルールはそもそも書けない(表現不可能)。
- **settings.json の実体**: `registerPermissions` の jq が
  `.permissions.allow` から中間 `*` を含むルールを一律 strip する
  (Claude Code 自身の「常に許可」プロンプトが書く分も含む — こちらは
  検出が上限で、宣言側のような表現不可能化はできない)。
- **`/promote-permissions`**: `isGenericPermission` の先頭で中間 `*` を
  即 `false` にする(以降のどの固定プレフィックス一致パターンにも
  昇格させない)。

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
ワイルドカードでは入れていない。`Bash(gh pr edit *)` と書くと、他人の PR の編集まで
許すことを表してしまうためである。

これらは代わりに gh-edit-allow hook(`docs/claude/gh-edit-allow.md`、#392)が
扱う。allow するのは、このセッション自身が作成した PR/Issue(と、そのリポジトリへの
`gh issue create`)に限る。それ以外は通常の確認フローに残す。
`.claude/skills/aocs-draft/SKILL.md` のように、別リポジトリのスキルが人の確認を
明示的に要求している場合は、その hook の skip 機構(同文書)で止められる。
