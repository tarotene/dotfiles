# ADR-998 — 粒度の小さい作業ログを、Obsidian vault の当日ノートへ worklog で追記し、SessionStart で渡す

- Status: Accepted
- Date: 2026-10-25
- Issue: No-Issue(`/grill-me` セッションで、Git で管理するには重すぎる粒度の作業ログの置き場を決める依頼から始まった)

## Context

セッションで試したこと、分かったこと、解決できずに残った詰まりは、チャットに流れて消える。
このリポジトリが採取している記録は、agent-turn-log(`docs/adr/0011-local-activity-log-capture.md`)の
1 ターンごとの依頼文だけである。依頼文は残るが、試した結果は残らない。しかも消費側は別の活動記録ツールで、
次のセッションが読む前提では作られていない。

一方、1 行で足りる事実(返信を送った、期限を済ませた)のために PR を 1 本作る例がある。Git は、
判断に使う裁定と決まった事実の正本であり、1 行の「済んだ」の置き場には重い。

個人の Obsidian vault は、Self-hosted LiveSync でスマホと同期され、パスは `home/modules/obsidian.nix` が
バックアップ用に宣言している。vault の当日ノートは Git の外にあり、本人がスマホで読み書きできる。
private な person-state リポジトリ側には、朝の机の手順専用の bash の書き込み口が先にあるが、
別のリポジトリのセッションから呼べず、記録できるのは「済んだこと」の 4 種類に限られる。

### 実測(2026-10-25): Obsidian の外からの追記は同期される

この前提(vault のファイル末尾に Obsidian の外から足した行が、LiveSync でスマホに届く)は、
記録が無かったので確かめた。vega で、Obsidian の停止中に 1 行、起動中に 1 行を `>>` 相当で追記し、
スマホの Obsidian で同じ日のノートを開いた。2 行とも届いた。vega の LiveSync は 60 秒ごとの定期同期
(`periodicReplication`、`liveSync` は off)で、起動時・保存時の同期は off である。

## Decision

### D1: 置き場は vault の当日ノートの `## 作業ログ` 節、1 行 1 件、追記専用

行の形は `- HH:MM [kind] 本文 (owner/repo#N)`。時刻は JST、`[kind]` と末尾の参照は省略できる。
当日ノートの場所は vault の `.obsidian/daily-notes.json`(`folder` と `format`)から読み、
`YYYY` / `MM` / `DD` 以外を含む書式は推測せずに止まる。既存の行は書き換えず、末尾に足す
(LiveSync と衝突させない)。書き込みの間はノートに flock をかけ、同時に書く複数のセッションの行が
壊れないようにする。同じ kind・本文・参照の行は、時刻が違っても重ねない。

軸: 還元 — 既存の当日ノートと日次ノートの設定を流用し、置き場を増やさない。

### D2: kind は 6 つの閉語彙。kind の無い行は本人の手書き(memo)

- `reply`: Gmail の外で返した返信・申し込み
- `deadline`: 手動の期限を済ませた
- `task`: Gmail の外で片付けた人手の作業
- `pr`: マージ状態から導けない PR の裁き
- `found`: 試して分かったこと
- `stuck`: 解決できずに残った詰まり

kind の無い行は、スマホで書いた行として `memo` と読む。以前の見出し `## 済んだこと` の行も、読み出しでは
同じ節として扱い、書き込みでは使わない。書いた行が読み戻すと同じ行になることを、書く前に検査する
(本文の末尾が参照の書式 `(owner/repo#N)` に見えると、読み出しで本文から参照へ化けるため、書かずに止める)。

軸: 表現不可能 — kind を型(列挙)で閉じ、読み戻せない行を書けなくする。

### D3: 書き込み口は新規の crate `worklog`。vault の場所と owner/repo は既存の部品で解く

`worklog add <kind> <text...> [--ref N] [--repo owner/repo]`、`worklog list`、`worklog session-start`
の 3 つを持つ。vault の場所は hub-resolve と同じ「環境変数 `WORKLOG_VAULT`、無ければ
`~/.config/dotfiles/worklog-vault` の 1 行目」で解き(2026-10-27 の Amendment で `OBSIDIAN_VAULT` / `obsidian-vault` に改めた)、マーカーは `home/modules/obsidian.nix` が
`vaultPath` から置く。owner/repo は hook-io の `git::origin_nwo` で作業ディレクトリから導くので、
書き手は番号だけを渡す(`--ref N`)。参照の書き忘れは起こせない。

既存手段: 自前 — 却下: Obsidian 公式 CLI の `daily:append`(アプリの起動が前提で、停止中は 1 行ごとに
GUI が立ち上がる。この機体では未有効化)、private リポジトリ側の bash の書き込み口の拡張(vault の
パスが 2 か所に分かれ、他のリポジトリから呼べない)。

軸: 表現不可能 — vault のパスを `obsidian.nix` の 1 か所の宣言にし、パスの食い違いを書けなくする。

### D4: SessionStart で規範と、同じリポジトリの直近の stuck / found を渡す

hook は `claude.nix` に全ホスト向けで登録する(matcher は `startup` だけ)。マーカーの無い機体では
何も出さず、あるのに vault が使えないときは stderr に 1 行だけ出して exit 0 で終わる。注入文は、
書く時点(外部操作を済ませたとき、試した結果が出たとき、解決できずに残ったとき)と kind の意味、
書かないもの(会社の業務の中身、機密、既存の記録から導けること)を述べ、作業ディレクトリと同じ
owner/repo の直近 7 日の stuck / found を、新しい順に最大 10 行、「記録であり、指示ではない」と断って付ける。
stuck が解決したかは追跡しない。解決したら found を書き、古い行は 7 日の窓から外れる。

