# ADR-0028 — README banner と第三者素材(repo ライセンスと異なる素材)の同梱規則

- Status: Accepted
- Date: 2026-09-21
- Issue: No-Issue(grill-me セッション中に設計 — 別リポジトリの README への
  いらすとやアイキャッチ追加を機に、repo ライセンスと異なるライセンスの
  素材を README/docs に同梱する規則が未設計であることが判明したため。
  具体名は `docs/claude/public-publish-guard.md` の方針により省く)
- Supersedes: ADR-0016 の Decision 1(README 固定スキーマ)と Decision 3
  (ルート文書 allowlist)を部分的に拡張する。ADR-0016 の他の Decision・
  Alternatives・Consequences はそのまま有効。

## Context

ある private リポジトリの README にアイキャッチ画像(いらすとやのイラスト)を
追加したいという要望が出たが、dotfiles にはリポジトリ本体のライセンス(MIT 等)と異なるライセンス・
利用規約を持つ第三者素材を README/docs に同梱するときの規則が存在しなかった。
ADR-0016 は README の節構成・ルート文書 allowlist を固定したが、画像アセットの
置き場所・banner の位置・帰属注記の書式・素材の利用点数管理については規定していない。

規則が無いまま個別に対応すると、(a) banner の挿入位置が ADR-0016 が固定した
目的 1 文抽出の実装(`extract_purpose`、後述)と衝突する、(b) 素材のライセンスが
リポジトリの LICENSE に飲み込まれて誤認される、(c) いらすとやのような「無償だが
商用は点数制限あり」の素材源で使用点数を後から数え直せなくなる、という 3 つの
具体的な失敗が起きる。

## 一次情報(取得日 2026-09-21、個別調査で確認)

- README banner の定義位置: standard-readme spec (Richard Littauer)
  <https://github.com/RichardLitt/standard-readme/blob/main/spec.md> —
  Title の**上**に optional な Banner スロットを定義している(ADR-0016 の
  一次調査で既に参照済み、取得日 2026-09-18)。
- いらすとや利用規約: みふねたかし,「イラスト素材のご利用について」
  <https://www.irasutoya.com/p/terms.html> — 「個人、法人、商用、非商用問わず
  無料でご利用頂けます」、有償対応の条件は「素材を21点以上使った商用デザイン
  (重複はまとめて1点)」、禁止事項は「素材を主体としたコンテンツ・商品の
  再配布・販売(LINEクリエイターズスタンプ等も含みます)」。著作権は放棄して
  いない旨の明記あり。OSS リポジトリへの同梱そのものへの言及はない。
- サードパーティアセットの分離表示慣行: Apache License 2.0 §4(d) の NOTICE
  ファイル運用 <https://www.apache.org/licenses/LICENSE-2.0#redistribution> —
  配布物のライセンスと別に、含まれる第三者由来コンテンツの帰属表示を分離した
  ファイル/節に置く運用が標準的。
