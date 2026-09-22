# リポジトリライフサイクル統制(visibility / license / triage / consolidation)

ADR-0023 により、別の私設ポートフォリオ管理リポジトリ(PRIVATE)から
dotfiles へ正本を移管した文書。「あるリポジトリを生かす/畳む/消す」を
判定する基準と、畳む/統合するときの手順を定める。`github-audit`
(ADR-0015)がガバナンス **drift** を検査するのに対し、本文書はリポジトリの
**ライフサイクル**(存続判定)を扱う — 両者は別の層であり、本文書は
`github-audit` を置き換えない。

RFC 2119 の "MUST"・"MUST NOT"・"SHOULD" の語義に従う。

## Visibility 方針

- 新規リポジトリは **private** をデフォルトとする。public 化は都度の
  明示的な判断であり、デフォルトではない。
- 研究成果物(論文・研究ノート・未発表原稿)は、第三者の権利(共著者・
  研究室・出版社)が確認できるまで private を維持する。公開手順は
  下記「将来の公開」節に従う。
- `.github` リポジトリのデフォルト community health files は、そのリポジトリ
  自体が public のときだけ有効になる。private のままなら無効という状態を
  維持するのも、public化するのも、どちらも明示的な判断として扱う。

## License 方針

- public リポジトリは明示的な `LICENSE` ファイルを持つ。Rust プロジェクトは
  `MIT OR Apache-2.0` をデフォルトとし、それ以外はエコシステムの慣行に従う。
- private のテーマ monorepo(「vessel」、下記「Consolidation」参照)は
  README にライセンス意図を明記する(典型例: 「all rights reserved; 公開時に
  LICENSE を定める」)。将来の公開が既知の状態から始められるようにするため。
- `LICENSE` はデフォルト community health file として供給できない —
  各リポジトリに個別に置く。

## Triage 基準(Maintain / Archive / Delete)

すべてのリポジトリは次の 3 状態のいずれか 1 つに帰着する。

- **Maintain** — 能動的に保守・整備されている: README・LICENSE(または
  明記されたライセンス意図)・再現可能なビルド/実行手順を備える。
- **Archive** — 下記「Deprecate-then-archive チェックリスト」を経て
  read-only にする。休眠しているものの **デフォルト判定**。private
  リポジトリの維持コストはゼロに近く、archive は可逆(unarchive はいつでも
  可能)で、archived リポジトリは一覧からフィルタで隠せる。
- **Delete** — GitHub の 90 日復元猶予を過ぎると不可逆。Delete は次に
  限定する:
  - 中身が空のリポジトリ、
  - 再利用価値のない使い捨て実験、
  - 別リポジトリに完全に重複している内容(テーマ monorepo に吸収済みかつ
    履歴自体に独立の価値がない場合)。

判断に迷ったら archive を選ぶ。

### Delete 前の安全網

delete の前に、必ずミラー bundle を作成し検証する:

```sh
git clone --mirror https://github.com/<owner>/<repo>.git /tmp/<repo>.git
git -C /tmp/<repo>.git bundle create ~/archives/github-bundles/<repo>.bundle --all
git clone ~/archives/github-bundles/<repo>.bundle /tmp/<repo>-verify
```

検証用 clone が成功しない限り delete を実行しない。

### Ledger 規律

統廃合 round の実行中は、判定の執行(リポジトリ・判定・根拠・実行日)を
その round の作業記録(計画書や tracking Issue など、round ごとに定める器)
に記録する。新しい記録用リポジトリを都度立てる必要はない。

## Deprecate-then-archive チェックリスト

