# ADR-1400 — skill を「手で打つ入口」と「エージェントが選ぶ知識」の 2 層に分け、入口だけに `my-` を付けて、それ以外を `/` の一覧から隠す

- Status: Accepted
- Date: 2026-11-08
- Issue: No-Issue(本人の依頼「スキル名が記憶に残らず、習慣として行うべきことが『どれだっけ』になる」から始まった作業で、対応する Issue は無い)
- Amends: ADR-1293(D2「名指しできない skill は統合の候補」の統合を、この ADR の段で `backlog-cleanup` と `issue-hygiene` について行った。D1〜D8 の決定は変えない)

## Context

2026-11-08 に、過去の会話記録(`~/.claude/projects/**/*.jsonl` の `<command-name>`)で、本人が手で打った skill の回数を数えた。

- `/grill-me` 116 回、`/wrapup-chores` 32 回(最後は 10/30)、`/mail-triage` 17 回(別リポジトリの skill)。ほかは 0〜3 回。
- 10/27 の改名(#1040。名前を責務どおりに揃えた)で `wrapup-chores` は `backlog-cleanup` になった。改名後の `/backlog-cleanup` は 0 回。
- 手で打たれる skill は数本で、残りの 30 本あまりは、エージェントが文脈から選ぶ側である。`/` を打つと、自作 33 本と旧名の転送口 11 本、第三者製、組み込みが並ぶ。

つまり、「名前は責務を端的に説明する」(`docs/agents/tooling-conventions.md`)という基準は、エージェントが選ぶ skill には合うが、本人が毎日手で打つ名前には、覚えにくいという害を出した。習慣の名前が、改名で切れた。

あわせて、責務の重なりも見つかった。`backlog-cleanup` と `issue-hygiene` は、同じ「open Issue 全件」を読み、停滞 Issue の扱いが食い違っていた(片方は裁定を経て閉じ、片方は「ユーザーの明示判断なしに閉じない」)。互いを参照していなかった。ADR-1293 D2 は、同じ対象の同じ遷移を担う skill を統合の候補とし、統合は後の段で行うとしていた。

## Decision

### D1: skill を、使われ方で 2 層に分ける

- **入口**: 本人が習慣として手で打つ skill。数本に絞る。
- **知識**: エージェントが文脈で選ぶ skill。本人は名前を覚えなくてよい。

層ごとに、名前の基準と `/` の一覧への出方を変える。知識の層の名前は、従来どおり責務を表す。

- 本命: 層を分け、名前の基準も層ごとに変える。
- 対抗馬: 全 skill に同じ命名規則を当てて、数を減らす案。表現不可能性: 手で打つ名前と、エージェントが選ぶ名前は、読み手が違う(前者は本人の記憶、後者は description の照合)。同じ基準では、片方に害が出る(今回は前者)。本命が勝つ。
- 既存手段: 拡張 — `docs/agents/tooling-conventions.md` に節を足す。新しい仕組みは足さない。

### D2: 入口の名前は `my-` で始める

`my-` に、「いつ打つか」で覚えられる語を続ける(`my-desk`・`my-cleanup`)。覚えるのは接頭辞 1 つで、`/my` と打てば入口だけが一覧に出る。将来、入口が増えても、同じ接頭辞の下に並ぶ。

接頭辞は `/my-` の形で、`/morning` のように組み込みの skill(`anthropic-skills:morning`)と衝突する名前を避ける。

### D3: 入口でない skill を、Claude の `/` の一覧から、ビルド時に名前だけから導いて隠す

`home/modules/agents.nix` が、`my-` で始まらない skill の `SKILL.md` に、frontmatter の先頭の 1 行として `user-invocable: false` を足して、Claude 用に配る。skill ごとの宣言も、手書きの一覧も持たない。この設定は、`/` の一覧から消し、`/名前` と打っても動かさず、エージェントは今までどおり文脈で選べる。

Codex 用(`.agents/skills`)には足さない。Codex の公式文書には、本人の一覧から隠す設定の記述が無い。

`checks.<system>.agent-skills`(`home/tests/agent-skills.nix`)が、次を検査する。隠した skill の `SKILL.md` は、2 行目の 1 行を除いて元と同じ。入口には足されていない。Codex 用には足されていない。入口が 1 本以上ある(接頭辞の付け間違いで、一覧から全 skill が消えるのを防ぐ)。

### D4: 新しい skill は既定で知識の層。入口にするのは、手打ちの実績があるときだけ

実績は会話記録から数える。入口にするときは、名前を `my-` で始め、`agent-tooling-authoring` の配線チェックリストに従う。他のリポジトリのプロジェクト skill にも同じ規約を当てる。そちらは配布の仕組みを共有しないので、入口でない skill の frontmatter に `user-invocable: false` を手で書く。

### D5: 同じ対象の同じ遷移を担っていた `backlog-cleanup` と `issue-hygiene` を、入口 `my-cleanup` に統合する

ADR-1293 D2 の「統合は後の段」を、この段で行う。取り込み先は `my-cleanup` の §2a(Issue の構造の整理)。親子の操作の正本は `tracking-issue` に置いたまま、複写しない。停滞 Issue は、構造の整理では閉じも束ねもせず、閉じるのは裁定で本人が選んだものだけにする。`github-audit-triage`(全リポジトリの監査結果)と `issue-ref-freshness`(他リポジトリの Issue の状態)は、読む対象が違うので統合せず、知識の層として残し、`my-cleanup` の手順から呼ぶ。

旧名(`backlog-cleanup`・`issue-hygiene`・`wrapup-chores`・`daily-desk`)は、転送だけの互換入口にする。削除条件は `docs/agents/tooling-migration.md` のまま変えない。

## 先行例との対比

- D1: 日常に使う層と、部品として使う層を分け、名前の基準を変える。
  先行例: Scott Chacon・Ben Straub『Pro Git』第 2 版 "Git Internals - Plumbing and Porcelain" https://git-scm.com/book/en/v2/Git-Internals-Plumbing-and-Porcelain (取得 2026-11-08)。日常に使う porcelain と、道具の部品になる plumbing を分けている。
  差分: 異なる — git は両層とも人が打てる。この ADR では、知識の層を人の一覧から消す。
  軸: 表現不可能 — 層ごとに基準を分け、片方の基準を他方に当てて害が出る状態(手で打つ名前が覚えにくい)を、構造から作らない。
- D2: 入口だけに共通の接頭辞 `my-` を付け、接頭辞 1 つを覚えれば一覧が出るようにする。
  先行例: GNU Emacs Lisp Reference Manual "Coding Conventions" https://www.gnu.org/software/emacs/manual/html_node/elisp/Coding-Conventions.html (取得 2026-11-08)。「You should choose a short word to distinguish your program from other Lisp programs. The names of all global symbols in your program … should begin with that chosen prefix.」
  差分: 異なる — Emacs の接頭辞の目的は名前の衝突を避けることである。この ADR では、目的を「接頭辞 1 つを打てば習慣の一覧が出る」想起の手がかりに移す。
  軸: 検出のみ — 接頭辞の付け忘れは、D3 が一覧から隠すので見えて分かる。入口にすべきものを知識に置く誤りは、レビューで見る。
- D3: `my-` で始まらない skill に、ビルド時に `user-invocable: false` を足す。skill ごとに手で書かない。
  先行例: Anthropic "Extend Claude with skills" https://code.claude.com/docs/en/skills (取得 2026-11-08)。「Set to `false` when only Claude should invoke the skill: Claude Code hides it from the `/` menu and doesn't run it when you type `/name`.」
  差分: 一致 — 設定値を、名前から導く点だけを足す。
  軸: 表現不可能 — 単一正本(名前)から導くので、`my-` 以外の skill が一覧に出る状態を作れない。
- D4: 新しい skill は既定で知識の層。入口にするのは手打ちの実績があるときだけ。
  先行例なし: 探した範囲は、このリポジトリの `docs/adr/`・`docs/agents/tooling-conventions.md`・Anthropic の skill 文書(code.claude.com/docs/en/skills)。キーワードは「入口」「entry」「user-invocable」「手打ち」。skill を入口と知識に分けて、昇格の根拠を実績にする規約は見つからなかった(一次情報のみ、取得 2026-11-08)。
  軸: 検出のみ — 実績の有無は会話記録から数えて読む。機械は止めない。同じ誤り(手で打たれない入口)が繰り返されたら、ADR-543 の昇格型で検査への昇格を検討する。
- D5: 同じ対象の同じ遷移を担う 2 本の skill を、入口 1 本に統合する。
  先行例: ADR-1293 D2 docs/adr/1293-state-transition-vocabulary.md (取得 2026-11-08)。名指しできない skill、同じ遷移を担う skill は統合の候補で、統合は後の段で行うとしている。
  差分: 一致 — 今回がその後の段である。
  軸: 還元 — 同じ open Issue を読む 2 つの手順と、食い違う停滞 Issue の規則を、1 つにする。

## 却下した案

- 全 skill に接頭辞を付ける。エージェントが選ぶ skill の名前は、責務を表す方が description の照合に合う。接頭辞を付けても、`/` の一覧が長いことは変わらない。
- 一覧を絞らず、「この場面ならこれ」の索引を起動時に注入する。名前を思い出す必要は残り、索引も更新の対象になる。隠す設定は名前から導けるので、更新する物が無い。
- 旧名 `/backlog-cleanup` に戻す。改名の害は旧名に戻せば消えるが、「習慣として打つ名前は、場面で覚えられる語にする」基準が無いままなので、次の改名で同じことが起きる。
- Codex 用にも、ファイルを足して隠す。公式文書に、本人の一覧から隠す設定が無い。記述の無い設定を足すと、意味の分からない設定が残る。

## Consequences

- 手で打つ skill は `/my-desk` と `/my-cleanup` の 2 本になる。第三者製の `/grill-me`(116 回)は upstream の名前のままで、隠れない。
- 隠した skill もエージェントは選べる。Claude Code が実際に隠すことは、`hms .` のあとに新しいセッションで `/` を打って確かめる(未実測。人の目で読む確認は、Issue に払い出す)。
- Codex の一覧は絞られない。接頭辞で絞るだけになる。
- 旧名を手で打つと、動かない(`user-invocable: false` は `/名前` も動かさない)。旧名の互換入口は、エージェントが旧名を指示文に書いたときの読み替えのために残す。
- 他のリポジトリのプロジェクト skill には、規約を当てる判断を、そのリポジトリ側に任せる。手で打たれている skill(別リポジトリの `/mail-triage` など)の名前と、メール系の重なりの有無は、そちらで裁定する。

## 執行点

- `home/modules/agents.nix`
- `home/tests/agent-skills.nix`
- `docs/agents/tooling-conventions.md`
- `config/agents/skills/agent-tooling-authoring/SKILL.md`
