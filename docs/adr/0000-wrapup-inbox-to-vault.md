# ADR-0000 — wrap-up inbox の気づきを Issue でなく vault のノートに溜め、裁定したものだけを Issue にする

- Status: Accepted
- Date: 2026-10-27
- Issue: No-Issue(`/grill-me` セッションで生まれた設計変更。別リポジトリのセッションが、セッション終わりの Wrap-Up で止まったことが発端)

## Context

wrap-up inbox は、作業中に出たスコープ外の気づきを溜め、Stop hook が**毎セッションの終わりに全行を Issue にさせる**
機構だった(`docs/agents/wrapup-inbox.md`)。次の 2 つが起きた。

- 些細な気づきの Issue が増え続け、本当に大事な開発タスクが埋もれた。open Issue のうち、出自フッター
  「wrap-up inbox」を持つものは、2026-10-27 に 17 件(open 全体で 49 件。直近の chores PR(#1070)が
  42 項目を片付けた直後の数)。
- 気づきを「Issue にする」前提のため、Issue にすべきでないもの(本人だけが裁定する許可の方針、private なリポジトリの
  事情など)が、セッション終わりの Wrap-Up で行き場を失って止まった。別リポジトリのセッションが、
  agent-permissions の昇格待ちのキュー(許可の規則を共有ポリシーへ昇格する候補)を見て、「Issue にするより
  `/promote-permissions` で裁定する方が自然」と報告して止まった。

さらに、retro(ADR-697)は毎回 PR にコメントを投稿しており、PR の購読者にとって迷惑で、private な内容も
公開リポジトリの PR に書く形になっていた。

本人は Obsidian vault(private。スマホと Self-hosted LiveSync で同期。ADR-998)を持っている。

## Decision

### D1: 気づきは vault の `inbox/<題名>.md` に、1 件 1 ノートで溜める。Stop は起票を求めない

- 気づき 1 件 = 1 ノート。同じ題名(同じ repo)は同じノートになり、再発は本文に 1 行足す。行数がそのまま
  再発の回数になり、ADR-543 が昇格の合図にしている「同じことの繰り返し」を、Issue のコメント数でなく
  ノートの行数で数える。
- 状態は frontmatter の `status`(`open` / `fixed` / `issued` / `dropped`)。`fixed` か `dropped` のノートに
  再発が付くと `open` に戻す。
- Stop hook は、気づきの起票を求めない。残るのは、更新した feedback memory に `#N` が無いときの指示だけ。
- `--procedure`(起票の手順書)と `--chores-begin` / `--chores-end`(`/backlog-cleanup` の実行中に Stop の
  起票指示を黙らせる印)は、役目が無くなったので削除する(呼ぶと「削除された」と断って exit 1)。
- `go: ask`(起票前の確認を求める印。ADR-478 D6)は読まない。裁定は `/backlog-cleanup` で行う。

軸: 表現不可能 — 同じ題名は同じファイルになり、重複起票が構造上起きない。Issue の洪水は、起票を
強制する経路が無くなるので起きない。

### D2: 裁定は `/backlog-cleanup`。直せるものは Issue を経ずに PR で直し、Issue にするのは大物と本人の作業だけ

`/backlog-cleanup` は `issue-inbox --list-open` で open なノートを再発の多い順に集め、本人が 1 件ずつ裁定する。
直したものは `--set-status fixed <PR の URL>`、Issue にしたものは `issued <URL>`、捨てたものは `dropped`。
Issue にするのは、chores の PR に入らない大物(設計・ADR が要る)と、本人の作業が要るものだけ。契機は、
SessionStart の「未裁定の件数」の掲示を見た本人が頼むとき(自動では走らない)。

軸: 還元 — 既存の `/backlog-cleanup`(ADR-387)の収集口を差し替えるだけで、新しい裁定の場を作らない。

### D3: vault の無い機体では、機体の jsonl に溜めるだけにする(規則は全機体で同じ)

vault を宣言しているのは vega だけで、altair と arcturus(会社)には無い。宣言が無い、または使えない
機体では、従来どおり機体の jsonl に溜め、起票は強制しない。機体によって Issue が増えたり増えなかったり
しないように、規則は全機体で同じにする。全機体を vault に寄せることは、**未実装(意図あり)**。

軸: 還元 — 既存の jsonl を、vault の無い機体の保存形式としてそのまま使う。

### D4: vault への書き込みは共通部品 `vault-note` に集め、ロックは vault の外に置く

新しい lib crate `crates/vault-note`。vault の解決、一時ファイル + rename による書き換え、追記、排他、
frontmatter の読み書き、ノート名の規則を持つ。書き換えは状態が変わるときだけ、内容が同じなら書かない
(LiveSync の同期を無駄に走らせない)。ロックは、rename でノートの inode が変わっても効くよう、
vault の外(`<state>/vault.lock`)に置く。

軸: 還元 — issue-inbox・session-retrospective・agent-settings-reconcile の 3 つが同じ入出力を
持つので、1 か所にまとめる。

### D5: vault の場所の宣言は、worklog と共有する 1 つのマーカー(`obsidian-vault`)にする

`hub_resolve::OBSIDIAN_VAULT`(環境変数 `OBSIDIAN_VAULT`、無ければ `~/.config/dotfiles/obsidian-vault` の
1 行目)。実値は `home/modules/obsidian.nix` の 1 か所だけ(ADR-0034)。worklog の `worklog-vault` を
これに改める(日次ノートの設定の検査は worklog が別に行う)。

軸: 表現不可能 — vault の場所の正本が 1 つになり、機構ごとに食い違う宣言が作れない。

## Alternatives considered

- **Issue のまま、`wrapup` ラベルで分けて、一覧と索引から外す**: Issue 起票の強制と、private な内容を
  公開 Issue に書けない問題が残る。ラベルは存在しないと `gh issue create` を落とす
  (`docs/agents/wrapup-inbox.md` の旧「スキーマ」の判断)。
- **当日ノートの節に追記する**(ADR-998 と同じ形): 日をまたいだ重複の検出と状態の管理が、全ての日を読む
  形になるので外した(分析による)。
- **retro のように台帳(jsonl)だけに溜める**: 本人の目に触れない。vault なら、スマホでも見え、Obsidian の
  Bases(core plugin、https://obsidian.md/help/bases 、取得 2026-10-27)で状態ごとの一覧が作れる。

## Consequences

- 気づきは GitHub に出ない。Issue の検索・購読・通知には載らない。`/backlog-cleanup` を頼まないと
  裁定されない(SessionStart の件数の掲示が、唯一の合図)。裁定されずに溜まる壊れ方は、`--list-open` が
  再発の多い順に返すことで和らげる。
- vault の無い機体(altair、arcturus)の分は、その機体でしか見えない。
- jsonl から vault への移行は、**そのプロジェクトの** jsonl だけを、セッション開始・Stop・`--migrate` の
  契機で行う。別のリポジトリの jsonl は、そのリポジトリでセッションが始まったときに移る。
- LiveSync と書き込みの共存(Obsidian の起動中に rename で置き換えても競合版ができないか、一時ファイルが
  同期されないか)は、vega の実機で未確認。確認は後続の Issue に払い出す。
- 公開リポジトリの Issue には、private な名前を書けない(ADR-0034)。vault のノートには書ける。

## 執行点

- `crates/vault-note/src/lib.rs`
- `crates/issue-inbox/src/lib.rs`
- `crates/issue-inbox/src/vault.rs`
- `crates/session-wrapup/src/lib.rs`
- `crates/hub-resolve/src/lib.rs`
- `home/modules/obsidian.nix`
- `config/agents/skills/backlog-cleanup/SKILL.md`
- `pipeline-map.toml`

## Verification

- `cargo test -p vault-note -p issue-inbox -p wrapup-stop-gate -p session-wrapup -p worklog -p pipeline-map`
- 実機(vega)で `hms .` を当て、`issue-inbox --add` で vault の `inbox/` にノートができ、ターン終了時に
  Stop が起票を求めないことを読み戻す。
