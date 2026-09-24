# issue-ref-freshness スキル

`config/claude/skills/issue-ref-freshness/SKILL.md` は、他リポジトリの issue/PR を
「Open である」前提で参照している記述を、参照先の裁定に追随させるスキル。

## 動機

upstream に起票した feature request が not planned で閉じられていた。それに数日
気づかず、このリポジトリには「upstream が対応したらこのパッチを落とす」という
記述が 3 箇所残っていた(flake.nix の overlay コメント、パッチのヘッダ、
operations.md)。記述は upstream の状態と無関係に固定されるため、放っておくと
「いずれ消える一時策」が恒久策として黙って居座る。

問題は 2 つに分かれる。

1. **裁定に気づけない。** GitHub の通知 inbox では、自分が起票した issue への
   動き(`reason: author`)が CI 通知や mention に埋もれる
2. **気づいても言及元が直らない。** 参照先の状態と、それを前提にした記述とを
   結ぶ仕組みがない

このスキルは 2 だけを受け持つ。1 は通知の可視化の問題で、別の私設ツールの
責務とした(このリポジトリには通知ポーリングを置かない)。

## 常時監視を置かなかった理由

最初の設計は、待ちタスクをラベル付きのローカル Issue で表し、毎日の systemd
timer で参照先の状態を照会し、閉じていたらコメントとラベル付け替えで発火させる
checker だった。実装言語は ADR-0024 に従い Rust の想定だった。

採らなかった理由は還元性(共有 AGENTS.md「技術・仕組みの選択」の第 2 軸)。
裁定への気づきを通知側で確保する前提では、常時監視が追加で担うのは
「誰も触らない参照の自動発火」だけになる。それに対して、登録規約・ラベル 2 種・
crate・timer という可動部品を持つことになる。スキルは登録ゼロで、既存の
あらゆる参照にそのまま効く。

触れられない参照の取りこぼしは、手動の一括点検(SKILL.md §3)で補う。

## 先行例との関係

- **todocheck**(<https://github.com/preslavmihaylov/todocheck>, 2026-09-20 取得)
  は `TODO(#N)` の参照先が閉じていたら lint を落とす
- **ory/closed-reference-notifier**(<https://github.com/ory/closed-reference-notifier>,
  2026-09-20 取得)は参照先が閉じたらローカル issue を起票する

どちらも「閉じた」ことだけで機械的に終端する。このスキルは終端を機械判定に
しない。completed と not planned では次の手が逆になるため、裁定を読んでから
言及元を書き直す(SKILL.md §4)。

- **Homebrew Formula Cookbook**(<https://docs.brew.sh/Formula-Cookbook>,
  2026-09-20 取得)の「パッチには upstream issue へのリンクをコメントで添える」
  規約は、このリポジトリの既存の書き方と同じ。SKILL.md §6 はこれに日付と条件の
  併記を加え、一括点検で拾える形にしている

## living-description との関係

living-description は「Issue 本文を、コメントで確定した裁定に追随させる」習慣。
このスキルは同じ構造を、裁定の出どころが他リポジトリにある場合へ広げたもの。
Issue 本文を書き直すときの作法は living-description に委ねる。

## 初回適用

herdrdev/herdr#4374(not planned, 2026-09-19)を参照していた 3 箇所を書き直し、
パッチを恒久化するかどうかの判断を #449 に切り出した。同時期に閉じられた
herdrdev/herdr#4317 は、このリポジトリに参照が残っておらず、代わりの監査・
回収スクリプトが既に恒久策として文書化されていた(`docs/worktree-lifecycle.md`)
ため、書き直しは不要だった。
