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
設定が読めない、不正な構文、未知の権限キー、所有記録の破損、設定元の増減は
実行を止める。可変ユーザー設定は権限部分を比較し、モデル選択や hook 信頼の
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

## 更新を中断したら記録から復元する

更新前の全バイト列を、本人のみ読み書きできる transaction journal に保存する。
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
| D1: 正本を home-manager に置く | dotfiles commit `8279bc6`、`crates/settings-reconcile/src/managed.rs` | 所有記録を権限へ拡張。軸: 表現不可能 — 単一正本 |
| D2: native sandbox を使う | OpenAI, Codex [rust-v0.164.0 permissions_toml.rs L112](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/permissions_toml.rs#L112)、[exec_policy.rs L443](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/exec_policy.rs#L443)、公開日未確認 | 明示 allow は sandbox を迂回しうるので生成しない。軸: 還元 |
| D3: 既存ライブラリと router を拡張する | dotfiles `managed.rs` / `router.rs`、OpenAI [rules 読込 L668](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/exec_policy.rs#L668) | 別の認可エンジンや CLI fork を作らない。権限検査の失敗だけ拒否。軸: 還元 |
| D4: 未配備の基盤、配備の順に PR を積む | dotfiles [ADR-0027](adr/0027-uncertainty-first-stacking.md) | 既存のセッション内チェーン規約と一致。軸: 還元 |
| D5: 共有設定を残し、境界外は一回の審査にする | Anthropic, [Permissions](https://code.claude.com/docs/en/permissions)、[Sandboxing](https://code.claude.com/docs/en/sandboxing)、公開日未確認 | Read/Edit/WebFetch の native 意味と hook ask を利用。機体管理設定だけの統制は採らない。軸: 還元 |

既存手段: `crates/settings-reconcile/src/bin/agent-permissions.rs` — 拡張: `crates/settings-reconcile`。
Claude の本体コードは非公開。固定版の根拠は Anthropic の
[v2.1.285 changelog](https://github.com/anthropics/claude-code/blob/v2.1.285/CHANGELOG.md#L3152)
と隔離試験で、現在の公開ドキュメントにある新しい挙動まで確認済みとはしない。

## 要求と実装の対応

| ID | 要求・合意 | 実装箇所 |
|---|---|---|
| R1 | 「そういえば Claude ではどうやってましたっけ？」 | 本文と既存 `docs/settings-reconcile.md` |
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
