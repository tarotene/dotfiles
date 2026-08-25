# plan-view — プランを HTML にして Chrome の専用窓へ飛ばす

Plan モードから抜けるとき、プランは端末内のプレーンテキストとして読まされる。この
repo のプランは 1 万字前後（`~/.claude/plans/` の実測で 161 本、最大 3 万字）あり、
`codex-plan-review` gate が deny するたび書き直された版を読み直すことになる。長文を
未レンダリングのまま端末で読むのが苦痛だという、体験そのものの問題への対処。

| 部品 | 役割 |
|------|------|
| `config/claude/hooks/plan-view.sh` | hook / CLI / `--selftest` を兼ねる 1 本 |
| `config/claude/hooks/plan-view.css` | 見た目。pandoc の `--include-in-header` に `<style>` で包んで渡される |
| `config/claude/commands/plan-view.md` | `/plan-view`（執筆中のプランを手動で飛ばす） |
| `~/.local/bin/plan-view` | hook を持たないエージェント・素のシェルからの入口 |

配備は `home/modules/claude.nix`（スクリプトと CSS は `home.file`、`settings.json` への
登録は activation 時の冪等 jq マージ）。変換器は `home/modules/packages.nix` の
`pandoc`。全ホスト共通。

## LLM を呼ばない

**構造の再解釈はしない。** 入力の Markdown を 1:1 で HTML に写すだけである。フェーズや
依存関係を図やカードに再構成する案（プラン本文を LLM に読ませて毎回 HTML を生成する）
は採らなかった。痛みの本体は「未レンダリングであること」であって「構造が見えない
こと」ではなく、毎回のトークンコストと待ち時間に見合わない。

結果として、この hook は決定論的で、ゼロトークンで、ExitPlanMode のたび必ず動く。

## なぜ plan-review gate と独立なのか

`codex-plan-review` gate と同じ `PreToolUse` / `ExitPlanMode` matcher に、**別エントリ
として**登録する。Claude Code は同一 matcher の hook を並列に走らせるので、plan-view は
review（最大 300 秒、deny あり）の結果を待たずに窓を開く。

つまり **deny されて書き直されるプランも描画される**。それを承知で独立を選んだ:

- gate の pass 経路の末尾から呼ぶ案は、既存 hook に責務が増え、fail-open 経路（codex
  不在・skip・タイムアウト・上限到達・スキーマ不適合）の各々に呼び出しを入れ忘れる
  リスクを背負う。表示という無害な機能のために、gate の縮退経路を全部見直すのは筋が
  悪い。
- 独立していれば、review が壊れていても skip されていても必ず描画される。
- 窓は毎回新規なので、途中の版を見てしまった分は閉じるだけで済む。

## 承認フローに干渉しない

hook モードは **成否に関わらず stdout に何も出さず `exit 0`** する。これが唯一の
不変条件であり、selftest が全経路で守っている。

`PreToolUse` hook が stdout に JSON を出すと `permissionDecision` として解釈される。
plan-view は表示するだけの道具なので、承認を許可も拒否もしてはならない。プラン本文が
取れなかった / pandoc が無い / 画面が無い、いずれの場合も黙って何もしない。

エスケープハッチは 2 系統（ADR-0005 の binary-existence gating と同じ流儀で、認証情報
ではなく存在だけを見る）:

```
touch ~/.claude/plan-views/skip     # 恒久的に止める
PLAN_VIEW_SKIP=1 claude             # このセッションだけ止める
```

`skip` は保持期限の掃除から除外する。消えると無効化が黙って解除されるため。
なお **CLI 経路（`plan-view` / `/plan-view`）はハッチを見ない** — ハッチは
ExitPlanMode の自動発火だけを止める。

## ブラウザは非同期で起動しなければならない

`PreToolUse` hook は同期実行である。ブラウザを前景で起動すると、既存 Chrome プロセスが
無いホストでは Chrome 本体が hook を掴んだまま居座り、**承認ダイアログが出ない**。

```bash
setsid "$BROWSER_BIN" --app="$url" --window-size="$WINDOW_SIZE" \
  >/dev/null 2>&1 </dev/null &
disown
```

