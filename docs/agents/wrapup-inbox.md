# wrap-up inbox — スコープ外の気づきを、vault のノートに溜めて、あとでまとめて裁定する

現行の正準入口は `session-wrapup start|stop`。保存と裁定の入口は `issue-inbox`、振り返りは
`session-retrospective`、期限付き試行は `feedback-trial` が担当する。
以下の旧 `wrapup-stop-gate` / `wrapup-session-start` による説明は、引き続き使える互換入口と移行の経緯を示す。
実装の配置と引数は[責務一覧](agent-libraries.md)を参照する。決定の記録は
`docs/adr/1136-wrapup-inbox-to-vault.md`(起草中の仮番号。PR 作成後に `scripts/adr-number-check --fix` で改番される)。

作業中に出た「今回のタスクのスコープ外だが記録する価値のある気づき」が揮発する問題への
対処。**収集**(セッション中に inbox へ溜める)と**裁定**(本人が `/backlog-cleanup` でまとめて決める)を
分離する。**Issue にするのは裁定で選んだものだけ**で、Stop は起票を求めない。以前は Stop が毎セッション、
inbox の全行を Issue にさせていたため、些細な気づきの Issue が増えて、大事な開発タスクが埋もれた
(ADR-1136 の背景)。

inbox に流すのは、現在進行中の変更と依存関係(`stacked-pr` スキル §1 の判定条件)が
無い気づきに限る。依存があれば stacked PR の追加提案段として受ける選択肢が先に
出るため、この文書が扱うのは「stack せず inbox に落ち着いた」気づきの以降の経路
(SessionStart 注入・保存・裁定)である。振り分け自体の設計根拠は
`docs/agents/stacked-pr.md`「スコープ外発見を stack の一段として受ける入口」を
参照。

| 部品 | イベント | 役割 |
|------|----------|------|
| `wrapup-session-start` | SessionStart | 「気づきは inbox に `--add` で追記せよ。Issue にはしない」を `additionalContext` で注入。未裁定の件数も掲示 |
| `wrapup-stop-gate` | Stop | jsonl に残った行を vault へ移す。更新した feedback memory に `#N` が無ければ指示する。**気づきの起票は求めない** |
| `/backlog-cleanup` | 手動 | vault の open な気づきを集め、本人が裁定し、直せるものは PR で直す(スキルは[`backlog-cleanup.md`](backlog-cleanup.md)) |

配備は `home/modules/agents.nix`(スクリプトは `home.file`、`~/.claude/settings.json`
への登録は activation 時の冪等 jq マージ)。全ホスト共通。vault の場所の宣言は
`home/modules/obsidian.nix` が置くマーカー 1 つ(下の「行き先」)。

## 行き先: vault のノート、vault の無い機体では jsonl

`--add` は、vault を宣言している機体では vault のノートに書き、宣言していない機体では従来どおり
機体の jsonl に溜める。**どちらも Issue にはしない**。機体によって Issue が増えたり増えなかったりしないよう、
規則は全機体で同じにしてある。

vault の宣言は hub-resolve の `OBSIDIAN_VAULT`(環境変数、無ければ
`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/obsidian-vault` の 1 行目。worklog と同じ単一の正本)で、
実値は `home/modules/obsidian.nix` の 1 か所だけにある(ADR-0034)。宣言があるのに読めないとき
(パスが無い、`.obsidian/app.json` が無い)は、理由を 1 行出して jsonl に退避する。黙って捨てない。

全機体を vault に寄せることは**未実装(意図あり)**で、いまは vault を持つ機体だけが対象である
(後続の Issue に残す)。

### ノートの形

気づき 1 件 = vault の `inbox/<題名から作った名前>.md` 1 枚。ファイル名はリンクに使えない文字
(`\ / : * ? " < > | # ^ [ ]` と制御文字)を `-` にし、150 バイトを超えるときは文字の境界で切って
末尾に ` ~<hash>` を付ける(`vault-note::note_name`)。同じ題名は同じノートになる
(同じ名前でも `title` と `repo` が違えば、別のノートにする)ので、重複が構造的に起きず、**本文の
行数がそのまま再発の回数**になる(ADR-543 が昇格の合図にしている「同じことの繰り返し」。以前は
既存 Issue へのコメントの数で数えていた)。