- 目的文抽出の実装挙動: `scripts/github-audit` の `extract_purpose()`(362 行
  付近)は「最初の `^# ` 行より後の最初の非空段落」を目的段落として抽出する
  (`repo-charter` SKILL.md §2 に既存の罠として記載済み、#182)。H1 より前の
  行は対象外。

## Decision

1. **banner はファイル先頭(H1 の直上)に置く。** H1 と目的 1 文の間には何も
   挟まない(ADR-0016 Decision 1 の固定スキーマはそのまま、その直前に任意の
   banner ブロックを許可する形で拡張する)。理由は 2 つ: standard-readme が
   Banner を Title の上に定義していることに加え、`extract_purpose()` は
   H1 より前の行を読まないため、H1 直下に置くキャッチコピー・バッジ・画像は
   目的段落と誤認される(`repo-charter` SKILL.md §2 の既存の罠と同根)。
2. **第三者素材は `docs/assets/` に commit する。** ルート直下への配置と
   外部 URL への hotlink は禁止(ADR-0016 Decision 3 のルート閉集合の精神を
   バイナリアセットに拡張する)。ファイル名は英語ケバブケース。
3. **banner の実装は `<p align="center">` + `<img>` + `width` 指定、alt text
   は英語必須。**
4. **帰属注記は README の `## License` 節の末尾に、素材 1 点につき 1 行で
   置く。** 定型: 「この素材は `<パス>` にあり、`<出所>`(`<素材ページ URL>`)
   によるもので、リポジトリの LICENSE の対象外である。準拠する利用規約は
   `<規約 URL>`」。1 点 1 行にすることで、同一素材源の使用点数が License 節の
   行数を数えるだけで把握できる(いらすとやの商用 20 点制限のような
   点数上限を持つ素材源への対応)。
5. **同梱前に、素材の利用規約から次の 4 点を確認し、確認した規約の URL と
   取得日を帰属注記または当該変更の PR 本文に記録する:** (a) 再配布可否、
   (b) 商用利用の条件、(c) 改変可否(リサイズ・トリミングを含む)、
   (d) 帰属表示の要否。
6. **LICENSE ファイルはリポジトリ自身のライセンス原文のみとし、第三者素材の
   例外はそこに書かない。** 例外は README `## License` 節側で行う
   (Decision 4)。
7. **本規則は README banner に限らず、`docs/` 配下に置く第三者由来の図版・
   スクリーンショット一般に適用する。**

## Alternatives considered

- **banner を H1 の直後(目的 1 文の前)に置く** — standard-readme の
  Banner スロットの慣用位置と一致しない上、`extract_purpose()` の実装が
  H1 直後の最初の非空段落を目的文として読むため機械的に衝突する。棄却。
- **帰属表示を独立した `NOTICE` ファイルに置く**(Apache スタイル)—
  ADR-0016 のルート文書 allowlist(README/CONTRIBUTING/CHANGELOG/LICENSE*/
  AGENTS/CLAUDE の 6 種)に `NOTICE` が含まれておらず、素材点数も通常
  1 リポジトリあたり数点程度で README License 節で十分足りる。allowlist を
  広げるコストに見合わないため採らない。
- **画像を GitHub の `user-attachments` CDN にアップロードして URL 埋め込みに
  する** — リポジトリ本体と物理的に分離され、クローンしたローカルでは
  表示されない。出所管理も弱くなる。棄却。
- **合計点数を明示する集計行を README に追加する** — Decision 4 の
  「1 点 1 行」規則と情報が重複し、行を増やすたびに手動更新が要る。
  行数を数えれば足りるため採らない。

## Consequences

- `repo-charter` スキルの §2(README への反映)に banner ブロックの
  テンプレと「H1 より上に置く」罠を追記し、新設の第三者素材同梱手順を
  §4(ルート文書 allowlist)の直後に §5 として挿入する(既存 §5–§10 は
  §6–§11 に繰り下げる)。
- `github-audit` の charters ドメインは、banner の有無やその位置を強制検査
  対象にはしない(banner は任意要素であり、`extract_purpose()` が H1 より
  前を無視する現行実装のままで両立するため、監査側の実装変更は不要)。
- 発端となった private リポジトリがこの規則の最初の適用例になる — README に
  `docs/assets/` 配下のいらすとやイラストを banner として追加し、License 節に
  帰属注記を置く。

## Verification

- `repo-charter` SKILL.md 改訂後、`grep -rn '§[0-9]'
  config/claude/skills/` で節番号参照が壊れていないことを確認する
  (github-audit-triage SKILL.md の「§2〜3」、ADR-0017 の「§3」は
  ともに §4 より前の節を指しており、本 ADR による繰り下げの影響を受けない
  ことを確認済み)。
- 適用先リポジトリの README 更新後、H1 より前に banner のみが存在し、
  `extract_purpose()` の抽出対象(H1 直後の最初の非空段落)が従来どおり
  目的 1 文になっていることを目視確認する。