`setsid` が無い環境では `&` + `disown` に落とす。検証項目 9（実際に Plan モードを抜けて
「窓が開き、かつ承認ダイアログも出る」）はこの罠の回帰テストである。

## なぜ `--app` の専用窓なのか

`--app=file://...` はタブバーも URL バーも無い窓を開く。通常タブにすると、この環境では
Chrome の既存窓にプランのタブが積み上がっていく（Chrome は同一 `file://` URL の既存
タブを再利用しない）。専用窓なら普段の browsing 窓を汚さず、読み終わったら `Ctrl+W`
で閉じるだけで済む。

「1 セッション 1 窓に集約して `meta refresh` で追従」案は採らなかった。ユーザーがその
窓を閉じたあと、マーカーが残っていて再オープンされず以降見えなくなる縮退があり、
状態を持たない毎回新規のほうが確実だと判断した。

`~/.nix-profile/bin/google-chrome` は既に nixGL ラップ済み（`home/modules/desktop.nix`、
ADR-0006）なので、`xdg-open` を経由せず直叩きしても GL は壊れない。

## なぜ pandoc なのか

`--standalone` 一発で、シンタックスハイライト（skylighting）・表・脚注・リストまで
JS ゼロで出る。軽量な `cmark-gfm` はトークン単位のハイライトを出せず、ブラウザ側で
`marked.js` + `highlight.js` を同梱する案は JS アセットの同梱と `file://` での動作保証を
抱え込む。

`--embed-resources` は使っていない。CSS を `<style>` に包んで `--include-in-header` で
渡せば単一ファイルになり、pandoc のバージョン差（2.x の `--self-contained` との
フラグ名の違い）に影響されない。

## CSS の順序依存とライトテーマ

**skylighting のハイライト色は pandoc が固定で吐く。** `--highlight-style` は 1 つしか
選べず、メディアクエリでは切り替わらない。そこで:

- 基底 = `breezeDark` を前提にしたダーク配色（hook は `--highlight-style=breezeDark`）
- `@media (prefers-color-scheme: light)` で背景・本文と **ハイライト色の両方** を上書き

ライト側で `code span.*` を上書きしないと、breezeDark の keyword / operator
（`#cfcfc2` = ほぼ白）が白背景で完全に読めなくなる。実測した既知の欠陥であり、
`plan-view.css` のライト節を削るとそこに戻る。

**上書きはクラスごとの色だけでは足りない。** skylighting は個別クラスのほかに 2 つの
基底色を置く:

```
code span { color: #cfcfc2; }        /* Normal — クラスの付かない span */
div.sourceCode { color: #cfcfc2; }   /* span の付かない素のテキスト */
```

この 2 つを外すと、`--arg c` の `c` や nix の `{ pkgs, ... }` のような素のトークンだけ
が白背景で消える（クラス付きのキーワードや文字列は読めるので、パッと見では気づき
にくい）。ライト節の先頭でまとめて `var(--fg)` に落としている。個別クラスはクラス
セレクタの分だけ詳細度が高いので、この基底規則には潰されない。

### CSS に HTML の終了タグを書いてはいけない

CSS は style 要素の中に埋め込まれる。ブラウザは style 要素の中では CSS のコメント構文
を解釈せず、`</style` の並びを見た時点で要素を閉じる。**コメント内に書いただけでも、
そこから下の CSS 全部が本文に漏れて文字として表示される。** 実際にこのファイルの
解説コメントに終了タグを書いて一度壊した。

このバグは grep ベースの検査では捕まらない（文字列は「存在する」ので全部通る）。
selftest は 2 段で守っている:

- `plan-view.css` に `</style` の並びが無いこと
- 生成 HTML の最後の `</style>` が `<body` より前に来ること（原因が何であれ漏れを検出）

上書きが成立するのは pandoc の `default.html5` の出力順に依っている（pandoc 3.1.3 で
実測: 組み込み CSS + skylighting がドキュメント 8 行目、`header-includes` が 233 行目）:

```
<style> 組み込み CSS + skylighting の色 </style>   ← 先
$for(header-includes)$ … $endfor$                  ← 後（plan-view.css はここ）
```