```
---
title: "<題名>"
repo: "owner/repo"
status: "open"        # open | fixed | issued | dropped
issue: ""             # issued にしたときの URL
pr: ""                # fixed にしたときの URL
first_seen: "YYYY-MM-DD"
signal: "copy-drift"            # 任意。as-code 化の候補のときだけ(以下 4 欄。ADR-1276)
mechanism: "unrepresentable"    # 次から防ぐ手段(5 段)
existing: "拡張:pipeline-map"   # 採用|拡張|自前
drop_reason: ""                 # dropped にしたときの閉じた理由(下)
---
- YYYY-MM-DD <detail>
```

- `signal`・`mechanism`・`existing` は、retro の `codify` などの行(`--retro-add`)が `--add` に構造のまま渡す。
  空の欄はノートに書かない(旧い気づきのノートの形は変わらない)。`signal` は語彙
  (`config/agents/codify-signals/vocabulary.json`)の id だけを受け付ける。再発しても最初の値を保つ。
- `signal` を持つノートを `dropped` にするには、閉じた理由 `--reason`(`semantic-judgment` / `unobservable` /
  `preference` / `one-off` / `existing-suffices` / `misread-signal`)が要る。ノートの `drop_reason` に残り、
  再発して `open` に戻ると消える。兆候ごとの採用・却下と理由は、日次の較正が数える(段 6)。

- 再発は本文に 1 行足す(`O_APPEND`)。同じ日に同じ内容の行は足さない(冪等)。
- `fixed` か `dropped` のノートに再発の行が付くと、`status` を `open` に戻す(直したはずのものが
  再発した合図)。`issued` は動かさない(Issue 側で扱う)。書き直しは、状態が変わるときだけで、
  一時ファイル + rename(`vault-note::Locked::replace`)。内容が同じなら書かない。
- 値は JSON の二重引用符文字列で書く(YAML としても正しい)。Obsidian の Properties で引用符なしに
  書き直されても読める。キーの順と、知らないキーは保つ。
- 排他ロックは vault の**外**(`<state>/vault.lock`)に置く。rename でノートの inode が変わるので、
  ノート自体にかけた flock では排他にならない。

