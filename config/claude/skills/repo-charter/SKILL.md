---
name: repo-charter
description: 新規リポジトリ作成時(gh repo create)、または既存リポジトリの目的が曖昧になってきたときに、README に機械検査可能な charter(目的1文・Scope・Issue litmus・topics)を播く手順。charter・リポジトリの目的を言語化・Issue受け入れ判定・Issue litmus・スコープが曖昧・迷走している・gh repo create、といった文脈で使う。repo purpose statement, Issue acceptance criteria, README charter, gh repo create checklist、といった英語の文脈でも使う。github-audit-charters(事後の横断監査)とは役割が異なる — こちらは作成/適合化する側。
---

新規リポジトリは「何を作るか」だけが先に決まり、「どの Issue がこのリポジトリに
属するか」を判定する軸が言語化されないまま育つことが多い。軸が無いと、AI Agent
は目指すべき方向性がブレ、人間は何を期待すべきかブレて、見当外れの Issue を切る
(あるいは切ってよいか判断できない)。charter はこの軸を、README の中の 3 見出し
+ 1 メタデータという機械検査可能な形に固定する。

事後の横断検査は `github-audit-charters`(手順は `docs/github-audit-charters.md`)。
このスキルは charter を**播く/適合化する**側の手順。

## 1. charter インタビュー

次の順で、1 つずつ確定させる(飛ばさない)。

1. **目的 1 文** — このリポジトリが存在する理由を 1 文で。README の H1 直後の
   第 1 段落の第 1 文になり、そのまま GitHub の description にもなる
   (両者は同じ文字列 — description は README のミラーであって独立した二つ目の
   要約ではない)。
2. **Scope(In / Out)** — 「このリポジトリが担うこと」「担わないこと」を
   箇条書きで。Out は「関連するが別リポジトリの責務」を具体的に書く
   (例: 「現像ワークフローの自動化は別リポジトリの責務」)。
3. **Issue litmus(判定問 + 実例)** — 「その Issue はこのリポジトリの
   [目的 1 文の核心] を前進させるか?」型の判定問を 1、2 個。判定問だけでは
   曖昧なので、採用例・棄却例を各 1、2 個添える。実例はパターンマッチで判定
   できることが目的なので、実在または実在しそうな具体的な Issue を書く
   (抽象的な原則の言い換えにしない)。

インタビューは人間との対話で埋める。埋まらない項目があるなら、それは
「リポジトリの目的がまだ固まっていない」ということなので、charter を書くこと
自体を急がず、目的の言語化を先に済ませる。

## 2. README への反映(スキーマ)

見出しリテラル(英語)は固定 — `github-audit-charters` がこの文字列で機械検査
する。日本語で書くリポジトリでも見出しはこの英語表記を使う。

```markdown
# <repo-name>

<目的 1 文>。<自由記述の続き。省略可>

## Why
(自由記述。既存の内容があればそのまま活かす)

## Scope

In:
- ...

Out:
- ...

## Issue litmus

判定問: <このリポジトリの目的を前進させるかを問う疑問文>

採用例:
- ...

棄却例:
- ...(別リポジトリの責務ならその名前を書く)
```

既存の `CONTEXT.md` / `vision.md` 等の詳細ドキュメントは削らず、README から
リンクする形で残す。charter は「意思決定に十分な最小限」であって、詳細設計の
置き場ではない。

## 3. GitHub メタデータへの反映

```bash
gh repo create <owner>/<repo> --private --description "<目的 1 文>"   # 新規時
gh repo edit <owner>/<repo> --description "<目的 1 文>"                # 既存の適合化時
gh repo edit <owner>/<repo> --add-topic <topic1> --add-topic <topic2>
```

`--description` は charter インタビューの目的 1 文と**一字一句**一致させる
(監査は正規化した文字列一致で判定する — `docs/github-audit-charters.md`)。
topics は最低 1 つ、リポジトリの技術領域・ドメインを表す語を選ぶ。

## 4. AGENTS.md への参照行(任意だが推奨)

AGENTS.md がある(または新設する)リポジトリでは、冒頭に 1 行:

```markdown
このリポジトリの方向性・Issue 受け入れ判定は README の Charter
(`## Scope` / `## Issue litmus`)を正本とする。作業規約は本ファイルが担う。
```

AGENTS.md 自体は `github-audit-charters` の監査対象ではない(方向性の正本は
README、作業規約は別問題)。

## 5. 自己検証

```bash
github-audit-charters
```

対象リポジトリが `ok` と出れば完了。`drifted` の場合は `missing=` の項目
(`no-purpose-paragraph` / `purpose-mismatch` / `no-scope-section` /
`no-issue-litmus-section` / `no-topics`)を読んで埋め直す。

## 6. 既存 Issue への適用(適合化のとき)

既存リポジトリに charter を播いた直後は、居座っている open Issue の中に
Issue litmus の棄却例に該当するものがないか一度だけ棚卸しする。該当する
Issue は「Issue litmus の棄却例(<該当する棄却例>)に該当」という理由を
コメントして close する。理由を書かずに close しない — 後から見た人が
「なぜ切られたか」を charter に立ち戻って再確認できることが目的。
