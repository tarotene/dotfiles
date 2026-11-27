# Claude・Codex・Antigravity はそれぞれの native 権限設定を使う

共有権限ポリシーの実行時管理は撤去した。Claude は Claude 専用 reconciler、
Codex は native 設定を使う。Antigravity CLI も専用のサブコマンドで native 設定へ配備する。所有記録がないことや、設定を本人が変更したことを
理由に router が通常のツール実行を止める仕組みはない。裁定は
[ADR-457 の 2026-10-27 Amendment](../adr/457-claude-native-install-source-of-truth.md) に残す。

## 設定の正本と適用後の比較対象

| 対象 | 宣言・執行点 | 適用する内容 |
|---|---|---|
| Claude | `home/modules/claude.nix`、`agent-settings-reconcile claude-permissions` | native の allow / ask / deny と撤回リスト、`sandbox`・`autoMode.classifyAllShell`([層構造](#claude-と-codex-の層構造を揃える)) |
| Codex | `home/modules/codex.nix`、`agent-settings-reconcile codex-permissions` | `on-request`、`auto_review`、名前付き `dotfiles` 権限 profile、通信 proxy、秘密パスの masking、個別 MCP 設定 |
| Antigravity CLI(`agy`) | `home/modules/antigravity.nix`、`agent-settings-reconcile antigravity-permissions` | `permissions.{allow,ask,deny}`、`toolPermission`、`enableTerminalSandbox`([宣言の仕様](#antigravity-cli-の宣言)) |

Claude の入力は `dotfiles.claudePermissions.{extraAllow,secretPaths,sandboxDomains,excludedCommands}`、Codex の入力は
`dotfiles.codexPermissions.secretPaths` / `domains` / `mcpAllow` / `apps` に置く。
非公開の実値は ADR-0034 に従い private wrapper に置く。
既存 3 ホストとの互換性のため `dotfiles.agentPermissions` の option は残るが、
旧共有 policy ファイル・所有検査・SessionStart の回収は配備しない。
認証 launcher は維持し、資格情報を渡す前に native 設定を検査する。共有の所有記録は参照しない。
旧入力は native option の既定値に使い、以後の宣言は native option に書く。

生成した比較対象は `~/.config/agent-native/claude-permissions.json` と
`~/.config/agent-native/codex-permissions.json`。前者は Claude reconciler の spec、
後者は `settings` と `mcp_allow` を持つ Codex の spec である。
両 CLI の設定全体を symlink にしないため、モデル・trust などの本人の設定は書き込み可能なまま残る。
Claude の `permissions.defaultMode = "plan"` と `autoMode.environment` は撤去時にも保持する。
`autoMode.allow`(#939)は `auto_mode_allow` の宣言があるときだけ書く。先頭に `"$defaults"` を置き、本人が追加した要素は残して
宣言との和集合にする。自分の worktree での `git switch -c` が `[Modify Shared Resources]` で拒否された(2026-10-22 の
1 回)のは、`classifyAllShell` が Bash の allow(`Bash(git switch *)`)を停止し、分類器が `.git` の保護パスを判定したため
(Anthropic, "Configure auto mode"、<https://code.claude.com/docs/en/auto-mode-config>、取得 2026-10-28。#939 のコメントが逐語で引用)。
分類器は `~/.claude/settings.json` の `autoMode` だけを読むため、この宣言は `agent-settings-reconcile claude-permissions` が
そこへ書き込む。実セッションで拒否が出なくなるかは、適用後の観測で確かめる(この PR では未確認)。

Codex の native writer は宣言した名前の profile だけを置き換え、古い通信先や書き込み許可を残さない。
ほかの profile・モデル・trust は保持する。`default_permissions` を宣言するときは競合する旧
`sandbox_mode` / `sandbox_workspace_write` を取り除く。これらの置換前には同じディレクトリに
権限 0600 の `config.toml.native-permissions-backup.<内容のSHA-256>` を作成し、現在値の一致を確認してから更新する。

宣言から外した許可(`mcp_allow` から外した MCP 許可、Claude の `allow` から外した規則)は、
hook と同じ「所有の記録」(`agent-settings-reconcile` の `managed`、#1161)で削除する。
前回の適用で書いた集合をホストごとのファイル(`~/.local/state/dotfiles/managed-allow-claude.json`・
`managed-mcp-allow-codex.json`)に残し、次の適用で「前回あるが今回の宣言にない」分だけを削除する。
所有権は記録だけで決めるため、本人が手で追加した規則や許可は残る。Codex の MCP 許可は
`approval_mode = "approve"` のときだけ削除し、本人が承認方式を変えたツールには触れない
(`mcp_servers.<server>` 自体も残す)。記録がない初回は何も削除せず、撤回リスト(`retire_allow`)の
完全一致だけが従来どおり効く。


## Antigravity CLI の宣言

`agy`(Antigravity CLI)の権限も、Claude・Codex と同じ共有入力(`agentPermissions.secret_paths`・`domains`)と
Claude の許可・確認規則から、`home/modules/antigravity.nix` が `~/.gemini/antigravity-cli/settings.json`
へ配備する。個人の identity だけで有効にし(`home/identities/personal.nix` の
`dotfiles.antigravityPermissions.enable`。クラウド AI ツールを会社側へ配備しない、#9 と同じ判断)、
比較対象の宣言 `agent-native/antigravity-permissions.json`(`{enable, spec}`)は全ホストに出力する
(`hms --follow` は欠落を「変化」とみなして停止するため)。

| 正本 | agy のキー | 変換 |
|---|---|---|
| `secret_paths` | `permissions.deny` | `read_file(<絶対パス>)`。`~/` は Nix が `home.homeDirectory` に展開する(`~` の展開は文書に記述なし)。読み取りの拒否は書き込みの拒否も含む |
| `domains` + `raw.githubusercontent.com` | `permissions.allow` | `read_url(<host>)`。sandbox の外向き通信の許可リストにもなる |
| `dotfiles.claudePermissions.allowRules` | `permissions.allow` | `Bash(P *)`・`Bash(P)` → `command(P)`(語単位の前方一致)。`mcp__…` は変換しない(Claude の許可に、agy へ載せる 3 サーバー — playwright・playwright-chrome・esa — の `mcp__…` がなく、agy は道具ごとに確認する)。`P` に `*` を含む規則は、`noAgyCounterpart` に載せたものだけを除外し、他は評価時に失敗する |
| `dotfiles.claudePermissions.askRules` | `permissions.ask` | 同じ変換 |
| `sandboxed_shell = "auto"` | `toolPermission` | `proceed-in-sandbox`(`prompt` なら書かない) |
| (Claude の `sandbox.enabled`) | `enableTerminalSandbox` | `true` |

- 規則の評価順は Deny > Ask > Allow(Antigravity docs "Permissions"、<https://antigravity.google/docs/permissions.md>、取得 2026-11-26)。
  本人が承認や `/permissions` で追加した規則は、所有の記録(`~/.local/state/dotfiles/managed-permissions-antigravity.json`、種別は
  `allow`・`ask`・`deny`)にないため残り、宣言から外した規則だけが次の適用で削除される。
- 書かないキー: `agentMode`(既定が `auto`。CLI は既定値のキーを書き直しで消すため、宣言しても毎回差分になる)・
  `allowNonWorkspaceAccess`・`trustedWorkspaces`・`model`。MCP の許可は変換しない(上のとおり、変換する Claude 側の規則がない。サーバー自体は `dotfiles.agents.mcpServers` から `~/.gemini/config/mcp_config.json` へ配備する)。
- **広がる範囲**: Claude の `Bash(git push *)` などは分類器(`classifyAllShell`)を後段に持つが、agy の `command(git push)` 許可は
  確認なしで sandbox の内外を問わず動く(同ページ "Terminal sandbox" の `git push` の例)。agy の `auto` モードの審査は、許可規則に
  合致するコマンドには効かない。agy の `command(P)` は前方一致なので、Claude で完全一致の `Bash(nix fmt)`・`Bash(npm ci)` も引数付きで通る。
  Claude の `excludedCommands`(`hms *`・`lp *` など)に対応する `unsandboxed(...)` は、個人アカウントでは無視される(同ページ)ため
  変換しない。`hms`・`lp`・`pkexec`・headless Chrome は、確認規則(ask)だけを配備する。
- reconciler は、`action(target)` の形でない規則と、`toolPermission` の列挙にない値を、ファイルに書き込む前に exit 1 で止める
  (CLI は解釈できない規則を黙って無視するため、配備したつもりで効かない状態を作らない。スキーマ:
  <https://antigravity.google/schemas/v1/settings.json> の `permissionRule`)。

**実測(vega、2026-11-26)**: 使い捨ての `HOME` に reconcile した `settings.json` を置いて `agy -p` を起動すると、ログ
`CLI settings initialized: permissions=&{Allow:[…] Deny:[…] Ask:[…]}, toolPermission=proceed-in-sandbox` に、宣言した規則がそのまま
出た。起動時の書き直し後も規則と 2 つのスカラーは残り、キーが辞書順に並び(`permissions` の中は `allow`・`deny`・`ask`)、権限は 0600
になる。reconciler は順序を見ずに比較するため、書き直し後の再適用で差分は出ない。

**未確認**: 承認で「永続」にした規則の保存先(`settings.json` には書かれなかった)。vega で agy の Linux sandbox が使えるか
(使えないと `proceed-in-sandbox` は `request-review` として動く。Antigravity docs "Terminal sandbox")。`read_file(<dir>)` の拒否が
配下のファイルに及ぶか(許可側は再帰と書かれているが、拒否側は記述なし)。agy の OAuth トークンの置き場(`~/.gemini` 配下に名前で
見つかるファイルはなかった。鍵束なら `~/.local/share/keyrings` で拒否済み)。agy の実行中に `hms` が書いた場合、CLI が保持する設定で
上書きされるか(次の `hms` で戻る)。実機での確認は vega の確認台帳に載せる。

## 認証 launcher は native の保護を確かめる

`crates/codex-launcher/src/lib.rs` の `validate_config` は、認証付き起動の前に
`on-request` / `auto_review`、proxy と credential masking、選択した workspace profile を検査する。
保護を上書きする profile・project 設定や別設定ホームは拒否し、確認した profile を起動時に指定する。
CLI の版・引数も確認してから資格情報を渡す。共有の移行済み記録や hash への依存はない。
この検査は認証情報の引き渡し条件であり、全ツール呼び出しを共有所有検査へ通すものではない。

通常 CLI と `codex-plan` は `codex_launcher::command` を共有する。
起動種別は `Launch` の閉じた enum で、通常 CLI のセッションと固定の
`app-server --listen stdio://` だけが保護検査後に `gh auth token --hostname github.com`
を呼ぶ。継承した GitHub token は全種別で除去し、取得した値は実セッション用の
子プロセス環境だけへ渡す。TUI・情報表示・モデル探索には渡さない。
新規・再開・派生のために adapter を起動し直すと、その都度取得し直す。
認証取得に失敗した場合は実セッションを起動せず、端末での再認証と
セッションの再起動・再開起動を案内する。実行中の親の資格情報は更新されない。

資格情報を tool の環境とモデル入力から隠す処理は、OpenAI *Codex* `rust-v0.164.0`
(commit `6aaac694803004a63c6f393d153c96d22521a6b9`) の native broker に任せる。
[GitHub provider L14–51](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/network-proxy/src/credential_broker/providers/github.rs#L14-L51)
の token 形状・環境変数・宛先制約を使い、独自の中継は追加しない
(公開日不明、取得日 2026-10-27)。返された `Command` は資格情報を含み得るためログに出さない。
認証付き中継は各 thread / turn の cwd と設定を再検査する。権限切替・直接の native コマンド・
cwd が不明な特殊再接続の制約は [codex-plan の起動経路](../codex-plan.md#github-資格情報の起動経路) に記す。

## Claude と Codex の層構造を揃える

共有ポリシーの最初の移行(2026-10-26)は、Claude の `sandbox`・`autoMode.classifyAllShell`・Read の秘密パス deny を
書いた。撤去(上の節)は journal の残る変更しか戻せず、最初の移行の journal は `transaction.rs` の `prune` で
消えていたため、Claude は「sandbox 有効・通信先空・自動許可なし・除外コマンドなし」の壊れた状態で残った。
sandbox の中では `gh` の認証(鍵束の D-Bus)・署名付き commit(gpg-agent)・`/tmp` 直下への書き込みが失敗し、
sandbox を外した再実行は自動モードの判定が「Safety Bypass」で拒否した。

直し方として「sandbox を外して先週に戻す」ではなく、Codex と同じ層構造に宣言で揃える(本人の裁定、2026-10-27)。
公式の指針は「権限ルールと sandbox は別の層で、両方を使う(defense-in-depth)」である。

| 層 | Claude の公式な指針 | Codex の公式な指針 | Codex(`dotfiles` profile) | Claude の宣言 |
|---|---|---|---|---|
| 権限ルール | 評価順は deny → ask → allow。Bash ルールは「プログラムの周りの境界ではない」。引数を絞る Bash パターンは壊れやすい | `~/.codex/rules/` に `prefix_rule(pattern = ["curl"], decision = "prompt", justification = …)` を置き、宛先が許可リストにあるコマンドも確認に回せる。追加後は Codex を再起動する(`recommended-configuration`) | Bash 前方一致の宣言なし。MCP は既定で毎回確認 | 狭い allow(`permissionRules`)と秘密パスの Read deny。広い Bash 前方一致は宣言しない |
| 分類器 | 自動モードの分類器。`classifyAllShell` は全シェルを分類し Bash の allow を停止する。分類器への例外は `autoMode.allow`(自然言語)で書く。`"$defaults"` を含めないと既定の一覧が丸ごと消える | `approvals_reviewer = "auto_review"` で、承認が必要な要求だけを審査エージェントに回す(`approvals_reviewer` の既定は `"user"`)。sandbox の中で完結する操作は審査しない(`agent-approvals-security`) | `auto_review` が全承認要求を審査する | `autoMode.classifyAllShell = true`、`autoMode.allow`(`"$defaults"` + 自分の linked worktree でのブランチ作成は可。main の checkout・ブランチ削除・push は含めない) |
| sandbox | OS が強制する。直し方は `allowWrite`・`allowedDomains`・狭い `excludedCommands`。`autoAllowBashIfSandboxed` は既定 true。`WebFetch(domain:...)` の許可は通信先にも加わる | `default_permissions` の権限 profile で最小権限の境界を作る(変更が不要なら `:read-only`、編集が必要なら `:workspace` を継承)。例は `approval_policy = "on-request"`・`approvals_reviewer = "auto_review"`・`features.network_proxy = true`(ないと `network.enabled = true` は宛先の許可リストを効かせない)・秘密のパターンの deny・通信先の allow。profile と旧 `sandbox_mode` は併用しない。既定は通信オフ。`<writable_root>/.git` は読み取り専用で保護。`dangerously_allow_all_unix_sockets = true` は Unix socket の許可リストを迂回するので、厳密に管理された環境でだけ使う | 常時 sandbox(全体 read・workspace write・秘密 deny・通信先を宣言) | `sandbox.enabled`・`failIfUnavailable`・`autoAllowBashIfSandboxed = true`・`allowedDomains`・`denyRead`・`excludedCommands` |
| hook | hook の判定は deny / ask を越えられない | 取得した 2 ページに記述なし | 同種の hook | 変更しない |

出典(Anthropic, "Sandboxing" と "Configure permissions"、<https://code.claude.com/docs/en/sandboxing>、
<https://code.claude.com/docs/en/permissions>、取得 2026-10-27、本文を逐語で引く形で取得して読んだ)。
Codex 側の公式な設計指針の出典(OpenAI, ChatGPT Learn、公開日不明、取得 2026-10-28。headless Chromium で本文を読み、表の文の根拠を本文で確かめた):

- "Recommended configuration"(<https://learn.chatgpt.com/docs/cyber-safety/recommended-configuration>)。
  サイバーセキュリティの試験環境(higher-risk な作業)向けの推奨で、一般の開発作業の推奨ではない。ただし権限 profile・
  `auto_review`・`network_proxy`・`prefix_rule` の使い方の例として、層ごとの対応表に使える。本文は
  「Use Codex permission profiles to create a least-privilege boundary.」と書き、`approval_policy = "on-request"` /
  `approvals_reviewer = "auto_review"` / `default_permissions` と `[features] network_proxy = true` の例を挙げる。
- "Agent approvals & security"(<https://learn.chatgpt.com/docs/agent-approvals-security>)。本文は
  「By default, the agent runs with network access turned off.」「The reviewer evaluates only actions that already need approval」
  「`<writable_root>/.git` is protected as read-only whether it appears as a directory or file.」
  「`dangerously_allow_all_unix_sockets = true` bypasses the Unix socket allowlist. Use them only in tightly controlled environments.」と書く。
  `developers.openai.com/codex/…` は 308 で `learn.chatgpt.com/docs/…` へ移っている(`/codex/` が付かない)。

### 宣言(`home/modules/codex.nix`)との食い違い

- 食い違わない: `dangerously_allow_all_unix_sockets = false`。herdr や署名が必要な操作は native の自動審査に渡す。
- 食い違わない: `approval_policy = "on-request"`、`approvals_reviewer = "auto_review"`、`default_permissions` の名前付き profile が
  `:workspace` を継承すること、`features.network_proxy = true`、秘密パスの deny、通信先の allow、旧 `sandbox_mode` を併用しない
  こと(reconciler が取り除く)。
- 宣言しない: `ja-polish` の `allow`(`~/.codex/rules/dotfiles.rules` の `prefix_rule`)。#1517 で入れたが、Codex のファイルシステムの制限(秘密パスの deny)があると、`allow` の規則は、その制限を外す許可にならない(OpenAI Codex rust-v0.165.0、`codex-rs/core/src/tools/sandboxing.rs:255-256`、取得 2026-11-15: 「a command allow rule does not authorize removing filesystem restrictions」)。Codex から実行した `ja-polish` は、キーの置き場(`~/.config/io.datasette.llm`)を読めずに素通しになったので、規則を外した(`docs/agents/ja-polish.md`「Codex では動かない」)。公式文書 OpenAI, Codex Rules(<https://learn.chatgpt.com/docs/agent-configuration/rules>、取得 2026-11-10)は、`allow` を「Run the command outside the sandbox without prompting.」と書くが、制限のあるファイルシステムでの例外は書いていない(取得した資料に記述なし)。それ以外のコマンドの `prefix_rule` も、宣言しない(公式の例は、許可リストにある宛先のコマンドも確認に回す用途で、Codex は Bash 前方一致の宣言を持たない、という上の表の判断は変えていない)。`reply-coverage` の規則は、別の仕組み(`docs/agents/reply-coverage.md`)が持つ。同じ制限が掛かるかは未確認。
- 取得した 2 ページに記述なし: `features.credential_masking`、hook の層。

Claude Code の sandbox が起動に必要とするコマンド(`socat`、Linux では `bubblewrap`)は、対話モードの Claude では `home.packages` で PATH に入る。
systemd の user unit のような headless 起動の PATH には `~/.nix-profile/bin` がなく、`failIfUnavailable = true` が効いて
`sandbox required but unavailable: … socat not installed` で起動を拒否する(2026-10-28、メールの自動仕分け unit が 10 分ごとに失敗)。
そのため `home/modules/claude.nix` は、同じ一覧から導いた `dotfiles.claudePermissions.sandboxBinPath`(nix store の bin を `:` で
つないだ文字列、読み取り専用)を生成する。headless で `claude` を起動する unit は、これを `Environment=PATH=…` に連結する。
unit の実値は private wrapper に置く(ADR-0034)。

`excludedCommands` の既定は、sandbox 内では鍵束の D-Bus・gpg-agent・nix daemon のソケットに届かない
コマンド(`gh`・`git commit`・`git push`・`git add`・`git fetch`・`git pull`・`nix build`・`nix eval`・`nix flake`・`nix fmt`)である。sandbox 外で動くため、
狭い allow と分類器を通る。内部で `gh` を呼ぶ自前ツール `pr-checks-wait` も同じ理由で追加した(sandbox 内でバックグラウンド起動すると
`PR の base ブランチを取得できません` で exit 2 になった。2026-10-28。適用後の確認は `hms` の後)。同様に `pr-hygiene-check *`(自分の PR を
点検するツール。内部で `gh` を呼ぶため、sandbox 内では所有者表もログイン名も取得できず失敗した。2026-11-15、朝の机の段 5a。読み取り専用)と、`lpstat *`
(プリンタの状態を読むのみ。下の `lp` の節)も追加した(2026-11-20)。

**除外パターンの照合規則**(Anthropic, "Sandboxing"、取得 2026-10-27): 呼び出し 1 行全体のコマンドがすべて
除外パターンに一致しないと、全体が sandbox の中に残る。パターンは呼び出しの文字列全体に対して照合される。`cd`・
ファイルへのリダイレクト・`$(…)` を含む呼び出しは sandbox に残る。さらに実測(下の表)で、**末尾だけに `*` を
持つパターンだけが効き**、途中に `*` を持つ `git -C * commit *` はリテラルのパスでも効かなかった。このため
git は worktree の cwd で `-C` なしに呼ぶ。`git -C *` のような広い除外は、全ての `-C` 呼び出しを外に出すため採らない。

**実測(vega、2026-10-28、#1164、`hms` 適用後の新しいセッション・Auto モード)**:

| 呼び出し | 結果 | 読み |
|---|---|---|
| `gh auth status; echo "exit=$?"`(1 回目) | 失敗(`token is invalid`) | 末尾の `echo` が除外に合わず sandbox に残った |
| `gh auth status`(2 回目、単体) | 成功(keyring でログイン済み) | 除外が効く。鍵束に届く |
| `gh pr list -R tarotene/dotfiles --limit 1`(単体) | 成功 | 同上 |
| `git push --dry-run`(単体) | 成功 | 同上。認証情報が通る |
| `git commit -S --allow-empty -m probeA`(worktree の cwd、`-C` なし、単体) | **成功**(署名できた) | `git commit *` の除外が効く。gpg-agent に届く |
| `git -C <リテラルの絶対パス> commit -S ...`(`git -C * commit *` を除外に入れた状態) | 失敗(`No secret key`) | 途中の `*` を持つパターンは効かない(変数展開が原因ではない) |
| `git -C "$TMPDIR/…" commit -S ...`、`git -C … add . && git -C … commit -S ...` | 失敗(`No secret key`) | 同上 |
| `git rebase --force-rebase HEAD~1`(使い捨てブランチ、worktree の cwd。除外になかった) | 失敗(`gpg: skipped "…": No secret key`、`failed to write commit object`) | `commit.gpgsign=true` のため、書き換えたコミットの再署名が sandbox 内の gpg に届かない |
| `git fetch origin`(除外に `git fetch *` を追加する前。worktree の cwd、単体、2026-10-28 に別のセッションで測定) | 失敗(`fatal: could not read Username for 'https://github.com': No such device or address`) | `fetch` も認証情報を gh の keyring 経由で読むため、sandbox 内では届かない。`git fetch *` と `git pull *` を除外に追加した(`git push` と同じ理由。`git fetch` を sandbox の外で再実行すると成功した。`git pull` は `fetch` を内部で呼ぶため同じ経路からの推定であり、未測定) |
| `git switch -c <branch> origin/<ref>`・`git branch -D <branch>`(除外に `git switch *`・`git branch *` を追加する前。worktree の cwd、sandbox 内、2026-11-11 に別のセッションで測定) | 失敗(`error: could not lock config file …/.git/config: File exists`)。`git switch -c` はブランチ作成と index・作業ツリーの更新は済むが、HEAD は元のブランチのまま残る。`git branch -D` はブランチ設定節の削除で止まる | sandbox は `.git/config.lock` の代役を置いて書き込みを禁じる(sandbox 外からは存在しない)。`branch.autoSetupMerge` の上流設定と設定節の削除が `.git/config` への書き込みになるため、`config.lock` の取得で失敗する。並行セッションとの競合という最初の推測は誤りだった。`.git/config` はフックやエイリアスを書けるため `allowWrite` には入れず、`git switch *` と `git branch *` を除外に追加した(追加後の効果は未測定。`hms` 適用後の新しいセッションで、`git switch -c <branch> origin/<ref>` が上流設定まで完了するかを確認する) |
| `git branch -D <枝> <枝> …`(複数引数)と `git branch --track <枝> origin/main`(上流設定付きの枝。除外に `git branch *` を追加後の設定。worktree の cwd、2026-11-20、vega、単体で呼び出し) | 成功。`could not lock config file` は出ず、枝が消え、`.git/config` の `branch.<枝>` の節も消えた(`git config --get-regexp 'branch\.<枝>'` が何も返さない)。上流設定を持たない枝はそもそも `.git/config` を書かないため、この測定では判定にならない | F73(2026-11-17 に `git branch -D` の 3 ブランチが `could not lock config file` で失敗)は、除外に `git branch *` を追加(2026-11-12、`b4451d93`)する前の設定のセッションだった可能性が高い(その日の `hms` 適用状況は未確認)。複数引数が除外パターンに当たらないという仮説はこの測定で反証された |
| `nix build nixpkgs#hello --no-link`(除外に `nix build *` を追加する前) | 失敗(`Read-only file system`、`cannot create Unix domain socket`) | `nix` は除外になかった。socket の作成自体が sandbox に拒否される(#1050) |
| `nix build nixpkgs#hello --no-link`(`nix build *` を追加した後) | **成功** | 除外が効く |
| `cargo --version`・`$TMPDIR` への書き込み | 成功 | sandbox 内で確認なしに通る |

自動モードの分類器は実測の全呼び出しを許可した(sandbox を外した再実行はしていないため、「Safety Bypass」の問題は除外コマンドで外実行になるこの構成では起きなかった、という範囲で確かめられた)。
署名が必要な書き換え系の `git rebase *`・`git merge *`・`git cherry-pick *`・`git revert *`・`git tag *` も除外に追加した(`git rebase` は上の表で実測した失敗。残りは同じ理由からの推定で、`hms` 後に確認する)。
この結果に基づき、既定に `git add *`・`nix build *`・`nix eval *`・`nix flake *`・`nix fmt *` を追加した(`git -C *` の 3 件は効かないと分かり除外した)。連結(`git add . && git commit`)を cwd で単体測定する項目が残る(#1164)。`nix` を sandbox の中に残す案(`allowWrite` で `~/.cache/nix` と socket を許可)は、socket 作成自体が拒否されるため採らない(`allowAllUnixSockets` が必要になり、範囲が広すぎる)。
`/tmp` 直下への書き込みは `allowWrite` を広げず、`$TMPDIR` を使う運用のままにする。

**自前ツールの書き込み先(2026-10-28、vega、#1164)**: hook の指示でエージェントが Bash で呼ぶツールは sandbox の中で動く。状態の置き場が書き込み許可にないと、`Read-only file system` で失敗する。実測で確認した失敗は次のとおりである:
`worklog add`(vault の journal)、`session-retrospective --retro-add`(`~/.local/state/claude/wrapup/retro`)、`issue-inbox --check-dup`(台帳の snapshot)。hook そのものは Claude Code が sandbox の外で動かすため影響しない。
直し方は、`excludedCommands`(全権限で動く)ではなく、ツールが所有する場所だけを `sandbox.filesystem.allowWrite` に宣言する:
`~/.local/state/claude`(`claude.nix`)と、vault(`obsidian.nix` の `vaultPath`。vault を宣言した機体だけが追加する。vega は `obsidian.nix`、会社の機体は private 側の宣言。2026-11-13 に arcturus の sandbox の書き込み許可に会社用 vault が入っていることを確かめた)。
`dotfiles.claudePermissions.allowWrite` は各モジュールが追加した値の和集合になる。

**uv のキャッシュと PyPI の通信先(2026-10-29)**: Python のリポジトリで、sandbox 内の `uv sync` が `~/.cache/uv` に書き込めず `Read-only file system (os error 30)` で失敗し、続けて `files.pythonhosted.org` への通信が拒否されて失敗した。
docs のビルドでは intersphinx が `docs.python.org` を読む。同様の理由で、uv を入れる `runtimes.nix` が `~/.cache/uv` を `allowWrite` に追加し、`domains` の既定に `pypi.org`・`files.pythonhosted.org`・`docs.python.org` を追加した。
`numpy.org` のように 1 つのリポジトリだけが使う宛先は、共有の `domains` には追加しない。

**npm のキャッシュ(2026-10-31 に観測、2026-11-08 に宣言)**: sandbox 内の `npm ci` が `~/.npm/_cacache/tmp` に書き込めず `EROFS: read-only file system` で失敗した。`--cache` を scratchpad に向けると通った(レジストリへの通信先は届いていた)ため、不足していたのは書き込み許可だけである。uv・cargo と同様に、`runtimes.nix` が `~/.npm/_cacache` だけを `allowWrite` に追加する(`~/.npm` 全体は許可しない。`~/.npm/_logs` は既に書き込み可能)。宣言後の確認(`hms` 適用後の新しいセッションで `npm ci` が通ること)は未実施。

**`hms` を sandbox の外で動かす(2026-11-08)**: `hms .` を sandbox 内で実行すると適用ロック(`~/.local/state/hms/lock`)を取得できず `Read-only file system` で失敗し、sandbox の外で再実行する往復が必要だった。`hms` は home-manager の適用で実環境に書き込むため、`allowWrite` の拡張ではなく `excludedCommands` に追加する。sandbox 外では全権限で動くため、`google-chrome --headless=new *` と同様に `askRules` にも同じパターンを追加し、実行のたびに許可の確認を出す。引数なしの `hms` が `hms *` に一致するかは未測定のため、`hms` と `hms *` の両方を書く。宣言後の確認(確認画面が出ること、許可すると適用ロックを取得して通ること)は未実施であり、`hms` 適用後の新しいセッションで確認する。画面が出ないと分かったら除外を外す(ask は除外と対にする)。

**`npx`・`bunx` は呼び出しごとの前置で `$TMPDIR` を使う(2026-11-20、F24)**: sandbox 内の `npx` は `~/.npm/_npx/<id>` を作れず失敗する(再現: `ENOENT: … mkdir '~/.npm/_npx/…'`。`~/.npm/_cacache` だけが書き込み許可)。`bunx` は `bun is unable to write files to tempdir: ReadOnlyFileSystem` で失敗する。
`npm_config_cache=$TMPDIR/npm npx --yes <pkg>` と `BUN_INSTALL_CACHE_DIR=$TMPDIR/bun bunx <pkg>` は sandbox 内で動作した(2026-11-20、vega、`cowsay` で実測。bun は `BUN_INSTALL_CACHE_DIR` だけで足り、`BUN_TMPDIR` だけでは足りない)。**許可や恒久の `env` は追加しない**: `npx`・`bunx` が取得した実行物は `<cache>/_npx` や bun の install cache に置かれ、書き込み可能な恒久の場所に置くと、sandbox 内で配置された実行物を sandbox 外(`dangerouslyDisableSandbox`)の `npx` が実行し得る(`runtimes.nix` が `~/.npm` 全体を許可しない理由と同じ)。`$TMPDIR` はセッションごとであり、実行物が残らない。`npx`・`bunx` は除外コマンドではないため、前置しても呼び出しは sandbox の外へ出ない。実測の途中で `~/.npm/_cacache` の下にできた `_npx`・`bun`・`_cacache` の実験の残骸は片付けた。

**`lp` を sandbox の外で動かす(2026-11-17)**: 朝の机の紙を家のプリンタへ印刷する `lp` は CUPS に Unix ソケットで届くため、sandbox 内では失敗すると推定して(2026-11-17 の時点では、書いた機体で CUPS が動いておらず `lpstat` が「Scheduler is not running」を返したため未実測だった。その後、2026-11-12 の vega の実測が見つかった: `lpstat -p -d` は sandbox 内で「Scheduler is not running」、外ではプリンタの状態を返した。読み取り専用の `lpstat *` も同様に 2026-11-20 に追加した。`lp` そのものを sandbox 内で動かした実測はない)、`excludedCommands` に追加した。外では全権限で動くため、`google-chrome --headless=new *`・`hms *` と同様に同じパターン `Bash(lp *)` を `askRules` にも追加し、印刷のたびに確認を出す(除外と対)。
紙の出力先 `~/.local/state/claude/desk/sheets` は、`home/modules/downloads.nix` の `systemd.user.tmpfiles.rules` に保持期間 14 日の掃除対象として追加した(ディレクトリは 0700)。
宣言後の確認(`hms` 適用後の新しいセッションで確認画面が出ること、プリンタを設定した機体で `lp` が通ること)は未実施。

**離席実行のコマンドと Calendar の書き込み(2026-11-19)**: 勤怠の同期を、本人が出発前に 1 回承認してから席を離れて動かす運用にする。離席中に許可の確認で止まると、その run は戻るまで進まない。
止まる箇所は 2 か所あった。(1) `ts-browser`(画面付きの Chromium を起動し、Unix ソケットを作れず sandbox 内で起動できない。2026-10-30 に実測)・`wl`(`oauth2.googleapis.com` に届かない。2026-11-11 に実測)・`work-sync-plan.sh`(その 2 つと、スマートカードが必要な `sops` を呼ぶ)は、`excludedCommands` にないため外へ出すたびに許可判定を通る必要があった。(2) Calendar の `update_event`・`delete_event` は事前許可になかった(`create_event` は本線の `settings.local.json` にだけあった)。そこで `excludedCommands` に `uv run --project timesheet/tools ts-browser *`・`uv run --project worklog/tools wl *`・`./scripts/work-sync-plan.sh *` を、`claudePermissions.extraAllow` に Calendar の `create_event`・`update_event`・`delete_event` を追加した。
**広がる範囲**: sandbox 外では全権限で動く。`uv run --project <dir>` はセッションのディレクトリの `<dir>` にあるコードをそのまま実行するため、別のリポジトリでも同じパスのコードがあれば同じ扱いになる。Calendar の書き込みもどのセッションでも確認なしになる。`google-chrome --headless=new *`・`hms *`・`lp *` とは違い、`askRules` には追加しない(確認を出すと離席実行の意味がない)。書き込みの歯止めは許可層ではなくツール自身にある(プレビューのダイジェストへの束縛・読み戻し・書き込みガード・`labor-core away decide`)。`respond_to_event`(`/leave` の辞退)と `ts-browser punch` の個別許可は追加しない。Codex には追加しない(`mcp_allow` は Codex も読むため使わない)。
宣言後の確認(`hms` 適用後の新しいセッションで確認画面なしにこの 3 つのコマンドと Calendar の 3 ツールが通ること、自動モードの分類器が止めないこと)は未実施。
分類器が止めると分かった場合に限り、`auto_mode_allow` に 1 文追加する。画面ロック中の Chromium の描画も未実測であり、初回は本人が同席して試運転する。

**cargo のレジストリと headless Chrome(2026-11-05、#1334)**: sandbox 内の `cargo metadata --offline --locked` が `~/.cargo/registry/src` を作れず `Read-only file system (os error 30)` で失敗した(`cargo run -p pipeline-map -- render` の最初の段)。
uv のキャッシュと同様であるため、`runtimes.nix` が `~/.cargo/registry` だけを `allowWrite` に追加した(`~/.cargo/bin` は許可しない)。
未取得の crate を取る通信先(crates.io)は当時は `domains` になかった(未実測)。2026-11-20 に sandbox 内の `cargo test --workspace` が `failed to download from https://static.crates.io/crates/tungstenite/0.30.0/download` で失敗したため、`index.crates.io` と `static.crates.io` を `domains` に追加した(F60。公開パッケージの置き場であり資格情報は不要)。一方、headless Chrome は sandbox 内で Unix ソケットを作れず失敗する。`allowAllUnixSockets` は D-Bus・鍵束に届くため採らず、`google-chrome --headless=new *` を `excludedCommands` に追加し、同じパターンを `askRules` にも追加した。sandbox 外では全権限で動くため、歯止めは ask だけになる。
ask の画面が実際に出るかは `hms` で適用した後でないと確認できない(未実測)。出ないと分かったら除外を外す。
図(SVG)の PNG 化は、ブラウザを使わない `librsvg`(`rsvg-convert`)に任せ、sandbox 内で行う。
Codex 側は、書き込み先が `workspace`・`temp` の 2 つの記号だけで(`agent-permissions.nix` の `writable_roots`)、ツールごとのキャッシュ宣言を持たないため対応する変更はない。

**`just` と `uvx` の書き込み先を `env` で向ける(2026-11-05)**: sandbox 内で、`just` の shebang レシピは `/run/user/1000/just`(`$XDG_RUNTIME_DIR/just`)に書けず `Read-only file system` で、`uvx` は `~/.local/share/uv/tools` に書けず `Could not create temporary file` で失敗した。`JUST_TEMPDIR=… just`・`UV_TOOL_DIR=… uvx` と前置すると通るが、前置は Claude Code の既知の安全な変数(一覧は非公開)にないため除外パターンにも一致しなくなる(`sandbox-compound-guard.md`)。
そこで `runtimes.nix` が `dotfiles.claudePermissions.env` に `JUST_TEMPDIR=~/.cache/just`・`UV_TOOL_DIR=~/.cache/uv/tools` を絶対パスで宣言し、`agent-settings-reconcile claude-permissions` が settings.json の `env` へ書く(宣言したキーだけ。`claude-plan-model` の `ANTHROPIC_DEFAULT_*_MODEL` には触れない)。`env` の値は展開されない(Anthropic, "Settings reference" の `env`、取得 2026-11-05。「Claude Code doesn't expand `$VAR` or `~` in values」)ため `$TMPDIR` は使えない。
`~/.local/share/uv/tools` を `allowWrite` に追加する案は採らない(`~/.local/bin` から参照される実行ファイルを sandbox 内から置き換えられてしまう)。
`GIT_EDITOR=true` は宣言しない: Claude Code が Bash の環境にすでに入れており(sandbox 内は `env` で、外の `git rebase --continue` は hook で確認)、前置は不要だった。`uv tool install` は `env` を効かせたセッションでは `~/.cache/uv/tools` に置かれ、`~/.local/bin` の参照先が sandbox から書ける場所になるため、そのセッションでは使わず `uvx` を使う。

宣言の仕組み・キーは [claude-permissions](../claude/claude-permissions.md#sandbox自動モード秘密パスも宣言する2026-10-27) に書く。

## 旧共有管理を撤去する

既存手段: `agent-permissions` — 拡張。既存の transaction journal の変更前・変更後を使う `retire` を追加し、別の移行ツールは作らない。通常の activation はこの撤去を native writer より先に行う。
手元で変更候補だけを見る場合は、次の順で行う。

```sh
agent-permissions retire --home "$HOME" --dry-run
agent-permissions retire --home "$HOME"
```

`--home` は撤去対象ホームの絶対パス。dry-run は変更候補を JSON で返し、書き込まない。
実行は現在の所有記録からたどれる journal だけを使い、取り消された移行は除外する。
証明できる共有変更だけを逆適用し、現在値が記録後の値と違う箇所は本人の編集として保持する。
後から加えた許可や個別 MCP の許可も保持する。完了は transaction と読み戻しで確かめ、撤去済みの印を残す。同じホームへ再実行しても共有設定を作り直さない。

journal がない場合は保守的に扱う。共有管理の名前付き selector とその profile の撤去だけを対象にし、由来を証明できないローカル設定・project 設定・rules は削除しない。
未完了の transaction がある場合は、その回復を先に行い、撤去で上書きしない。
旧 `inspect` / `migrate` / `apply` / `check` は互換 CLI の入口として残るが、通常実行や activation の前提ではない。
旧 promotion queue は自動で再適用しない。必要な許可は `/promote-permissions` から native 宣言として提案する。

### 撤去が戻せないものと、終わらせる条件(#1158)

撤去は 1 回で終わる設計のままにする。理由: 再実行しても journal がなくなった変更は戻らない(journal は `transaction.rs` の `KEEP_JOURNALS = 5` で完了のたびに古いものから消える)。失敗は無言にせず、dry-run と実行の出力の `retained` に「由来を証明できず戻さなかったもの」を理由付きで並べ、`queued_rules` に待ち行列の件数を出す。

- 最初の移行が書いた Claude の `sandbox`・`autoMode.classifyAllShell`・Read の秘密パス deny は、journal が消えていても戻らない。これらは撤去に頼らず、`home/modules/claude.nix` の宣言が直接合わせる(#1165)。
- 待ち行列の許可(`agent-permissions-pending.jsonl`)は、`agent-permissions pending` で一覧表示し、`pending --restore-rules ID,ID|all` で元の Claude 設定(`.json` の `permissions.allow`)へ戻して待ち行列から外すか、`pending --drop ID,ID` で外す。元のファイルがない・`.json` でないものは戻さず、待ち行列に残して理由を出す(消えた project の設定を作り直さない)。
- `home/modules/codex.nix` の `retireAgentPermissions`(全ホストの activation のたびに走る)を外す条件: 3 ホストすべてで撤去済みの印があり、各ホストの `retained` を本人が確認し、待ち行列が空になったとき(確認は #1104)。外すときは、この活性化と `retire` モジュール、旧 CLI を同じ変更で削除する。

## 承認経路は upstream のコードに基づいて区別する

以下は OpenAI, *Codex*, `rust-v0.164.0`、commit
`6aaac694803004a63c6f393d153c96d22521a6b9` のコードを読んだ範囲である。
公開日未確認、取得日 2026-10-27。

- 承認要求は PermissionRequest hook の allow / deny を先に適用し、判定がないときに reviewer へ進む。
  `auto_review` を宣言しても既存 hook の拒否を迂回しない。
  [`tools/approvals.rs` L484–505](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L484-L505)。
- reviewer は guardian が応答すればその結果を採り、応答がない経路は user へ進む。
  全ツールが必ず自動承認されるという意味ではない。
  [`tools/approvals.rs` L552–583](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L552-L583)。
- Apps は app policy、custom MCP は server/tool の approval mode を使う。
  [`mcp_tool_call.rs` L219–229](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/mcp_tool_call.rs#L219-L229)。
  Apps の reviewer は link → app → app default の順に見て、requirements が許す指定を使う。
  custom MCP はその app 固有の上書きを使わず、既定 reviewer を使う。
  [`connectors.rs` L514–550](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/connectors.rs#L514-L550)。

設定の parse・読み戻しと、reviewer が実際に審査したことは区別する。
今回の nested sandbox 内では reviewer の end-to-end 実行は未検証。
既存の過去の実測を、今回の設定で実測した結果として流用しない。

## main の自動追従では native 宣言を比較する

`hms --follow` は候補世代と配備済みの両方にある
`agent-native/claude-permissions.json`・`agent-native/codex-permissions.json`・
`agent-native/antigravity-permissions.json` を全バイト比較する。
3 つの宣言が存在し、読み取れ、一致するときだけ自動適用する。変更・欠落・読み取り失敗時は
適用せず通知し、本人の差分確認と手動の `hms` を待つ。共有管理の `migrate` は要求しない。
詳細は [適用手順](../operations.md) の main 自動追従を参照する。
