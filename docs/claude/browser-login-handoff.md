# browser-login-handoff — ブラウザ未ログイン時のハンドオフ手順を型化した理由

## なぜこれが必要か

Playwright MCP・claude-in-chrome でブラウザ操作する複数のスキル(person-state
系の private スキル含む)で、対象アカウントが未ログインだったときの
ハンドオフ手順をその場で都度組み立てていた。ユーザーから「この手順、
コモディティ化(型化)したい」というフィードバックを受けた(#562)。

パスワード・二段階認証コードの入力を Claude が代行してはならないという
制約は既に自明だが、「未ログインをどう判定するか」「引き継ぎ後にどう
再開するか」は毎回微妙にブレていた。判断知識としてスキルに切り出す。

## 器をスキルにした理由

未ログイン検出の判断基準は文脈依存(ページごとにログイン状態の見え方が
異なる)であり、機械的なフック(hook)では代替できない。CLAUDE.md への
格上げも検討したが、発動条件を「ブラウザ操作中に未ログインを検知した」
という具体的な状況で言い切れるため、全セッション常時発火のグローバル
原則にする必要はないと判断した。

## `venue-search`ではなく新規スキルにした理由

`venue-search`は「ログイン不要な会場探し」に設計上限定しているスキルで、
未ログインハンドオフの発生源(private側のperson-state系スキル、Google系
サービス操作)とも設計上の一致点が無い。`external-call-scheduling`
(Claude が代行できない作業をユーザーに引き継ぐ判断知識)とは同じ型だが、
対象がブラウザのログイン操作という具体的な技術面を持つため、別スキルに
分離した。

## `.agents/skills/`へミラーしない理由

Playwright MCP・claude-in-chrome はいずれも Claude Code 固有のツール
(MCP server)であり、Codex CLI・Copilot CLI では利用できない。
`external-call-scheduling`・`venue-search`と同じ理由で、cross-tool
ルーティング対象からは外している。

## 先行例調査(2026-09-29)

- Playwright 公式 "Authentication" (<https://playwright.dev/docs/auth>):
  ログイン状態の保存(`storageState`)はテストランナー向けの自動ログイン
  setup project を前提としており、対話セッションで人間にハンドオフする
  手順は書かれていない。
- Playwright MCP README (microsoft/playwright-mcp): 既定で persistent
  profile を使い、ログイン情報はディスクに保存される
  ("All the logged in information will be stored")。これにより再発頻度
  が既に低い(SKILL.md §5 参照)。
- リポジトリ内の全 skill を「未ログイン」「storageState」「ハンドオフ」で
  検索したが、対話セッション向けの型は既存の skill のいずれにも無かった。
