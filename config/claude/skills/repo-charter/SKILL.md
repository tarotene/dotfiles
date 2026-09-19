---
name: repo-charter
description: 新規リポジトリ作成時(gh repo create)、または既存リポジトリの目的が曖昧になってきたときに、README + CONTRIBUTING.md に機械検査可能な charter(目的1文・Scope・Issue受け入れ判定・命名クラス・topics)を播き、AGENTS.md/CLAUDE.md ルーティングと skills 配置を整える手順。charter・リポジトリの目的を言語化・Issue受け入れ判定・スコープが曖昧・迷走している・gh repo create、といった文脈で使う。repo purpose statement, Issue acceptance criteria, README charter, gh repo create checklist、といった英語の文脈でも使う。github-audit(事後の横断監査)・github-audit-triage(監査駆動の一括裁定)とは役割が異なる — こちらは 1 リポジトリを播く/適合化する側。
---

新規リポジトリは「何を作るか」だけが先に決まり、「どの Issue がこのリポジトリに
属するか」を判定する軸が言語化されないまま育つことが多い。軸が無いと、AI Agent
は目指すべき方向性がブレ、人間は何を期待すべきかブレて、見当外れの Issue を切る
(あるいは切ってよいか判断できない)。charter はこの軸を、README/CONTRIBUTING の
中の機械検査可能な形に固定する。

事後の横断検査は `github-audit`(手順は `docs/github-audit.md`)、監査 findings
からの一括裁定は `github-audit-triage`(`docs/claude/github-audit-triage.md`)。
このスキルは 1 リポジトリを**播く/適合化する**側の手順。README 正形・
ルート文書 allowlist・AGENTS.md 正本化は ADR-0016、CONTRIBUTING.md 自体の
固定スキーマは ADR-0017(いずれも tarotene/dotfiles)の裁定に従う。

## 1. charter インタビュー

次の順で、1 つずつ確定させる(飛ばさない)。

1. **目的 1 文**(≤120 字) — このリポジトリが存在する理由を 1 文で。README の
   H1 直後の段落の書き出しになり、そのまま GitHub の description にもなる
   (両者は同じ文字列 — description は README のミラーであって独立した二つ目の
   要約ではない)。
2. **命名クラス**(ADR-0014 + ADR-0020) — このリポジトリは `naming-codename`
   (恒久ツール・一語)/ `naming-descriptive`(研究・記録・コンテンツ系の
   複合語)/ `naming-pj`(期限付きプロジェクト、`pj-` prefix)/
   `naming-site`(公開ドメインのサイト、FQDN)のどれか。**新規作成は常に
   ADR-0020 の cutoff より後なので、字句パターンだけでなく閉じた語彙も
   ここで強制する**(github-audit の naming ドメインが後から drift 検出
   するのを待たない — 生成側で先に閉じる):
   - `naming-descriptive` を選ぶ場合: 名前の末尾トークンが
     `config/github-audit/descriptive-species.tsv` の種別語に**ないと
     作成に進めない**。ここにない種別語(特に「完了しうる行為」を表す
     語 — cleanup, migration 等)は `naming-pj` へ倒す。種別語を追加したい
     場合は先にそのファイルの改訂 PR を立てる。
   - `naming-codename` を選ぶ場合: 名前が
     `config/github-audit/codename-registry.tsv`(PUBLIC)または
     `~/.config/github-audit/codename-registry.local.tsv`(PRIVATE)に
     **登録されていないと作成に進めない**。未登録なら先にレジストリへの
     追記(PUBLIC は PR、PRIVATE はローカルファイルへの直接追記)を行う。
   - `naming-site` を選ぶ場合: ドメインが対応する
     `site-domains.tsv`/`.local.tsv` に登録されている前提で進める。
   - `naming-pj` は対象スロットが字句規則のみ(閉じた語彙なし)。
   名前が選んだクラスのパターン・語彙に一致しない場合、リポジトリ名を
   変えるかクラスを選び直すかをここで決める(改名は既存リンクを壊すので、
   新規作成時に決めるのが一番安い)。
3. **Scope**(地の文、In/Out ラベルなし) — このリポジトリが担うこと・担わない
   ことを、境界と代表的な caveats だけ数文で書く。担わない側は「関連するが
   別リポジトリの責務」を具体的に書く(例: 「現像ワークフローの自動化は
   downstream リポジトリの責務」)。箇条書きの羅列にせず、読んで文脈が繋がる
   ようにする(standard-readme の簡潔性規範、ADR-0016)。