Obsidian の Bases(core plugin、https://obsidian.md/help/bases 、取得 2026-10-27)で、
`status` ごとの一覧を作れる。

## なぜ SessionEnd ではないのか

SessionEnd で何かを起票させる案は、ドキュメント上の制約で成立しない:

- `SessionEnd` は decision control を持たない。副作用専用で、exit 2 でも何もブロック
  できず、stderr はユーザーにしか見えない。
- `SessionEnd` の hook は短い時間予算を共有する(設定で引き上げても上限 60 秒)。

(この制約は、気づきを Issue にしなくなった今も、jsonl の行を vault へ移す処理を SessionEnd に
置かない理由になる。移す処理は SessionStart・Stop・`--migrate` で走り、冪等である。)

## 設計判断

| 論点 | 決定 | 根拠 |
|------|------|------|
| スコープ | user グローバル(`~/.claude/settings.json`) | 気づきの記録はプロジェクト非依存。home-manager 配備で全ホストに自動展開 |
| 粒度 | ターン単位・session_id フィルタなし | 気づいた直後の記録が最も文脈が濃い。残骸はどのセッションでも自己修復的に回収される |
| 収集指示の経路 | SessionStart `additionalContext` 注入 | グローバル CLAUDE.md を store symlink にすると `#` メモリ追記が壊れる。skill は受動的ルールの担い手として発火が不確実 |
| 起票 | しない(裁定は `/backlog-cleanup`) | 毎セッションの起票は些細な Issue を増やし、本当に大事な開発タスクを埋もれさせた。Issue にするのは、本人が選んだ大物・本人の作業が要るものだけ |
| `go: ask` の行 | `go` は読まない | 起票前の確認を行に持たせる必要が無くなった(裁定は `/backlog-cleanup`) |
| 縮退 | vault 無し・使えない → jsonl。gh 不在 / git repo 外 / GitHub remote 不在 → feedback の指示を出さない | ADR-0005 の binary-existence gating。記録は残り、条件が揃う環境・セッションで回収される |
| jsonl のスキーマ | 最小 JSONL `{"ts", "title", "detail"}`、任意で `repo` | vault の無い機体の保存形式。題名の無い行は vault に書けず jsonl に残る |
| jsonl の置き場所 | `${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/<slug>.jsonl` | user グローバル機構が任意のリポジトリ(会社リポジトリ含む)の作業ツリーに未追跡ファイルを生やすのは誤コミットリスク |
| slug のキー | 原則リポジトリ単位(`remote.origin.url` の正規化)。remote なし・git repo 外はプロジェクト絶対パスの `/` `.` → `-` 置換にフォールバック | 旧・絶対パス slug は同一リポジトリでも worktree ごとに inbox が分散し、worktree 削除後は orphan 化していた。remote URL 単位に正規化すれば worktree 間で inbox を共有できる |
| repo(ノートの `repo`) | 行の `repo` → 作業中プロジェクトの `origin` の `owner/repo` → jsonl のファイル名(slug) | 同じ題名が別のリポジトリで出ても、別のノートにする |

## inbox の整合性

書き込みを LLM の自由編集に任せると、重複・失敗行の誤削除・並行セッションとの競合を検証できない。
そこでリスクのある操作をすべて `issue-inbox` の決定論的サブコマンドに寄せ、`cargo test` で回帰テストする:

- `--add <inbox> <json1行>` — JSON 検証 + 追記。vault を宣言していればノートへ(上記)。書けなかった行は
  jsonl に退避する(`mkdir -p` + flock 追記)。初回利用時のディレクトリ不在で沈黙する事故を防ぐ。
- `--list-open [--repo R] [--json]` — 状態が open の気づきを、再発の行数の多い順に出す。vault が無い
  機体では jsonl の行を同じ形で出す。vault の絶対パスは出さない。`/backlog-cleanup` の唯一の収集口。
  `--json` には `signal`・`mechanism`・`existing`(無ければ空文字)も出る。
- `--set-status <title> <fixed|issued|dropped> [url] [--repo R] [--reason R]` — ノートの frontmatter だけを
  書き換える(本文はバイト単位でそのまま残す)。jsonl の機体では、その行を取り除く(`<inbox>.filed.jsonl` に
  退避)。`--reason` は `dropped` のときだけで、`signal` を持つノート(as-code 化の候補)では必須。jsonl の
  機体では行を取り除くので、理由の記録先が無い(較正は vault の機体だけで効く)。
- `--check-dup "<title>" [repo]` — `gh issue list --search` で同名 open Issue を検査する。
  exit 1 = 重複(一致した Issue 番号を stdout に 1 行出す)、exit 3 = 判定不能。Issue を作る前に、
  `/backlog-cleanup` と promotion-detect が使う。gh は 10 秒でタイムアウトし、プロセスグループごと打ち切る
  (exit 3。#1129)。Stop で起票を求めていたころの「失敗した照会のあと、変化の無い行を保留する」台帳
  (`--check-dup --inbox --session-id`、`lookup_failure`)は、保留する相手の指示が無くなったので削除した。
- `--mark-filed <inbox> <json1行>` — jsonl の**行全体の完全一致で先頭 1 行だけ**削除
  (`ts` は一意性を保証しないため削除キーにしない)。flock + tmp+mv。削除した行は
  `<inbox>.filed.jsonl` に tombstone として追記してから mv する(#297)。vault の無い機体の消化手段。
- `--migrate <project-dir>` — 旧 slug の inbox の回収(下の節)と、jsonl に残った行の vault への移行。

jsonl の flock は `--add` と `--mark-filed` の両方が取る。vault 側は `vault-note` の
`<state>/vault.lock` が排他する。どちらの経路も、jsonl のロックを取ってから vault のロックを取る順に
固定してあり、逆順で取る経路が無いのでデッドロックしない。

削除したサブコマンド: `--procedure`(起票の手順書)、`--chores-begin` / `--chores-end`
(`/backlog-cleanup` の実行中に Stop の起票指示を黙らせる印。#820)。Stop が気づきの起票を
求めなくなったので、黙らせる対象が無い。呼ぶと「削除された」と断って exit 1 する。

## slug のリポジトリ単位化と自己修復マージ

worktree ごとに絶対パスが変わる旧 slug 方式では、同一リポジトリでも inbox が
worktree の数だけ分散し、worktree 削除後は誰も読まない orphan として残り続けた。
`repo_slug()` は `git config --get remote.origin.url` を正規化して
`<host>-<owner>-<repo>` 形式の slug を作る(scheme・認証情報・`.git` suffix・
大文字小文字・https/ssh/scp 表記の差を吸収)。remote が無い、または git repo 外
なら従来の絶対パス slug にフォールバックする。

過去に書かれた旧 slug の inbox を回収するため、`--migrate <project-dir>`
サブコマンド(Stop hook 本体と `wrapup-session-start` の双方が毎回呼ぶ)が
自己修復マージを行う。安全性はこのリポジトリの inbox 整合性モデル(上記)を壊さないことを
最優先に設計している:

- 使用中になり得る `<inbox>.lock` は削除・再作成しない(#297)。unlink すると、既にその lock を
  open/flock 待ちしている別プロセスとの相互排他が壊れる。orphan のまま残る lock ファイルは空なので
  実害はない。新旧両方の lock を取るのはこのマージだけで、全呼び出し箇所が「新 → 旧」の同一順序で
  取得するためデッドロックしない。
- 空ファイル判定もロック取得後に行う(空判定直後に並行 `--add` が書いた行を消さないため)。
- 新ファイルへは append のみ(truncate/rewrite する経路を持たない)。dedup は
  行全体の完全一致。
- 旧の全行が新に存在することを確認できたときだけ旧本体を削除する。1 行でも欠ければ残して次回
  呼び出し(次セッション)に再試行を委ねる(冪等)。

移行後も、旧パスを埋め込んだ SessionStart 注入を持つ長命セッションが
リテラル旧パスへ `--add` して旧ファイルを再作成することがあり得るが、
次にどのセッションの hook が起動しても `--migrate` が毎回走るため自己修復される。

jsonl から vault への移行(`drain_to_vault`)も同じ契機で走る。**そのプロジェクトの** jsonl の行だけを
移す(別のリポジトリの jsonl は、そのリポジトリでセッションが始まったときに移る。jsonl の
ファイル名から owner/repo を逆算できないため)。読めない行と vault に書けなかった行は jsonl に残し、
移した行は `<inbox>.filed.jsonl` に退避する。

## 検証

- `cargo test -p wrapup-stop-gate -p issue-inbox -p vault-note -p session-wrapup` — vault のノート
  (`vault_inbox.rs`)・縮退・`--add`/`--mark-filed`/`--check-dup`・SessionStart 注入の回帰テスト
  (CI の `ci.yml` でも実行)。テストは `OBSIDIAN_VAULT` と `XDG_CONFIG_HOME` を消した一時環境で走り、
  実機の vault に書かない。
- 手動 E2E は vault を宣言した機体で `issue-inbox --add` して `inbox/` にノートができること、新しい
  セッションの SessionStart 注入に未裁定の件数が出て、ターン終了時に Stop が起票を求めないことを読む。

## feedback 型 auto memory の経路(#328)

wrap-up inbox は「今回のタスクのスコープ外だが記録する価値がある気づき」だけを対象にしており、
ユーザーから受けた作業方針上のフィードバックを Claude Code の auto memory
(`~/.claude/projects/*/memory/*.md`、frontmatter `metadata.type: feedback`)に保存するケースは
対象外だった。実測(2026-09-23)では、ローカル auto memory の `type: feedback` ファイル 17 件中
14 件が Issue 化されずローカルに閉じたままだった。

`wrapup-stop-gate` の同じ Stop 本体に、inbox とは独立した検査を足してある: 今セッション中に更新された
`type: feedback` メモリで `#N`(Issue 番号)参照が無いものを検出して、Issue 化を促す。**今は Stop が出す
指示は、この 1 つだけ**である(気づきの inbox は指示を出さない)。「今セッション」の境界は
`wrapup-session-start` が touch する stamp ファイル
(`~/.claude/wrapup-stop-gate/feedback-session/<session_id>.stamp`)の mtime を基準に `find -newer` で
判定する。stamp が無い(SessionStart 未実行など)場合は判定不能として何もしない側に倒す(ADR-0005 と
同じ fail-open)。

原則そのもの(不可視なローカルメモに閉じ込めない)は共有 AGENTS.md、
auto memory 固有の配線は `config/claude/CLAUDE.md` に持つ。

## 判定レッジャーからの自動集約行(ADR-478)

`verdict-escalate`(`crates/verdict-escalate`)が、`bleep` のような自作
ガードレールの判定レッジャー(`agent-verdicts/*.jsonl`)をセッション単位で
集約し、同一 fingerprint(ツール・reason_id・match_class・マッチ語の
ハッシュ)で閾値(既定 3 回)以上 deny/ask された場合に inbox へ 1 行を
追記する。詳細は[ADR-478](../adr/478-agent-verdict-ledger.md)と
[`verdict-escalate.md`](verdict-escalate.md)を参照。

この経路が書く行は、最小スキーマに `repo`(任意、`"owner/repo"`: ノートの `repo`)と
`go`(任意、`"ask"` 固定)を足す。`go` は、以前は「起票前に人間の明示的な GO が要る」印で、Stop の
指示が AskUserQuestion で確認させていた(ADR-478 D6)。気づきが起票されなくなったので、
`go` は読まない — 裁定は `/backlog-cleanup` で行い、「収集・集約は自動、Issue にするのは人間が
選んだもののみ」は同じ形で保たれる。

## 消化経路

溜まった気づきは、[`backlog-cleanup` スキル](backlog-cleanup.md)が `--list-open` で集め、再発の
回数の多い順に本人が裁定する。直せるものは Issue を経ずに stacked PR で直し(`--set-status fixed`)、
大物と本人の作業が要るものだけが Issue になる(`--set-status issued`)。捨てるものは `dropped`。
**ノートは直接編集しない**(読み書きは `--add` / `--list-open` / `--set-status` を介する)。

## 状態と再開の規則(語彙 JSON)

気づきの状態と事象は `config/agents/wrapup-inbox/vocabulary.json`(`object: "inbox-finding"`)に書く(ADR-0000、書式は
[`state-vocabulary.md`](state-vocabulary.md))。語彙は実装の記述で、実装を駆動しない。コードの挙動は変えていない。

| 状態 | 終端 | 意味 |
|---|---|---|
| `open` | いいえ | 本人の裁定を待つ。`/backlog-cleanup` が再発の多い順に見せる |
| `fixed` | いいえ | 直した。**再発すると `open` に戻る**(閉じているが、再発を待つ) |
| `dropped` | いいえ | 捨てた。**再発すると `open` に戻る**(`drop_reason` は消える) |
| `issued` | はい | Issue にした。以後は Issue が追い、**再発しても `open` に戻らない** |

- 再開: `fixed`・`dropped` に再発の行が付くと `open` に戻る(事象 `reopen`、機械が行う)。再発を別の項目にはしない
  (同じ題名は同じノートで、本文の行数が再発の回数)。`issued` は、行は足すが状態を動かさない。
- 失効の状態は設けない。放置された `open` は、セッション開始時の hook(`session-wrapup start`、`node:session-wrapup-start`)が
  「未裁定の気づきが N 件ある」と出す(事象 `remind`)ことで再通知する。これが、裁定待ちの項目の性質 P1 を満たす手段
  (対応表 `config/agents/vocabulary/refinements/inbox-finding.json`)。数えるのは作業中のプロジェクトの `open` だけで、
  別のプロジェクトの `open` は、そのプロジェクトでセッションを始めるまで再通知されない。
- `--set-status` は現在の状態を見ずに状態を書く(`issued` から `open` へも書ける)。語彙の `fix`・`file`・`drop` は
  `open` からの遷移として書いてあり、これが想定する使い方である。

### 語彙と実装の照合の方法

`cargo test -p issue-inbox`(`vault.rs` の `vocabulary_matches_vault_rules`)が、語彙を `include_str!` で読み、実装の
実際の動きから導いた集合と突き合わせる: 状態は `STATUSES`、`--set-status` の行き先は `fix`・`file`・`drop` の `to`、
再発で `open` に戻る状態は `add_item` を実際に動かして得て `reopen` の `from`、終端は戻らない状態(`issued`)だけ。
語彙だけ、または実装だけを変えるとテストが落ちる。

## 待ちターンでは出さない(#854)

バックグラウンド処理の完了を待つターンでは、retro の指示も feedback の指示も出さない。仕組みと限界は
[`retro.md`](retro.md) の「待ちターンでは出さない」に書いた。

## 再発の記録(重複を捨てない)

同じ気づきが何度出ても再発回数がどこにも残らないと、ADR-543 が散文 → script → gate への昇格の合図に
している「同じ規範違反が繰り返し起きる」が見えなくなる(以前の手順書は、重複を黙って捨てていた)。
ノートの本文の行数が再発の回数である。`--list-open` が回数の多い順に返すので、`/backlog-cleanup` は
繰り返されているものから裁定する。日次 timer で自動検出する仕組みは持たない(繰り返し見落としが
起きたときに、ADR-543 の昇格型に従って検討する)。先行例は Google SRE Workbook ch.10
"Postmortem Culture"(https://sre.google/workbook/postmortem-culture/、取得 2026-10-02)で、
action item に追跡番号を持たせ、繰り返しを傾向として見る。
