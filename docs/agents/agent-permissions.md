# Claude・Codex の権限を同じ宣言で管理する

権限の正本は `home/modules/agent-permissions.nix`。`agent-permissions` は
既存の設定を棚卸しし、確認した内容だけを両 CLI の標準設定へ反映する。
対象読者は、この dotfiles を適用する本人と保守するエージェントである。
初回は「移行前に一覧を確認する」までを読み、以後は診断と復元の節を参照する。
基盤追加の段では既定配備を変えない。次段で標準設定と router を接続する。

## 書き込み・読み取り・通信を分けて宣言する

通常の書き込みは現在の作業場所と一時領域に限る。Git の共有メタデータ、
ホーム設定、Nix・Cargo のキャッシュへの書き込みは境界外であり、native の
一回の審査へ進む。恒久的な Bash / prefix allow は生成しない。

通常資料の読み取りは許可する。秘密パスは Claude の Read deny と sandbox、
Codex の filesystem deny に変換する。認証を使う通常操作と秘密値の表示は別扱いにする。
秘密パスの追加は制限の強化、削除は権限の拡大として扱う。

通信先の宣言は接続だけを許可する。投稿・送信に関する既存 hook は存続する。
`domains` の初期値は空で、既知の宛先は本人の宣言に追加する。
非公開の宛先・作業場所は非公開 wrapper の値として渡し、このリポジトリへ書かない。
外部ツールは個別審査とし、MCP の各サーバーと App の既定を prompt にする。

`project_rules.<作業場所>.domains` はプロジェクト限定の通信先である。
Claude の WebFetch は hook が現在の場所と照合する。shell と Codex では
グローバルな通信許可へ昇格させず、native の一回の審査を使う。

## sandbox の中で動く shell は自動で許可する