archive すると **description と topics も read-only になる** ため、
以下の順序は必須。[GitHub OSPO の archiving ガイド](https://github.com/github/github-ospo/blob/main/docs/archiving-public-repositories.md)
に基づく。

1. **Description** — archive の前に更新する:
   - 後継リポジトリがある場合: `DEPRECATED — ` を先頭に付け、後継名を書く。
   - 単に休眠している場合: 事実に基づく記述のままでよい(prefix 不要)。

   ```sh
   gh repo edit <owner>/<repo> --description "DEPRECATED — absorbed into <successor>"
   ```

2. **Topics** — トリアージのメタデータを追加する:

   ```sh
   gh repo edit <owner>/<repo> --add-topic archived-2026
   ```

   吸収されたリポジトリには、後継テーマを示す topic も追加する
   (例: `absorbed-into-<successor>`。50 文字/小文字の topic 制約に収まる
   よう短縮する)。

3. **README バナー** — 後継のあるリポジトリには先頭に追記する:

   ```markdown
   > [!WARNING]
   > This repository is deprecated and read-only.
   > Successor: <https://github.com/owner/successor> (see its PROVENANCE.md).
   ```

4. **open な Issue/PR** — すべて close する。外部利用者がいる場合のみ、
   移行案内用の Issue を 1 件 pin して残す。

5. **Archive**:

   ```sh
   gh repo archive <owner>/<repo> -y
   ```

archive 自体は可逆(unarchive はいつでも可能)。手順 1–2 だけは archive 後
だと(unarchive しない限り)やり直せない。

## Consolidation(テーマ monorepo への統合)

多数の休眠リポジトリを少数のテーマ monorepo(「vessel」)へ統合し、
保守面を縮退させる手順。

### 方式: snapshot + PROVENANCE.md

- 吸収元の **HEAD 時点のファイル状態のみ** を取り込む。git 履歴は
  取り込まない — 由来情報はデータとして残し、吸収元自体は delete ではなく
  archive されるため復元可能性は保たれる。
- 各 vessel は `PROVENANCE.md` を持ち、次を記載する:
  - 統合日と手法の記述、
  - 表: サブディレクトリ → 吸収元リポジトリ URL → 最終 commit SHA、
  - 取り込み時に行った調整を記す Notes 節。
- ビルド成果物やエディタの残留物は取り込まない: `a.out`、`*.dSYM`、
  `.DS_Store`、`*.bak`、`#*#`、`__pycache__`、TeX 中間ファイル(`*.aux`、
  `*.log`、`*.out`)など。

### Vessel の整備水準(grooming floor)

- README: 研究・テーマの内容、成果物の説明、サブディレクトリの対応表。
- ライセンス意図を明記(上記「License 方針」参照)。
- 再現可能なビルド/実行手順: 数値計算コードは現行ツールチェーンで最低限
  ビルドが通ることを目指す(コンパイラの置換が必要なら PROVENANCE の
  Notes に記す。例: `ifort` → `gfortran`)。TeX は latexmk でビルドが通る
  ことを目指す。

### 吸収元の処理

vessel が検証(PROVENANCE の SHA が吸収元の HEAD と一致し、ビルド手順が
通る)を通過した後、吸収元は上記「Deprecate-then-archive チェックリスト」
で処理する — 常に archive であり、delete しない(履歴を復元可能に保つ)。

### 命名規則(vessel)

テーマ monorepo は研究テーマ名で命名し、ツール名や時代区分では命名しない
(良い例: 研究対象そのものを指す名前。悪い例: 「◯◯時代の研究一式」の
ような時代区分名)。標準ディレクトリ構成は次に従う:
`simulations/`、`analysis/`、`publications/`、`notes/`、`seminars/`。

`<技術>-lab`(特定技術の使い捨て実験。休眠したら archive/delete の第一
候補)の種別語判定は ADR-0020 の閉語彙(`config/github-audit/
descriptive-species.tsv`)に委ねる — 本文書は独立の命名規則を持たない。

## 将来の公開

デフォルトでは非公開のまま保留する。vessel を public にする用意ができたら:

1. 第三者の権利(共著者・研究室・出版社)を確認する。
2. `CITATION.cff` を追加する(GitHub が "Cite this repository" ボックスを
   表示する)。
3. [Zenodo GitHub 連携](https://help.zenodo.org/docs/github/) を有効化し、
   リリースを切る → concept DOI + version DOI が発行される。
4. README に DOI バッジを追加し、以降の保守予定がなければ archive する
   (DOI と citation box は archive 後も残る)。

## 適用範囲外(github-audit との棲み分け)

- `github-audit` の 6 ドメイン(rulesets / charters / naming / settings /
  renovate / titles、ADR-0015・ADR-0031)は、既存リポジトリのガバナンス
  drift を検査する。
  本文書はそれとは別の問い(このリポジトリはまだ必要か)を扱う。
- リポジトリの dormancy を機械的にスコアリングする機構は現時点で
  存在しない(旧 Rust CLI の当該機能はコードごと退役)。必要になれば
  `github-audit` にドメインを追加するかを別途検討する。