4. **Issue 受け入れ判定(判定問 + 実例)** — 「その Issue はこのリポジトリの
   [目的 1 文の核心] を前進させるか?」型の判定問を 1 個。判定問だけでは
   曖昧なので、採用例・棄却例を各 1〜2 行で添える(長文の弁明は書かない —
   1 項目 1〜2 行に収める)。実例はパターンマッチで判定できることが目的なので、
   実在または実在しそうな具体的な Issue を書く(抽象的な原則の言い換えにしない)。
   この判定問 + 実例は CONTRIBUTING.md の `## Issues` 節の冒頭に書く(次々節)
   — 見出し名としての「Issue litmus」という自作語彙は ADR-0017 で廃止した。

インタビューは人間との対話で埋める。埋まらない項目があるなら、それは
「リポジトリの目的がまだ固まっていない」ということなので、charter を書くこと
自体を急がず、目的の言語化を先に済ませる。

## 2. README への反映(ADR-0016 の固定スキーマ)

見出しリテラル(英語)は固定・順序も固定 — `github-audit` の charters ドメインが
この形で機械検査する。日本語で書くリポジトリでも見出しはこの英語表記を使う。
`## Background` は任意(知的出自のみ)、それ以外は必須。

```markdown
# <repo-name>

<目的 1 文>。<自由記述の続き。省略可>

## Background

(任意。なぜこの形で存在するに至ったかの知的系譜のみ。改名の経緯・日付・
「〜待ち」のような時限状態は書かない — 履歴は git log/CHANGELOG の責務。
個別 Issue 番号も焼き込まない。)

## Install

<環境構築手順>

## Usage

<最小の実行可能な例>

## Scope

<地の文。境界と代表的な caveats>

## Development

<build/test/lint の定型コマンド>

## License

<ライセンス表記>
```

**書いてはいけないもの(出典は `docs/adr/0016-repository-document-canon.md`)**:
履歴・時限記述(「改名した」「follow-up 待ち」等、Google style guide の
timeless documentation 規範)、個別 Issue 番号の引用(Art of README の
永続性原則)、`## Issue litmus` 見出し(CONTRIBUTING.md へ移設、次節)、
許可外の見出し。既存の `CONTEXT.md` のような詳細ドキュメントは中身を精査し、
恒久的な内容だけ README/`docs/` へ移し、それ以外は破棄する(ルート allowlist、
次々節)。

## 3. CONTRIBUTING.md への反映(ADR-0013 部分 supersede、固定スキーマは ADR-0017)

CONTRIBUTING.md は GitHub が Issue/PR 作成画面で自動リンク表示する標準ファイル
(GitHub Docs "Setting guidelines for repository contributors")。この標準
機構が最も生きる 2 節 — issue の切り方・PR の出し方 — を固定見出しにする
(出典・調査範囲・却下した代替案は `docs/adr/0017-contributing-schema.md`)。
環境構築/テストの定型コマンドは README の `## Development` が既に正本なので、
ここでは重複させず参照 1 行に留める。

```markdown
# Contributing

See [README](README.md) for what this repository is and how to set up a
development environment.

## Issues

Judging question: <このリポジトリの目的を前進させるかを問う疑問文>

Accepted:
- ...

Rejected:
- ...(別リポジトリの責務ならその名前を書く)

## Pull requests

<PR の出し方・レビュー方針・マージ方式の定型文>
```

`## Expectations`(単独メンテナの応答期待値)は任意節として `## Pull
requests` の後に追加してよい。見出しリテラル(英語)・順序ともに
`github-audit` の charters ドメインが機械検査するため固定 — 日本語で書く
リポジトリでもこの英語表記を使う。「Issue litmus」という見出し名の自作語彙は
ADR-0017 で廃止した(判定問 + 採用例・棄却例という**内容の形**は `## Issues`
冒頭に維持 — AI エージェントがコントリビュータになるケースへの言及は
調査した一次情報のいずれにも無く、ここは先行例のない領域)。

## 4. ルート文書 allowlist(ADR-0016)

リポジトリのルートに置いてよい Markdown は `README.md` / `CONTRIBUTING.md` /
`CHANGELOG.md` / `AGENTS.md` / `CLAUDE.md` の 5 種(+ `LICENSE*`)のみ。
`CONTEXT.md` / `NOTES.md` のような野良ルート文書は作らない — 深い文書は
`docs/` 配下に置く。既存リポジトリの適合化でこの種のファイルが見つかったら、
恒久的な内容は README の `## Background` か `docs/` へ吸収し、ファイル自体は
削除する。

