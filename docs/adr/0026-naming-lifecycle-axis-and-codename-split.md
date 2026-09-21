# ADR-0026 — 命名クラス体系にライフサイクル軸を直交させ、naming-codename を分割する

- Status: Accepted
- Date: 2026-09-21
- Issue: #220(design 起票)、grill-me セッションで裁定
- Supersedes: ADR-0014 の 4 クラス体系(Decision 1)。ADR-0014 の他の決定
  (topics を正本にする、形式は機械判定・クラス帰属は LLM ノードが人間に
  委ねる、既存リポジトリの裁定は別セッションで行う)はそのまま存続する。
- Amends: ADR-0020(閉語彙アプローチ)を naming-codename に限定して継続
  適用し、naming-coined には適用しない、と明確化する。ADR-0020 本文は
  immutable のまま、関係だけをここに記録する。

## Context

2026-09-19、`github-audit-triage` の naming ドメイン初回実運用で、
ユーザーから次の指摘が出た(#220 に記録):

> 明らかに期限が存在しそうなものに対して pj- が付いていなかったり、
> study とか research 系のリポジトリにそれと分かる prefix がないのが
> 問題で、そもそもそのあたりのルール設計が無であることがわかった。

grill-me セッションでこの指摘を掘り下げるため、2 系統の分析を行った
(具体的にどのリポジトリを分析したかは ADR-0014 の方針により本 ADR には
書かない — private/company リポジトリ名は dotfiles の成果物に残さない)。

1. **個人記録用リポジトリ(person-state/CV ハブ)の分析**: このリポは
   `naming-codename` topic を自己宣言しているが、実体は ADR-0020 が
   定義する「意味を持たない恣意的ラベル(星座名等の閉じた語彙)」とは
   異なり、他の自作リポとの対を意図した形態素パターンから作られた
   造語(選定時に命名審査パネルを経ている)だった。「無意味な符丁」と
   「有意味だが記述的ではない、著者固有の命名形態論に基づく造語」が
   同一クラスに混在していた。
2. **全リポジトリの傾向分析**: `naming-descriptive` に分類され得る
   archived リポのうち複数件は、「開始・終了日が実質確定していた」
   「対象は自分の知識・学位であって外部成果物ではない」という共通点を
   持つ完了済み研究アーカイブだった。一方、別の複数件は同じ研究・学習系
   だが**進行中**の資格試験勉強であり、「期限はあるが未完了」という
   別の状態を取る。`naming-pj`(`pj-` prefix 固定)は「命名パターン」と
   「時限性」を 1 トークンに縛っているため、この種のリポは
   `naming-descriptive` の受け皿に事後的に流れていた。

これら 2 つの指摘は独立した軸の問題である: (1) は「クラス内の意味的
異質性」、(2) は「クラス体系が時限性という直交する性質を表現できて
いない」。両方を本 ADR で同時に改訂する(grill-me セッションでの裁定)。

## Decision

### 1. `naming-codename` を 2 クラスに分割する

- `naming-codename`: 意味を持たない恣意的ラベル。ADR-0020 の閉語彙
  アプローチ(`config/github-audit/codename-registry.tsv`)を**引き続き
  適用**する。新規採番は登録済みキャストから行う。
- `naming-coined`(新設): 著者固有の命名形態論に基づく、意味を持つ
  造語(Context の分析例が該当)。**閉語彙を持たない** — 各リポジトリ
  ごとに個別の意図を持って作られる造語であり、固定キャストから採番する
  性質のものではない。命名時にその造語がなぜ選ばれたかの根拠
  (形態素パターン・比較審査の記録)を当該リポジトリの ADR 等に残す
  ことを推奨するが、dotfiles 側は強制しない。

これにより命名クラスは 5 種になる: `naming-codename` /
`naming-coined` / `naming-descriptive` / `naming-pj` / `naming-site`。

### 2. ライフサイクル軸を直交トピックとして新設する

`naming-*` の 5 クラスとは別に、次の 2 トピックを**併用可能**な直交軸
として新設する。1 リポジトリに `naming-*` を 1 つ + ライフサイクル
トピックを 0 個または 1 個、両方付けられる。

- `lifecycle-timeboxed`: 外部成果物を持つ時限プロジェクト。`pj-`
  prefix と対応するが、prefix 自体は命名パターン(`naming-pj`)の役割を
  保ち、このトピックは「今なお時限性という性質を持つか」を独立に表現
  する。プロジェクト完了後も `naming-pj` の命名は変えない
  (ADR-0007 の一括リネーム禁止と整合)が、`lifecycle-timeboxed` は
  完了後に外してよい。
- `lifecycle-study`: 研究・学習記録。対象が自分の知識・学位であり
  外部成果物を持たない。完了済み(archived)・進行中いずれの状態も
  取る。`naming-descriptive` や `naming-pj` のどちらとも併用できる。

判定の機械化: `github-audit` の naming ドメインが、`isArchived` +
description 中の閉語彙(`study` / `research` / `seminar` / `exam` /
`coursework` / `thesis` / `graduate` 等、初期集合は ADR-0020 の
`descriptive-species.tsv` の `archive` 追加と同じ手続きで
`config/github-audit/lifecycle-species.tsv` に置く)の一致から
`lifecycle-study` 候補を検出する。`lifecycle-timeboxed` は `pj-`
prefix の有無と `isArchived` の組み合わせで候補を検出する。**候補提示
までが決定論、最終的なトピック付与は ADR-0014 Decision 3 のとおり
`github-audit-triage`(LLM ノード)が提案し人間が裁定する** — 判定
プロセス自体は変更しない。

### 3. 既存リポジトリの裁定は本 ADR の対象外

ADR-0014 Decision 4 を継承する。2026-09-19 時点で GO 済みの既存
naming topic 割り当ては、本 ADR 確定後に再監査で見直す(#278)。

### 4. repo-charter スキルへの反映

新規リポジトリ作成時のインタビュー(命名クラス質問)に `naming-coined`
の選択肢と、ライフサイクルトピックの質問を追加する(#279)。

## Alternatives considered

- **`naming-study` を 5 クラス目として新設**(命名クラスの中に押し込む
  案): ADR-0020 の生成的閉語彙アプローチを素直に踏襲できるが、
  「進行中の資格勉強」のように `pj` 的でもあり `study` 的でもある
  リポジトリの帰属が単一クラスでは表現できない。棄却。
- **`naming-pj` の定義を「完了後も pj のまま」に精緻化するだけ**
  (ライフサイクル軸を新設しない案): `naming-descriptive` に流れていた
  完了済み研究アーカイブを救えない(そもそも `pj-` prefix を持たない
  ため、定義変更だけでは既存の命名パターンと矛盾する)。棄却。
- **`naming-codename` を分割せずサブタイプ topic で表現**
  (`codename-arbitrary` / `codename-coined`): 主クラスの数を増やさない
  利点はあるが、`naming-*` という「1 リポジトリ 1 個」の正本原則
  (ADR-0014 Decision 2)と、サブタイプという新しい階層を導入すること
  の一貫性コストを比較し、主クラス分割の方が既存の運用(1 リポ = 1
  naming-* topic)と整合すると判断し棄却。
- **ライフサイクルトピックを `naming-timeboxed` / `naming-study` と
  命名する**(`naming-*` prefix で統一する案): 見た目の一貫性はあるが、
  「命名クラス」と「ライフサイクル軸」が概念として直交する
  (1 リポジトリに `naming-*` 1 個 + ライフサイクル 0〜1 個、という
  カーディナリティの違いを持つ)ことが prefix からは分からなくなる。
  棄却。

## Consequences

- `github-audit` の naming ドメインが、5 クラスの語彙・パターンに加えて
  `lifecycle-species.tsv` を判定基準として参照する(実装は #278)。
- 既存の `naming-codename` 8 件は、本 ADR 確定後の再監査で
  `naming-coined` への再分類対象になり得る(裁定は再監査時、#278)。
- `repo-charter` スキルのインタビューに新しい選択肢・質問が増える
  (#279)。
- 命名クラス・ライフサイクル軸の語彙・パターンを変更する場合は、この
  ADR を supersede する新しい ADR を起こす(ADR-0008 の規約、ADR-0014
  Decision 4 の継承)。

## Verification

- `github-audit naming --selftest` に 5 クラス + ライフサイクル 2
  トピックの分岐が fixture で検証されること(実装は #278)。
- 本 ADR 自体は docs のみの変更のため `nix flake check` への影響はない。