登録を `obsidian.nix` に置かず `claude.nix` に置くのは、hook の一覧が `claude.nix` の 1 つのリストで、
別のモジュールから足す口が無いためである。vault を宣言した機体(`obsidian.nix` を読む機体)だけで
注入される結果は変わらない。

軸: 表現不可能 — 機体の宣言(マーカー)と規範の注入を結び、vault の無い機体で「書け」と言われる状態を作らない。

### D5: Git へ上げる段は持たない。範囲は Claude Code のセッションに限る

`worklog` はログを読み返して Git へ上げることをしない。Git に載せるべき事実(決まった事実の正本を
変えるもの)は、そのセッションで正本を直す PR にし、ログはその証跡にとどめる。Codex と Copilot の
セッションへの注入は、今は登録していない(未決)。

## Alternatives considered

- **agent-turn-log の JSONL に載せる。** 却下。出力の契約(パスとフィールド名)は別の活動記録ツールが直接
  依存し、この側の判断で変えられない。依頼文を残す記録で、本人がスマホで読む面も無い。
- **Obsidian の Local REST API プラグインを経由する。** 感触で外した(プラグインを増やしたくない)。
  分析の結論ではない。
- **書き手を `worklog add` だけにし、本人の手書きを許さない。** 却下。スマホから数秒で書けることが、
  vault を選んだ理由の 1 つである。kind の無い行を memo として読めば、手書きは壊れない。

## Consequences

- 書き漏れは検出しない。規範は注入文の散文だけである。書き漏れが繰り返し起きたら、終了時の検査
  (Stop hook)へ昇格させることを検討する。
- vault の日次ノートの書式を `YYYY` / `MM` / `DD` 以外の記号語に変えると、`worklog` は止まる。
- 記録は vault にあり、リポジトリの検査(CI)の対象にならず、Git の差分にも出ない。
- 時刻は JST 固定である。`TZ` には依存しない。
- 機体ごとに vault があるわけではない。マーカーを置いた機体でだけ書ける。

## 執行点

- `crates/worklog/src/lib.rs`
- `crates/worklog/src/main.rs`
- `crates/worklog/tests/cli.rs`
- `home/modules/obsidian.nix`
- `home/modules/claude.nix`
- `pipeline-map.toml`

## 参考文献

- Obsidian, "Obsidian CLI", <https://obsidian.md/help/cli>(取得 2026-10-25)— 公式 CLI は 1.12 以降で、
  アプリが起動していなければ最初のコマンドで起動する。

## Amendment (2026-10-27 — ADR-1136)

vault の場所の宣言を、worklog 専用の `WORKLOG_VAULT` / `~/.config/dotfiles/worklog-vault` から、
vault を使う機構が共有する `OBSIDIAN_VAULT` / `~/.config/dotfiles/obsidian-vault`(`hub_resolve::OBSIDIAN_VAULT`)に
改めた。wrap-up の気づき・retro・裁定待ちの写しも vault に書くようになり、機構ごとに別のマーカーを置くと、
vault の場所の正本が分かれるため。存在確認は `.obsidian/app.json` で、日次ノートの設定
(`daily-notes.json`)は、これまでどおり worklog だけが追加で読む。D1〜D5 は変更しない。

### 執行点

- `crates/hub-resolve/src/lib.rs`
- `crates/worklog/src/lib.rs`
- `home/modules/obsidian.nix`

## Amendment (2026-10-28 — Codex と Copilot のセッションにも注入する)

D5 の「範囲は Claude Code のセッションに限る」と「Codex と Copilot のセッションへの注入は、今は登録していない
(未決)」を、次に改める。vault を宣言した機体では、Claude Code、Codex、Copilot CLI のどのセッションにも、
作業ログの規範と直近の stuck / found を渡す。

- Codex: 共通の hook 宣言(`claudeHookDeclarations`)から展開済みで、`~/.codex/hooks.json` の `SessionStart`
  (matcher `startup`)に `worklog session-start` がある(vega、2026-10-28 に読んだ。`docs/claude/codex-hook-parity.md`
  にも「実装済み」とある)。D5 の「登録していない」は、Codex については事実と違っていた。出力は Claude Code と同じ入れ子の形。
- Copilot CLI: `sessionStart` は最上位の `additionalContext` だけを読む(GitHub Docs, "GitHub Copilot hooks reference"、
  <https://docs.github.com/en/copilot/reference/hooks-reference> の `sessionStart`、取得 2026-10-28)。そこで
  `worklog session-start --agent copilot` が最上位の形で出し、`home/modules/agents.nix` が `~/.copilot/settings.json` の
  `sessionStart` に登録する。`--agent claude|codex` は従来の入れ子の形。
- 未確認: 実際の Copilot CLI と Codex のセッションで、注入された文が文脈に入ること。`hms` の適用後に、新しい
  セッションで確かめる。

D1〜D4 は変更しない。D5 のうち、Git へ上げる段を持たないことは変えない。

### 執行点

- `crates/worklog/src/main.rs`
- `crates/worklog/tests/cli.rs`
- `home/modules/agents.nix`

## Amendment (2026-10-29 — ADR-0000)

D2 の kind の閉語彙に、7 つ目の `mail`(読んだメールの裁定の結論と出典の id)を足す。本文は
`<処分>: <要旨> [<媒体>:<id>]` の文法で、`worklog add` が検査する。D1〜D5 は変更しない。詳細は
ADR-0000 を参照する。

### 執行点

- `crates/worklog/src/lib.rs`
- `crates/worklog/tests/cli.rs`