## 5. AGENTS.md / CLAUDE.md ルーティング(ADR-0016)

AI 向け正本は `AGENTS.md` 1 本。README/CONTRIBUTING を参照する側に置き、
内容を複製しない。

```markdown
# Agent instructions for <repo-name>

See [README.md](README.md) for what this repository is and does, and
[CONTRIBUTING.md](CONTRIBUTING.md) for what Issues and pull requests are
accepted. This file is for agent operating instructions only.
```

Claude Code を使うリポジトリでは `CLAUDE.md` を `@AGENTS.md` import 1 行 +
Claude 固有差分のみのルータにする:

```markdown
@AGENTS.md

<!-- Claude-specific differences from AGENTS.md, if any, go below. -->
```

CLAUDE.md に実内容を書き足していく(AGENTS.md との二重管理になる)構成は
`github-audit` の charters ドメインが drift として検出する。

## 6. skills を持つ場合のルーティング(ADR-0016)

このリポジトリ自身が Claude Code の repo スコープ skill を持つ(`.claude/
skills/<name>/SKILL.md` を新設する)場合、正本はツール中立の
`.agents/skills/<name>/SKILL.md` に置き、`.claude/skills/<name>` はそこへの
相対 symlink にする(Codex CLI・Copilot CLI は `.agents/skills/` を
ネイティブ読取、Claude Code は `.claude/skills/` のみ読取のため):

```bash
mkdir -p .agents/skills/<name>
# SKILL.md 等を .agents/skills/<name>/ に置いてから:
mkdir -p .claude/skills
ln -s ../../.agents/skills/<name> .claude/skills/<name>
```

## 7. GitHub メタデータへの反映

**新規作成の場合、`gh repo create` の前に手順 2 の閉じた語彙チェックが
通っていることを確認する**(ADR-0020)— `naming-codename` を選んだのに
レジストリ未登録のまま `gh repo create` すると、直後の `github-audit
naming` が `codename-not-registered` で即 drift 報告する。

```bash
gh repo create <owner>/<repo> --private --description "<目的 1 文>"   # 新規時
gh repo edit <owner>/<repo> --description "<目的 1 文>"                # 既存の適合化時
gh repo edit <owner>/<repo> --add-topic <naming-クラス> --add-topic <topic2>
```

`--description` は charter インタビューの目的 1 文と**一字一句**一致させる
(監査は正規化した文字列一致で判定する — `docs/github-audit.md`)。命名クラス
の topic(`naming-codename` 等)は必ず 1 つ、加えてリポジトリの技術領域・
ドメインを表す topic を最低 1 つ。

**新規作成の場合、続けて標準 ruleset を播く**(#153)。`gh` 自体には
`repo create` 直後に走るフック機構が無いため、この手順が事実上の自動適用に
なる。リポジトリの型が rust/typst/astro のいずれかで該当 governance skill を
持つ場合:

```bash
github-rulesets-apply <rust|typst|astro> <owner>/<repo>
```

review 層(Copilot code review + 会話 resolve 必須)は ADR-0021 のとおり
初期は付けない。開発初期フェーズを過ぎたら `--with-review` を付けて再実行する。

## 8. 自己検証

```bash
github-audit charters naming
```

対象リポジトリが両ドメインとも `ok` と出れば完了。`drifted` の場合は
`missing=` の項目を読んで埋め直す(`docs/github-audit.md` に各項目の説明)。

## 9. 既存 Issue への適用(適合化のとき)

既存リポジトリに charter を播いた直後は、居座っている open Issue の中に
CONTRIBUTING.md `## Issues` の棄却例に該当するものがないか一度だけ棚卸しする。
該当する Issue は「`## Issues` の棄却例(<該当する棄却例>)に該当」という
理由をコメントして close する。理由を書かずに close しない — 後から見た人が
「なぜ切られたか」を charter に立ち戻って再確認できることが目的。

## 10. 一括適合化が必要なとき

多数のリポジトリが同時に drift しており 1 リポジトリずつのインタビューでは
収束しない場合は、監査駆動で一括起草・一括レビューする
`github-audit-triage` スキルを使う(このスキルのスキーマ定義を正本として
参照する)。
