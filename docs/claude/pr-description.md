# pr-description スキル

`config/claude/skills/pr-description/SKILL.md` — PR の本文(Description)を標準
スケルトンで書き、見た目に影響する変更には Before/After 証跡を必ず添える習慣を
持つスキル。強制側は `config/claude/hooks/pr-gate.sh` の `G_visual`(設計は
`docs/claude/pr-gate.md`)。

## 動機

PR の本文がリポジトリごと・セッションごとにまちまちで、何を書けば十分かの基準が
無かった。加えて GitHub CLI(`gh`)が v2.99.0(2026-09-01)で `--attach` フラグを
実装し、`gh pr create|edit|comment` から直接画像/動画をアップロードして本文に
埋め込めるようになった。これを機に、次の 2 点をグローバルな基本戦略として固定した:

- PR 本文の標準スケルトン(課題・解決策・Before/After・検証・要確認)
- 見た目に影響する変更には Before/After 証跡を義務付ける

## スキルとゲートの分担(二層構成)

義務の実効性は「スキル(判断知識)+ pr-gate 判定(強制)」の二層で担保する
(`living-description` と同型 — commit ce6d154)。

- **スキル(`pr-description`)**: 本文スケルトンそのもの、Before の撮り方、面ごとの
  撮影手段、そして「意味的完成」の基準 —— Before と After が実際にペアで揃って
  いること。全リポジトリで有効(判断知識なので発動に allowlist は関係ない)。
- **ゲート(`pr-gate.sh` の `G_visual`)**: 「視覚証跡が本文に存在するか」だけを
  機械的に検査する下限。3 択の OR(画像 / Before-After 見出し配下の fenced code
  block / `No-Visual: <理由>`)。`~/.claude/pr-gate-repos` の allowlist 内(既定は
  このリポジトリのみ)でのみ block する。

この分担は Copilot によるプランレビューで指摘された論点への裁定でもある:
「G_visual は単一の添付画像だけで PASS するため、ペア性を強制できない」という
指摘に対し、ペア強制をゲート側に持ち込むと Before が存在しない正当ケース(新規
追加など)を誤って止める側に倒れるため、機械強制は「証跡の存在」までに留め、
「対比としての十分性」はスキル(LLM の判断)の責務とした。`G_link` が参照先
Issue の実在を検査しないのと同じ設計思想であり、`docs/claude/pr-gate.md` の
G_visual 節にも明記している。

## `gh --attach` の事実(2026-09-05 時点で裏取り済み)

- v2.99.0(2026-09-01)で追加。`gh pr|issue create|edit|comment` の repeatable な
  フラグ。`--attach './after.png#Alt text'` の `#` 以降が alt テキストになる。
- 本文中に同じローカルパスの画像記法があればその位置がアップロード後の URL に
  書き換わり、無ければ本文末尾に追記される。
- 画像・動画のみ(画像は 10MB 上限)。対象リポジトリへの write 権限が必要。
  GitHub.com / GitHub Enterprise Cloud のみ(GHES 非対応)。プライベートリポジトリ
  の添付 URL は閲覧に認証が必要(このリポジトリは public なので無関係)。
- このリポジトリのピン先(nixos-26.05)の `gh` は 2.96.0 で `--attach` を持たない
  ため、`flake.nix` の nixpkgs-unstable オーバーレイ(herdr と同じ単一パッケージ
  escape hatch、ADR-0001 Amendment)に `gh` を追加して先取りしている。stable が
  2.99.0 以上に追いついたらオーバーレイのこのエントリを外す。

## charm-freeze を選んだ理由

ターミナル出力(プロンプト、statusline、TUI のレンダリング結果)の見た目を画像化
する手段として、stable 収録済みの `charm-freeze`(バイナリ: `freeze`)と
`termshot` を比較検討した。`freeze` はパイプで受けた ANSI 出力を PNG/SVG に描画
できる(`cmd | freeze -o out.png`)ため、変更前に保存しておいた「Before」の出力を
後から画像化できる。`termshot` はコマンドを pty で再実行する方式で、既に失われた
変更前の状態を再現できない。「Before は変更を当てる前に撮る」というスキルの原則と
相性が良いのは前者。出力は必ず PNG にする(`--attach` は画像/動画のみ、SVG は
受理されない)。

## `PULL_REQUEST_TEMPLATE.md` を置かない理由

`docs/claude/pr-gate.md` に既にある判断(`gh pr create --body` はテンプレートを
一切読まない)を維持する。静的テンプレートを置いても実際の発生源(エージェント)
には当たらず、置いた瞬間から死に設定になる。標準スケルトンはスキル(判断知識)に
持たせ、リポジトリ固有のテンプレートがある場合はその見出し構造に従いつつ内容
要素を埋める運用にした。

## `diagramming` / `living-description` との関係

`diagramming` は作図全般の処方、`living-description` は Issue/PR 本文を正本として
保つ運用習慣。`pr-description` はどちらとも独立で、PR を出す**瞬間**の本文の型と
証跡要件に特化する。ただし両方と接続点がある: Before/After の画像を Mermaid 図
などで補強したい場合は `diagramming` を、PR マージ後に関連 Issue の本文を更新する
場合は `living-description` を併用する。