`sandboxed_shell`(`auto` | `prompt`、既定 `auto`)は、native sandbox の中で動く shell コマンドに
承認を求めるかを決める。`auto` の Claude は `sandbox.autoAllowBashIfSandboxed = true` を生成する。
Anthropic の「Sandboxing」によれば、`false` の間は「All Bash commands go through the regular
permission flow, even when sandboxed」で、`git status` のような読み取りまで毎回確認になる。
`true` でも、sandbox を外した再実行、`Bash(…)` の ask・deny 規則(pkexec の ask など)、
重要パスへの `rm` は従来どおり審査される(Anthropic,
[Sandboxing](https://code.claude.com/docs/en/sandboxing)、取得 2026-10-26)。Codex は
`approval_policy = "on-request"` が同じ意味を持つ(OpenAI,
[Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)、取得 2026-10-26)ので、
生成内容は変えない。

書き込みは作業場所と一時領域、通信は宣言した宛先に OS が制限するので、恒久の allow 規則は増えない。
一方、署名(`~/.gnupg` を sandbox が読ませない。#1050)と、`domains` に無い宛先への通信(git push、gh)は
sandbox の中では動かず、sandbox を外した再実行として一回ずつ審査される。

`prompt` から `auto` への変更は権限の拡大なので、`apply` は拒否し、`inspect` → `migrate` を要求する。
`auto` から `prompt` は縮小なので自動で反映する。この欄が無い記録は `prompt` で審査された
ものとして読む。

**適用の順序に注意する。** 配備された `policy.json` と所有記録の `policy` が違う間は、`assess` が
「policy changed」で失敗し、hook は inert な呼び出し以外の全ツールを拒否する。拡大を伴う変更は、
本人の端末で次の順に、続けて行う。

1. 新しいバイナリと新しい policy のパスを得る。`hms` を当てる前の `agent-permissions` は旧版で、
   `deny_unknown_fields` のため新しい policy の欄を読めない(2026-10-26 に `unknown field` で拒否されるのを実測)。
   以降は、ここで得た新しいバイナリを使う。
   - `nix build .#dotfiles-tools --no-link --print-out-paths`(この出力 + `/bin/agent-permissions`)
   - `nix build .#homeConfigurations.<host>.config.dotfiles.agentPermissions.policyFile --no-link --print-out-paths`
2. 新しいバイナリで `reclaim` を実行し、待機の許可を昇格候補のキューへ退避する。配備済みの
   policy.json と記録が一致している間に行う。これを飛ばすと `migrate` が待機の許可を
   キューに残さず取り除く。
3. 「移行前に一覧を確認する」の `inspect` → `migrate`(`--policy` にそのパス)→ `check` を、新しいバイナリで行う。
4. 直ちに `hms` で新しいバイナリと `policy.json` を配備する。

3 と 4 の間は、配備済みの policy.json が旧版なので、この間も全ツールが止まる。短い間に済ませ、
その間は本人の端末だけで作業する。

## 移行前に一覧を確認する

初回の適用は自動では行わない。まず、build した `agent-permissions` と
Nix が生成した `policyFile` を指定して一覧を保存する。以下の変数には、
当該 build の出力パスを使う。作業中のエージェントが具体値を提示する。

```sh
umask 077
agent-permissions inspect --policy "$policyFile" --project "$PWD" --json > /tmp/agent-permissions-review.json
```

JSON は実ファイルの重複を除いた一覧、設定元、内容の SHA-256、権限部分と
移行方針、生成する標準設定を含む。認証・モデル・接続用の環境変数は一覧へ載せない。
`decisions` の値を次のように読む。

| 値 | 処理 |
|---|---|
| `manage` | 本人の可変設定から恒久 allow を撤去し、正本の境界を設定する |
| `remove` | 本人の Codex rules ファイルを空にする。変更前の内容はバックアップに残る |
| `retain` | 共有・管理設定を変更せず、内容を記録して以後の変更を検出する |
| `review` | 意味や構文が未確認。適用を止める |

`.rules` は実行可能な Starlark なので、部分的なパターン照合では解釈しない。
一覧を読んで撤去を承認した本人のファイルだけ、`review` を `remove` へ変更できる。
共有ファイル・未知の設定構文は、同じ方法で黙って削除しない。
共有設定が sandbox や hook の境界を弱める場合も、残置を承認しただけでは通らない。

撤去・残置・取り込みの一覧を本人が確認した後に、同じ JSON を適用する。
適用前の `migrate --dry-run` は書き込まずに移行方針と内容の一致を検査する。

```sh
agent-permissions migrate --policy "$policyFile" --project "$PWD" --approved /tmp/agent-permissions-review.json
agent-permissions check --policy "$policyFile" --project "$PWD"
```

実行直前に再集計し、一覧とハッシュが一致しなければ何も変更しない。
取り込みによる権限の拡大は正本の変更として別に確認する。
通常の `apply` は記録済み設定の一致を検査し、宣言による制限の強化だけを自動反映する。
通信先の追加、秘密パスの削除、対象プロジェクトの変更は再レビューが必要になる。

## ずれを検出したら設定元から診断する

`inspect` は人向け表示と `--json`、`check` は成功時の JSON に対応する。
`agent-permissions --help` と `agent-permissions <command> --help` で、サブコマンドと
オプション、ずれからの復旧の手順(`inspect --json` → `migrate --dry-run` → `migrate` →
`check`)を表示する。`migrate --dry-run` の各変更には、内容が変わるかを示す `changed` が付く。
`check` はずれた設定元を 1 件目で止めず、全件を 1 行ずつ列挙する。
設定が読めない、不正な構文、未知の権限キー、所有記録の破損、設定元の削除や、
下の節の待機に当たらない増減は実行を止める。可変ユーザー設定は権限部分を比較し、モデル選択や hook 信頼の
更新を誤検出しない。残置した共有ファイルは全文ハッシュも比較する。

所有記録が壊れても `inspect` は診断と一覧を出す。本人がその一覧を確認したときだけ、
`migrate --retire-ownership --approved REVIEW` で記録を再構築する。
破損した記録もバックアップし、確認後に内容が変われば拒否する。
以前の所有範囲を復元できないため、由来が不明になった拒否規則は残す。

棚卸しは Claude の user / project / local、linked worktree の main checkout 側
local、登録済みプロジェクト、plugin と管理設定、Codex の user / project / 全 profile、
rules、MCP / App / plugin の承認設定を対象とする。読めない設定元は無視しない。
`CODEX_HOME` / `CLAUDE_CONFIG_DIR` を別の場所へ変更した起動は未実装(意図あり)で、
通常の設定元を読んだものとしては扱わず拒否する。
CLI 引数による設定の上書き、Codex の enterprise/cloud/MDM の配布経路は、
ローカルファイル一覧だけでは全件確認できない。native の有効設定の読み戻しを
受入試験に含め、未確認のホストで完全な設定元の網羅をうたわない。

実行前検査の拒否時は、本人の端末で専用 CLI を直接実行して診断・レビューする。
任意のシェルコマンドを検査の例外にはしない。未知の設定を取り除く前に
新しい一覧を確認し、移行後は `check` の読み戻し結果を確認する。

## 追記された許可は待機として扱い、構造的なずれだけを止める

Claude Code は「Yes, and don't ask again」と、sandbox の通信先承認のたびに、`settings.local.json`
の `permissions.allow` へ規則を追記する(Anthropic,
[Settings](https://code.claude.com/docs/en/settings)・[Sandboxing](https://code.claude.com/docs/en/sandboxing)、
取得 2026-10-26。実機でも 2026-10-26 に再現した)。これは本人自身の判断なので、全セッションの全ツールを
止める理由にしない(#1043)。`check` は差分を 2 種に分ける。

| 種類 | 条件 | 扱い |
|---|---|---|
| 待機(pending) | 書き込み可能な Claude の設定で、`permissions.allow` への追加だけが差分。追加は次の閉じた語彙に限る | ツールは止まらない。`check` は報告して成功し、`apply` と移行後の読み戻しは拒否する |
| 構造的なずれ | 上記以外すべて(語彙外の規則、deny / ask の変更、`sandbox` など他の欄、未知の構文、共有ファイルの変更、設定元の削除) | 拒否のまま |

待機の語彙: 正確なホスト名の `WebFetch(domain:…)`、`WebSearch`、`*` を含まない `mcp__…` の 1 ツール、
途中に `*` を含まない `Bash(…)`(ワイルドカードは末尾の `:*` か ` *` だけ。#461)。`Edit`・`Write`・
`Read` の許可、裸のツール名は含まない。記録に無い設定ファイルも、空か待機の語彙だけなら新しい設定元の
ずれにしない(プロジェクトで初めて承認を保存すると Claude Code がファイルを作る)。

Codex の承認も同じ考え方で待機にする。Codex は、コマンドの前方一致を「don't ask again」で承認すると
`~/.codex/rules/default.rules` に 1 行追記し、通信先を承認すると同じファイルに `network_rule` を追記する。
MCP ツールの「Always allow」は `config.toml` の `…tools.<tool>.approval_mode = "approve"` を書く
(OpenAI, Codex rust-v0.164.0 `execpolicy/src/amend.rs` L65-L124、`execpolicy/src/rule.rs` L138-L145、
`core/src/mcp_tool_call.rs` L2340-L2489、取得 2026-10-26)。待機にするのは次だけである。

- `default.rules`: 記録した内容の**後ろに足されただけ**で、足された各行が Codex の書く形と完全に一致するもの。
  `prefix_rule(pattern=["…", …], decision="allow")` と、正確なホスト・既知のプロトコル・`allow` の
  `network_rule(host="…", protocol="…", decision="allow"[, justification="…"])`。Starlark は解釈しない。
  形が違う行、deny・prompt の決定、ワイルドカードのホスト、記録した内容の書き換えは構造的なずれ。
- `config.toml`: ツール 1 つの `approval_mode` が、無い状態か `prompt` から `approve` になっただけ(MCP サーバー・
  App・plugin の MCP サーバー配下)。サーバー全体の既定値や他の欄の変更は構造的なずれ。

構造的なずれでも、次の呼び出しだけは通す(`pending.rs` の `inert_call`)。本人に質問し、診断するためで、
任意のシェルは例外にしない。

- `AskUserQuestion`・`ExitPlanMode`(外部に作用しない)
- `agent-permissions` の読み取り専用の呼び出し: `inspect`・`check`・`migrate --dry-run`・`--help`。
  単語だけで構成され、リダイレクト・パイプ・`;`・`&&`・引用符・`$` を含むものは通さない。
  `--restore` と `--retire-ownership` は `--dry-run` より先に作用するので通さない

`disabledMcpjsonServers` は許可でなく制限で、空の一覧は何も許可しない。Claude Code が新しい
ディレクトリで起動するたびに空の欄を足すので、`~/.claude.json` の追跡から外した(以前の記録も同じ形に
そろえて比較する)。有効化された `enabledMcpjsonServers` は許可として追跡を続ける。

hms の activation は、`agent-permissions apply` が失敗しても止めず、警告を出して続ける。失敗で止めると
`set -eu` が後続の段と「現在の世代」の印の付け替えを飛ばし、profile だけが進んだ状態が残る
(2026-10-26 に `generation/reality mismatch` の警告として観測した)。実行時の検査(router)は
記録が許す範囲だけを通し続けるので、続行は安全側である。

## 待機の許可はセッションの境界で回収し、昇格候補に移す

待機の許可が、承認したセッションを越えて残らないようにする。回収は、Claude の設定ファイルからは追加された規則を
取り除き、Codex の `default.rules` は記録した内容に書き戻し、`config.toml` は該当するキーだけを元の値に戻す
(コメントや書式は toml_edit が保つ)。回収は 3 つの契機で走り、どれも冪等である。

| 契機 | 登録 | 確実さ |
|---|---|---|
| SessionStart(`startup`・`resume`・`clear`) | `home/modules/claude.nix` の `agent-permissions-reclaim`(Codex へも同じ宣言から配る) | 確実 |
| hms の activation(`apply` の前) | `home/modules/codex.nix` の `reconcileAgentPermissions` | 確実。`apply` は未回収の許可があると拒否する |
| SessionEnd | 同上(`timeout = 3`) | 最善努力。結果を返せず、時間も短い(Codex は 1〜3 秒に丸められる)ので、これだけには頼らない |

`agent-permissions reclaim` は、待機の許可を昇格候補の**キュー**(`~/.local/state/dotfiles/agent-permissions-pending.jsonl`、
追記専用の行ログ)に追記し、読み戻してから、元の設定ファイルから取り除く。順序が要である: 途中で落ちても
許可が「両方にある」状態にしかならず、「どちらにも無い」状態にはならない。取り除きは移行と同じ transaction
(厳密なバイト列の比較・復元用 journal・読み戻し)を通る。他のセッションが同時に書き換えたら拒否する。
構造的なずれがあるときは、何も回収しない。キューは追記だけなので、並行するセッションの追記が互いを消さない。
取り下げは `drop` 行の追記で表し、書き直さない。

SessionStart は、回収した件数と、キューに溜まった昇格候補の件数を、`[agent-permissions]` で始まる日本語の行でセッションに伝える(ADR-625: `<hook-directive>` の外枠は wrap-up 系だけが使う)。
構造的なずれがあるときは、その全件を伝える(復旧の migrate は本人の端末で行う)。hook は常に成功を返し、
セッションの開始を妨げない。

昇格は `/promote-permissions` が担う(`agent-permissions pending` で候補を読む)。共有ポリシーに宣言があるのは
通信先(`domains`・`project_rules.<project>.domains`)だけなので、昇格できるのは `WebFetch(domain:…)` と、
Codex の `network_rule(… decision="allow")` の候補に限る。`mcp__…`・`WebSearch`・`Bash(…)`・`prefix_rule`・
ツールの `approval_mode` の候補は、ツールを `prompt` に保つ現行の方針のとおり宣言が無く、
キューは記録にとどまる。これらを昇格できるようにするには、共有ポリシーの形を広げる別の裁定が要る(未実装、
意図は未決)。

この仕組みで、規則「個別許可は原則その実行限り」は「そのセッション限り」に緩む(本人の裁定、#1043)。
並行する別のセッションが開始されると、もう一方の許可が回収されて再度プロンプトが出ることがある。

## 更新を中断したら記録から復元する

内容が変わるファイルの更新前後の全バイト列を、本人のみ読み書きできる transaction journal に
16 進の文字列で保存する(版 2)。内容が変わらないファイルは記録しない — 確認済みのハッシュは
書き込みの前後に再検査されるので、復元に要らない。数値の配列で全ファイルを保存していた
版 1 の journal も、引き続き復元できる。完了した journal は新しい 5 件だけを残し、古いものは
`migrate` の成功後に消す。復元したい journal は、次の `migrate` の前に退避する。
各ファイルは同じディレクトリの一時ファイルから rename し、書き込み後にも読み戻す。
symlink を経由した書き込みは拒否する。複数ファイル全体の更新は単一の原子的操作ではないため、
中断を示す `pending` が残ったら実行を拒否する。

`migrate` が表示した journal の絶対パスを、次の `journal` に指定する。
全ファイルが更新前・更新後のいずれかと一致するときだけ復元し、途中の他の編集は上書きしない。

```sh
agent-permissions migrate --restore "$journal"
```

## 先行例と実装上の判断

出典の取得日はすべて 2026-10-26。公開日が確認できなかった資料はその旨を記す。

| 判断 | 一次資料・先行例 | 差分と軸 |
|---|---|---|
| D1: 正本を home-manager に置く | dotfiles commit `8279bc6`、`crates/agent-settings-reconcile/src/managed.rs` | 所有記録を権限へ拡張。軸: 表現不可能 — 単一正本 |
| D2: native sandbox を使う | OpenAI, Codex [rust-v0.164.0 permissions_toml.rs L112](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/permissions_toml.rs#L112)、[exec_policy.rs L443](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/exec_policy.rs#L443)、公開日未確認 | 明示 allow は sandbox を迂回しうるので生成しない。軸: 還元 |
| D3: 既存ライブラリと router を拡張する | dotfiles `managed.rs` / `router.rs`、OpenAI [rules 読込 L668](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/exec_policy.rs#L668) | 別の認可エンジンや CLI fork を作らない。権限検査の失敗だけ拒否。軸: 還元 |
| D4: 未配備の基盤、配備の順に PR を積む | dotfiles [ADR-0027](adr/0027-uncertainty-first-stacking.md) | 既存のセッション内チェーン規約と一致。軸: 還元 |
| D5: 共有設定を残し、境界外は一回の審査にする | Anthropic, [Permissions](https://code.claude.com/docs/en/permissions)、[Sandboxing](https://code.claude.com/docs/en/sandboxing)、公開日未確認 | Read/Edit/WebFetch の native 意味と hook ask を利用。機体管理設定だけの統制は採らない。軸: 還元 |
| D6: sandbox の中で動く shell は自動で許可し、sandbox 外への再実行と ask・deny は審査する | Anthropic, [Sandboxing](https://code.claude.com/docs/en/sandboxing)、OpenAI, [Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)、公開日はどちらも未確認 | auto-allow mode と `on-request` の仕様どおり。`prompt`→`auto` は拡大として reviewed migrate に回す(#1030 の R4 を保つ)。軸: 表現不可能 — 境界を OS の sandbox が強制する |

既存手段: `crates/agent-settings-reconcile/src/bin/agent-permissions.rs` — 拡張: `crates/agent-settings-reconcile`。
Claude の本体コードは非公開。固定版の根拠は Anthropic の
[v2.1.285 changelog](https://github.com/anthropics/claude-code/blob/v2.1.285/CHANGELOG.md#L3152)
と隔離試験で、現在の公開ドキュメントにある新しい挙動まで確認済みとはしない。

## 要求と実装の対応

| ID | 要求・合意 | 実装箇所 |
|---|---|---|
| R1 | 「そういえば Claude ではどうやってましたっけ？」 | 本文と既存 `docs/agents/agent-settings-reconcile.md` |
| R2 | 「両者を同じソースにして一元管理できる状態にしたい」 | Nix 宣言、共通 Policy と native 設定生成 |
| R3 | 「整理・取り込みまで含める」 | inspect / migrate、本人確認後に適用 |
| R4 | 「範囲が広がるときだけ確認」 | apply の拡大判定とレビュー照合 |
| R5 | 「個別許可は原則その実行限り」 | 恒久 allow 空、外部ツール prompt |
| R6 | 「権限設定全体を対象にする」「権限の境界も決め直す」 | 複数設定元の棚卸しと生成 |
| R7 | 「作業場所の外はその都度審査」「通常の資料は読める」「既知の宛先＋個別審査」 | native filesystem/network と hook |
| R8 | 「表せない分は個別審査へ」「一回の本人承認を認める」 | sandbox 外実行の ask、project 通信先の native 審査 |
| R9 | 「蓄積・ずれ・誤操作の防止」 | 所有記録、check、fail-closed の権限検査 |
| R10 | 「プロジェクト固有分も共通管理」 | projects / project_rules、共有ファイルを編集しない |
| R11 | 「未知の許可は提示して止める」 | unknown と review の適用拒否 |
| R12 | 「一覧確認後に境界と審査へ置換」 | 承認済み一覧の再読込とハッシュ照合 |
| R13 | sudachipy を「入れる件を起票すれば良い」 | 依存宣言済みと確認。runner を使わなかった原因を [#1029](https://github.com/tarotene/dotfiles/issues/1029) に記録 |
| R14 | 共有リポジトリの設定は変更せず、本人の環境側で制御する | retain と変更検出、tracked local も保護 |

Reference-Only: #939 — worktree ブランチ作成の自動審査拒否。受入試験用。
Reference-Only: #991 — router 外の許可 hook の意図。既存登録を維持する。
Reference-Only: #1021 — Codex hook 入力の不足。今回解消したとは扱わない。

実装状態の「所有記録」は、適用した権限設定と残置した設定元を記録した本人のローカルファイルを指す。
「native」は Claude / Codex 本体の設定・sandbox・承認処理を指す。

## 検証の限界を残す

対象は許可の蓄積・ずれ・誤操作の防止である。hook を無効にした起動や、
同一ユーザー権限の悪意あるコードの隔離までは保証しない。
共有の Edit / WebFetch allow が shell sandbox の範囲を広げうる場合は、
共有ファイルを消さず shell 実行を一回の確認へ戻す。
Codex の PreToolUse ask は使わず、native profile と PermissionRequest を使う。
macOS 実機と認証付きの全経路は未確認。新しいセッションで有効設定と hook 信頼を読み戻してから、
当該ホストで有効になったと報告する。

## 配備は所有記録の検査と同じ段で接続する

`home/modules/codex.nix` が共通モジュールを import し、既定で有効にする。
Claude の旧 permissions activation は共通管理中には実行しない。
Codex の既存 writer は端末表示だけを担当し、権限の writer と競合させない。
設定全体は symlink にせず、正本の JSON だけを Nix store から配備する。

初回 activation は撤去への承認を推測しない。所有記録が無ければ移行の案内を表示する。
配備された router は記録ができるまで実行を拒否するため、初回はレビューと
一時設定での試験を済ませてから移行・activation を行う。
通常の activation は、既存の hook・モデル・端末設定の更新後に `apply` を走らせる。

router は入力変換前に権限検査を行う。deny、ask、allow の順に集約し、
この検査の panic・10秒の時間切れは拒否する。他の検査の失敗時の扱いは変えない。
native の30秒のhook期限より前に返す設計だが、プロセス全体の強制終了まで
拒否応答を返せるとは保証しない。
Codex の PermissionRequest も、既存の限定 allow 判定より先に権限のずれを検査する。

`/promote-permissions` は正本への変更提案を作る入口となる。実行時の allow 追加や
プロジェクト設定の削除は行わず、個別の操作は原則その実行限りの承認を使う。