selftest はこの順序を、双方に固有のマーカー（skylighting が付ける `/* Keyword */` と
自前 CSS だけが定義する `--measure:`）の行番号比較で守っている。`code span.kw` を
目印にすると自前 CSS 側の上書き規則にも当たって空振りするので、そこは戻さないこと。
順序テストのフィクスチャにはコードブロックを必ず含める — 本文にハイライト対象が無いと
pandoc は skylighting の CSS を一切出さず、テストが空振りする。

## プラン本文とタイトル

本文の取得は既存 `codex-plan-review.sh` と同じ 3 段:

1. `tool_input.plan`
2. `tool_input.planFilePath`
3. `~/.claude/plans/*.md` の最新

実測では ExitPlanMode で 1 と 2 の**両方**が来る（`plan_chars: 7245`,
`has_plan_file_path: true`）が、片方だけのケースに備えて 3 段で落とす。

タイトルは本文の最初の h1、無ければファイル名（プラン 161 本のうち 160 本が h1 を
持つ）。**フェンスの内側は見ない** — プランは bash ブロックを多用し、その中の
`# コメント` が h1 と同じ形をしているため、素朴な `grep` だとコードのコメントが
タイトルになる。`#!/usr/bin/env bash` は `#` の直後に空白が無いので誤認しない。

`<title>` は `--metadata pagetitle=` で設定する。`--metadata title=` にすると pandoc が
`title-block-header` を描き、本文の h1 と二重になる。

メタ行（リポジトリ名 · 時刻）は h1 の直下に raw HTML の `<p class="plan-view-meta">`
として差し込む。**h1 との間の空行は必須** で、詰めると gfm リーダが raw HTML を独立
ブロックとして扱わず h1 の続きに飲み込む。

## 生成物の衛生

生成物はプラン本文そのものを含むため、既定 umask に任せない: `umask 077`、
`~/.claude/plan-views/` を 0700、ファイルを 0600。保持期限は既定 30 日
（`PLAN_VIEW_RETENTION_DAYS`）。

## 自動発火が Claude Code だけである理由

| エージェント | 介入点 | 結論 |
|---|---|---|
| Claude Code | `PreToolUse` / `ExitPlanMode` に `tool_input.plan` が来る | 自動発火 |
| Codex CLI | hooks はあるが **plan モードの概念自体がない**（`--plan` なし、exit-plan イベントなし）。`hooks.json` は `config.toml` の `hooks.state.trusted_hash` で trust 管理され、書き換えるたび再 trust が必要 | 手動 CLI のみ |
| Devin CLI | plan 面が皆無（ACP サーバモードのみ） | 手動 CLI のみ |

そのため汎用の `plan-view` CLI を下層に置き、自動発火は Claude Code にだけ配線した。

## スコープ外

- **ブラウザ側での承認** — hook は人間の入力を待てない。承認は端末で行う
- **mermaid 等の図のレンダリング** — 機械変換に限定した（上記「LLM を呼ばない」）
- **`path:line` のエディタジャンプ** — この環境に GUI エディタが無く（`vim`/`nvim`
  のみ）、`vscode://` 的なスキームが成立しない
- **ライブリロード / 1 窓集約** — 毎回新規窓を選んだ（上記「なぜ `--app`」）

## 検証

```bash
bash config/claude/hooks/plan-view.sh --selftest
shellcheck -e SC1091 -S error config/claude/hooks/plan-view.sh
nix flake check
nix build .#homeConfigurations."$(hostname)".activationPackage --no-link
home-manager switch --flake .#"$(hostname)" -b backup
```

適用後:

```bash
# ExitPlanMode のエントリが 2 本並び、既存の codex エントリが無傷であること
jq '.hooks.PreToolUse' ~/.claude/settings.json

# 手動経路
plan-view ~/.claude/plans/<slug>.md
plan-view --no-open --out /tmp/p.html ~/.claude/plans/<slug>.md

# 無効化
PLAN_VIEW_SKIP=1 claude
```

`home-manager switch` を 2 回連続で走らせても `settings.json` のエントリが増殖しない
こと（冪等マージ）も確認する。
