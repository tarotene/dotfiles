# Claude と Codex はそれぞれの native 権限設定を使う

共有権限ポリシーの実行時管理は撤去した。Claude は Claude 専用 reconciler、
Codex は native 設定を使う。所有記録が無いことや、設定を本人が変更したことを
理由に router が通常のツール実行を止める仕組みは無い。裁定は
[ADR-457 の 2026-10-27 Amendment](../adr/457-claude-native-install-source-of-truth.md) に残す。

## 設定の正本と適用後の比較対象

| 対象 | 宣言・執行点 | 適用する内容 |
|---|---|---|
| Claude | `home/modules/claude.nix`、`agent-settings-reconcile claude-permissions` | native の allow / ask / deny と撤回リスト、`sandbox`・`autoMode.classifyAllShell`([層構造](#claude-と-codex-の層構造を揃える)) |
| Codex | `home/modules/codex.nix`、`agent-settings-reconcile codex-permissions` | `on-request`、`auto_review`、名前付き `dotfiles` 権限 profile、通信 proxy、秘密パスの masking、個別 MCP 設定 |

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
`autoMode.allow`（#939）は `auto_mode_allow` の宣言があるときだけ書く。先頭に `"$defaults"` を置き、本人が足した要素は残して
宣言との和にする。自分の worktree での `git switch -c` が `[Modify Shared Resources]` で拒否された（2026-10-22 の
1 回）のは、`classifyAllShell` が Bash の allow（`Bash(git switch *)`）を停止し、分類器が `.git` の保護パスを判定したため
（Anthropic, "Configure auto mode"、<https://code.claude.com/docs/en/auto-mode-config>、取得 2026-10-28。#939 のコメントが逐語で引用）。
分類器は `~/.claude/settings.json` の `autoMode` だけを読むので、この宣言は `agent-settings-reconcile claude-permissions` が
そこへ書く。実セッションで拒否が出なくなるかは、適用後の観測で確かめる（この PR では未確認）。

Codex の native writer は宣言した名前の profile だけを置き換え、古い通信先や書き込み許可を残さない。
ほかの profile・モデル・trust は保持する。`default_permissions` を宣言するときは競合する旧
`sandbox_mode` / `sandbox_workspace_write` を取り除く。これらの置換前には同じディレクトリに
権限 0600 の `config.toml.native-permissions-backup.<内容のSHA-256>` を作り、現在値の一致を確かめて更新する。

宣言から外した許可（`mcp_allow` から外した MCP 許可、Claude の `allow` から外した規則）は、
hook と同じ「所有の記録」（`agent-settings-reconcile` の `managed`、#1161）で消す。
前回の適用で書いた集合をホストごとのファイル（`~/.local/state/dotfiles/managed-allow-claude.json`・
`managed-mcp-allow-codex.json`）に残し、次の適用で「前回あるが今回の宣言に無い」分だけを消す。
所有は記録だけで決めるので、本人が手で足した規則・許可は残る。Codex の MCP 許可は
`approval_mode = "approve"` のときだけ消し、本人が承認方式を変えたツールには触れない
（`mcp_servers.<server>` 自体も残す）。記録が無い初回は何も消さず、撤回リスト（`retire_allow`）の
完全一致だけが従来どおり効く。


## 認証 launcher は native の保護を確かめる

`crates/codex-launcher/src/lib.rs` の `validate_config` は、認証付き起動の前に
`on-request` / `auto_review`、proxy と credential masking、選択した workspace profile を検査する。
保護を上書きする profile・project 設定や別設定ホームは拒否し、確認した profile を起動時に指定する。
CLI の版・引数も確認してから資格情報を渡す。共有の移行済み記録や hash への依存は無い。
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
（公開日不明、取得日 2026-10-27）。返された `Command` は資格情報を含みうるのでログに出さない。
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

| 層 | Claude の公式な指針 | Codex(`dotfiles` profile) | Claude の宣言 |
|---|---|---|---|
| 権限ルール | 評価順は deny → ask → allow。Bash ルールは「プログラムの周りの境界ではない」。引数を絞る Bash パターンは壊れやすい | Bash 前方一致の宣言なし。MCP は既定で毎回確認 | 狭い allow(`permissionRules`)と秘密パスの Read deny。広い Bash 前方一致は宣言しない |
| 分類器 | 自動モードの分類器。`classifyAllShell` は全シェルを分類し Bash の allow を停止する。分類器への例外は `autoMode.allow`(自然言語)で書く。`"$defaults"` を含めないと既定の一覧が丸ごと消える | `auto_review` が全承認要求を審査する | `autoMode.classifyAllShell = true`、`autoMode.allow`(`"$defaults"` + 自分の linked worktree でのブランチ作成は可。main の checkout・ブランチ削除・push は含めない) |
| sandbox | OS が強制する。直し方は `allowWrite`・`allowedDomains`・狭い `excludedCommands`。`autoAllowBashIfSandboxed` は既定 true。`WebFetch(domain:...)` の許可は通信先にも加わる | 常時 sandbox(全体 read・workspace write・秘密 deny・通信先を宣言) | `sandbox.enabled`・`failIfUnavailable`・`autoAllowBashIfSandboxed = true`・`allowedDomains`・`denyRead`・`excludedCommands` |
| hook | hook の判定は deny / ask を越えられない | 同種の hook | 変更しない |

出典(Anthropic, "Sandboxing" と "Configure permissions"、<https://code.claude.com/docs/en/sandboxing>、
<https://code.claude.com/docs/en/permissions>、取得 2026-10-27、本文を逐語で引く形で取得して読んだ)。
Codex 側の公式な設計指針は、概要ページ(<https://learn.chatgpt.com/docs/security>)しか取得できず、**未確認**。

Claude Code の sandbox が起動に要るコマンド（`socat`、Linux は `bubblewrap`）は、対話の Claude では `home.packages` で PATH に載る。
systemd の user unit のような headless の起動の PATH には `~/.nix-profile/bin` が無く、`failIfUnavailable = true` が効いて
`sandbox required but unavailable: … socat not installed` で起動を拒否する（2026-10-28、メールの自動仕分けの unit が 10 分ごとに失敗）。
そのため `home/modules/claude.nix` は、同じ一覧から導いた `dotfiles.claudePermissions.sandboxBinPath`（nix store の bin を `:` で
つないだ文字列、読み取り専用）を出す。headless で `claude` を起動する unit は、これを `Environment=PATH=…` に連結する。
unit の実値は private wrapper に置く（ADR-0034）。

`excludedCommands` の既定は、sandbox の中では鍵束の D-Bus・gpg-agent・nix daemon のソケットに届かない
コマンド(`gh`・`git commit`・`git push`・`git add`・`git fetch`・`git pull`・`nix build`・`nix eval`・`nix flake`・`nix fmt`)である。外で動くので、
狭い allow と分類器を通る。

**除外パターンの照合規則**(Anthropic, "Sandboxing"、取得 2026-10-27): 呼び出し 1 行全体のコマンドがすべて
除外パターンに合わないと、全体が sandbox の中に残る。パターンは呼び出しの文字列に対して合わせる。`cd`・
ファイルへのリダイレクト・`$(…)` を含む呼び出しは sandbox に残る。さらに実測(下の表)で、**末尾だけに `*` を
持つパターンだけが効き**、途中に `*` を持つ `git -C * commit *` は、リテラルのパスでも効かなかった。このため
git は worktree の cwd で `-C` なしに呼ぶ。`git -C *` のような広い除外は、全ての `-C` の呼び出しを外に出すので採らない。

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
| `git rebase --force-rebase HEAD~1`(使い捨てのブランチ、worktree の cwd。除外に無かった) | 失敗(`gpg: skipped "…": No secret key`、`failed to write commit object`) | `commit.gpgsign=true` で、書き換えたコミットの再署名が sandbox 内の gpg に届かない |
| `git fetch origin`(除外に `git fetch *` を足す前。worktree の cwd、単体、2026-10-28 に別のセッションで測定) | 失敗(`fatal: could not read Username for 'https://github.com': No such device or address`) | `fetch` も認証情報を gh の keyring 経由で読むので、sandbox の中では届かない。`git fetch *` と `git pull *` を除外に足した(`git push` と同じ理由。`git fetch` を sandbox の外で再実行すると成功した。`git pull` は `fetch` を内部で呼ぶので同じ経路からの推定で、未測定) |
| `nix build nixpkgs#hello --no-link`(除外に `nix build *` を足す前) | 失敗(`Read-only file system`、`cannot create Unix domain socket`) | `nix` は除外に無かった。socket の作成自体が sandbox に拒否される(#1050) |
| `nix build nixpkgs#hello --no-link`(`nix build *` を足した後) | **成功** | 除外が効く |
| `cargo --version`・`$TMPDIR` への書き込み | 成功 | sandbox 内で確認なしに通る |

自動モードの分類器は、実測の全呼び出しを許可した(sandbox を外す再実行はしていないので、「Safety Bypass」の
問題は、除外コマンドで外の実行になるこの構成では起きなかった、という範囲で確かめられた)。
署名が要る書き換え系の `git rebase *`・`git merge *`・`git cherry-pick *`・`git revert *`・`git tag *` も、除外に足した
(`git rebase` は上の表で実測した失敗。残りは同じ理由からの推定で、`hms` 後に測る)。
この結果に基づき、既定に `git add *`・`nix build *`・`nix eval *`・`nix flake *`・`nix fmt *` を足した(`git -C *` の
3 件は効かないと分かり、外した)。連結(`git add . && git commit`)を cwd で単体測定する項目が残る(#1164)。`nix` を sandbox の中に残す案(`allowWrite` で `~/.cache/nix`、socket の許可)は、socket の作成自体が
拒否されるので採らない(`allowAllUnixSockets` が要り、範囲が広すぎる)。
`/tmp` 直下への書き込みは `allowWrite` を広げず、`$TMPDIR` を使う運用のままにする。

**自前の道具の書き込み先(2026-10-28、vega、#1164)**: hook の指示でエージェントが Bash で呼ぶ道具は、sandbox の中で
動く。状態の置き場が書き込み許可に無いと、`Read-only file system` で失敗する。実測で確かめた失敗は次のとおり。
`worklog add`(vault の journal)、`session-retrospective --retro-add`(`~/.local/state/claude/wrapup/retro`)、
`issue-inbox --check-dup`(台帳の snapshot)。hook そのものは Claude Code が sandbox の外で動かすので影響しない。
直し方は、`excludedCommands`(全権限で動く)ではなく、道具が所有する場所だけを `sandbox.filesystem.allowWrite` に宣言する:
`~/.local/state/claude`(`claude.nix`)と、vault(`obsidian.nix` の `vaultPath`。vega だけが宣言する)。
`dotfiles.claudePermissions.allowWrite` は各モジュールが足した値の和になる。

宣言の仕組み・キーは [claude-permissions](../claude/claude-permissions.md#sandbox自動モード秘密パスも宣言する2026-10-27) に書く。

## 旧共有管理を撤去する

既存手段: `agent-permissions` — 拡張。既存の transaction journal の変更前・変更後を使う
`retire` を追加し、別の移行ツールは作らない。通常の activation はこの撤去を native writer より先に行う。
手元で変更候補だけを見る場合は、次の順で行う。

```sh
agent-permissions retire --home "$HOME" --dry-run
agent-permissions retire --home "$HOME"
```

`--home` は撤去対象ホームの絶対パス。dry-run は変更候補を JSON で返し、書き込まない。
実行は現在の所有記録からたどれる journal だけを使い、取り消された移行は除外する。
証明できる共有変更だけを逆適用し、現在値が記録後の値と違う箇所は本人の編集として保持する。
後から加えた許可や個別 MCP の許可も保持する。完了は transaction と読み戻しで確かめ、
撤去済みの印を残す。同じホームへ再実行しても共有設定を作り直さない。

journal が無い場合は保守的に扱う。共有管理の名前付き selector とその profile の撤去だけを対象にし、
由来を証明できないローカル設定・project 設定・rules は削除しない。
未完了の transaction がある場合は、その回復を先に行い、撤去で上書きしない。
旧 `inspect` / `migrate` / `apply` / `check` は互換 CLI の入口として残るが、通常実行や activation の前提ではない。
旧 promotion queue は自動で再適用しない。必要な許可は `/promote-permissions` から native 宣言として提案する。

### 撤去が戻せないものと、終わらせる条件(#1158)

撤去は 1 回で終わる設計のままにする。理由: 再実行しても、journal が無くなった変更は戻らない(journal は
`transaction.rs` の `KEEP_JOURNALS = 5` で完了のたびに古いものから消える)。失敗は無言にせず、dry-run と
実行の出力の `retained` に「由来を証明できず戻さなかったもの」を理由つきで並べ、`queued_rules` に待ち行列の
件数を出す。

- 最初の移行が書いた Claude の `sandbox`・`autoMode.classifyAllShell`・Read の秘密パス deny は、journal が
  消えていても戻らない。これらは撤去に頼らず、`home/modules/claude.nix` の宣言が直接合わせる(#1165)。
- 待ち行列の許可(`agent-permissions-pending.jsonl`)は、`agent-permissions pending` で一覧し、
  `pending --restore-rules ID,ID|all` で元の Claude 設定(`.json` の `permissions.allow`)へ戻して待ち行列から
  外すか、`pending --drop ID,ID` で外す。元のファイルが無い・`.json` でないものは戻さず、待ち行列に残して
  理由を出す(消えた project の設定を作り直さない)。
- `home/modules/codex.nix` の `retireAgentPermissions`(全ホストの activation のたびに走る)を外す条件:
  3 ホストすべてで撤去済みの印があり、各ホストの `retained` を本人が確認し、待ち行列が空になったとき
  (確認は #1104)。外すときは、この活性化と `retire` モジュール、旧 CLI を同じ変更で消す。

## 承認経路は upstream のコードに基づいて区別する

以下は OpenAI, *Codex*, `rust-v0.164.0`、commit
`6aaac694803004a63c6f393d153c96d22521a6b9` のコードを読んだ範囲である。
公開日未確認、取得日 2026-10-27。

- 承認要求は PermissionRequest hook の allow / deny を先に適用し、判定が無いときに reviewer へ進む。
  `auto_review` を宣言しても既存 hook の拒否を迂回しない。
  [`tools/approvals.rs` L484–505](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L484-L505)。
- reviewer は guardian が応答すればその結果を採り、応答が無い経路は user へ進む。
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
`agent-native/claude-permissions.json` と `agent-native/codex-permissions.json` を全バイト比較する。
両宣言が存在し、読み取れ、一致するときだけ自動適用する。変更・欠落・読み取り失敗時は
適用せず通知し、本人の差分確認と手動の `hms` を待つ。共有管理の `migrate` は要求しない。
詳細は [適用手順](../operations.md) の main 自動追従を参照する。
