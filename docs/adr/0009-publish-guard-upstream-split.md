# ADR-0009 — 公開面ガードの上流分離と配布

- Status: Accepted
- Date: 2026-09-10
- Issue: No-Issue(事後対応。実装は [tarotene/publish-guard](https://github.com/tarotene/publish-guard) #1-#4)

## Context

PR #155(2026-09-09 merge、追随修正 #157)で `config/claude/hooks/
public-publish-guard.sh` を導入した。所属 org の private リポ名と個人
private リポ名が PUBLIC な `tarotene/dotfiles` の PR 本文・Issue 本文・
コミット済みファイルに混入した事故(2026-09-10)の再発防止で、`git push`
と `gh pr|issue create|edit|comment` を PreToolUse で検査する。git 履歴が
orphan rewrite で単一コミットに畳まれている(`c63d8d5`)ため、この経緯は
コミット時系列だけでは追えない。

この仕組みを他の環境(所属 org の同僚・一般公開)にポータブル化して配布
する方法を調査・研究・敵対的レビューした結果、次の2点が判明した。

### この仕組みには代替が存在しない

- GitHub **push protection** が block する面は「CLI からの push・UI コミット・
  ファイルアップロード・REST API・GitHub MCP(public リポのみ)」で、
  **Issue 本文・PR 本文・コメントは列挙されていない**。— GitHub, "About
  push protection",
  <https://docs.github.com/en/code-security/secret-scanning/introduction/about-push-protection>
  (2026-09-10 取得)
- 非コード面(Issue/PR/Discussions/Wiki)の secret scanning は 2024-08-16
  GA だが**検知のみ、防止なし**。— GitHub Changelog, "Secret scanning for
  non-code GitHub surfaces is now generally available", 2024-08-16,
  <https://github.blog/changelog/2024-08-16-secret-scanning-for-non-code-github-surfaces-is-now-generally-available/>
  (2026-09-10 取得)
- push ruleset は内容検査が原理的に不可能(パス・パス長・拡張子・サイズの
  4種の metadata ルールのみ)で、public リポには適用されない。— GitHub,
  "Available rules for rulesets",
  <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets>
  (2026-09-10 取得)
- `git-secrets`/`gitleaks`/`trufflehog`/`detect-secrets` はいずれも git
  内容専用のスキャナで、`gh pr create` の argv/heredoc を見る機構が無い。
- Anthropic/OpenAI/GitHub/Cursor いずれも tool call 傍受プリミティブのみを
  提供し、outbound 内容分類器や機密識別子レジストリは提供していない。

→ `gh pr|issue` 経路を防止する仕組みは自作以外に存在しない。配布する価値は
確立している。

### `tarotene/dotfiles` は upstream として構造的に不適格

ADR-0004 の決定4は「No semver releases」。一方 Claude Code の plugin
marketplace は `version` 省略時に **commit SHA** で消費者を pin する。—
Anthropic, "Plugin marketplaces", <https://code.claude.com/docs/en/plugin-marketplaces.md>
(2026-09-10 取得)

`tarotene/dotfiles` は *clean orphan history による main 履歴書き換え* を
漏洩対応の正規手段として既に3回実行している。ここを upstream にすると、
次の orphan rewrite で全消費者の pin が消滅する。ADR-0004 を改めて履歴
書き換えを封じる案は「漏洩対応の最後の手を、漏洩防止ツールの配布のために
手放す」倒錯であり採らない。

## Decision

1. 配布物は **別リポジトリ [tarotene/publish-guard](https://github.com/tarotene/publish-guard)**
   (public / MIT)を upstream とする。ADR-0004 決定4(No semver releases)
   と orphan-history rewrite の運用は `tarotene/dotfiles` 側に保持したまま、
   新リポだけが安定した commit 履歴と tag を持つ。**ADR-0004 は改めない。**
2. `tarotene/dotfiles` は新リポを **flake input として pin**(`flake.lock`
   で rev pin)し、`home.file` 配備を維持する。`/plugin install` は
   home-manager 管理外の手動状態を生み(#132「gh stack 拡張が home-manager
   管理外」と同型のドリフト)、ADR-0001 の source of truth 原則に反するため
   採らない。
3. **二層構造**: エージェント非依存の汎用 CLI(`publish-guard`)+ 薄い
   adapter 層(Claude Code plugin / Codex CLI / Copilot CLI)。既存の
   `plan-view` の先例(`docs/claude/plan-view.md` — 「汎用の `plan-view`
   CLI を下層に置き、自動発火は Claude Code にだけ配線した」)と同型。
4. **ゼロデータ配布**: denylist(org 名・private リポ名)は一切配布しない。
   手書きが必要なのは org 名 1 行(`orgs.txt`)だけで、リポジトリ名は各自の
   `gh` credential でライブ導出する。「ツールは公開・データはローカル」
   原則(`scripts/github-audit-rulesets` の `overrides.tsv` と同型)を
   引き続き適用する。
5. **fail-loud**: 判定不能(merge-base 解決不能・stdin 読み取り不能・jq
   不在)を無言の pass にせず `ask` にエスカレートする。旧実装は無言
   exit 0 で、「導入済みだから守られている」と信じたまま無防備になる経路
   だった。
6. **matcher は複合1本 `Bash|mcp__.*`**。旧実装は `Bash` matcher のみで、
   GitHub MCP server 経由の `mcp__github__create_issue` 等が完全に無検査
   だった(本仕組み最大の機能欠陥)。matcher を分けると、登録系
   (`registerHooks` の存在判定が command 文字列の完全一致でしか行えない
   制約)により片方が無検査で残る経路が生まれるため、1本に統一する。
7. `tarotene/dotfiles` は `home/modules/claude.nix` の配線のみを更新する
   (Claude Code plugin 相当の hook 登録)。Codex CLI / Copilot CLI 向けの
   adapter は新リポジトリが提供するが、それを `tarotene/dotfiles` の
   `~/.codex/hooks.json`・`~/.copilot/settings.json` に配線するかどうかは
   本 ADR の対象外とし、別の判断に委ねる。

## Alternatives considered

- **denylist の hash/Bloom filter 化**(policy ファイルが漏洩しても無害に
  する案。HIBP の k-anonymity レンジ API が最も近い前例だが、この用途での
  先行実装は無し)— 現在の照合方式(`grep -F` による部分文字列一致)と
  両立しない。org 名は元々推測可能性が高く保護価値も薄い。棄却。
- **社内 git への policy pack 配布** — private リポ名の網羅リストが N 台に
  増殖し、「守るための仕組みが、守るべきものの棚卸し表を量産する」反転が
  起きる。棄却。
- **pre-commit フレームワーク hook 単体** — install 機構を再発明せずに
  済むが、PR/Issue 本文経路を原理的にカバーできず、唯一の正当化根拠を
  捨てることになる。棄却。
- **gitleaks への全面寄せ**(private ruleset を `[extend] path` /
  `GITLEAKS_CONFIG_TOML` で与える案)— 自作コード量は最小になるが、consumer
  に gitleaks バイナリ依存と TOML 学習コストを押し付ける。今回は採らないが
  将来の選択肢として記録する。

## 明示すべき境界

これは **security boundary ではなく、事故とエージェントの滑りに対する
ガードレール**である。詳細と出典(Lampson 1973 / Saltzer & Schroeder 1975 /
CWE-184 / Rahman et al. 2022)は
[tarotene/publish-guard の README](https://github.com/tarotene/publish-guard#readme)
(「これは security boundary ではない」節)に記録した(ADR-0008 決定1:
実装物の利用者向け説明は実装物のリポジトリを正本にし、ここには埋め込まない)。

## Consequences

- `config/claude/hooks/public-publish-guard.sh` を削除し、
  `tarotene/publish-guard` の CLI + Claude adapter を flake input 経由で
  配備する(実装は本 ADR と同じ変更セットの後続コミットで行う)。
- `docs/claude/public-publish-guard.md` は上流リポへのポインタ + dotfiles
  固有の配線に改稿する(ADR-0008 の docs-correspondence 原則)。
- 新リポの stopword/allowlist 拡張や Codex/Copilot adapter の詳細は
  `tarotene/publish-guard` 側の README/CONTRIBUTING が正本であり、この
  ADR には埋め込まない(ADR-0008 決定1: 時間で腐る事実は別文書からリンク)。
- 未検証の前提(実装時点):
  - Claude Code hook の `deny` が `bypassPermissions` 下でも効くかは公式
    ドキュメントで断定できなかった。README で「境界ではない」と書く根拠に
    関わるため、確定した一次情報が見つかるまで断定しない。
  - GitHub の secret scanning custom pattern が非コード面(Issue/PR 本文)に
    対して評価されるかどうかも、どちらとも書いた公式ドキュメントが無い。
- `tarotene/publish-guard` 側の PreToolUse 入出力契約(Claude Code /
  Codex CLI / Copilot CLI)は実機(`codex exec
  --dangerously-bypass-hook-trust`・`copilot -p ... --allow-all-tools`)で
  実測して確認した(2026-09-10)。Codex CLI の MCP tool 命名規則は未確認
  のまま残っている。

## Verification

- `nix flake check` — 3 ホストとも green(flake input 追加後)
- `home-manager switch` 後、`~/.claude/settings.json` の PreToolUse に
  旧 `public-publish-guard.sh` の command が残っていないこと、新 command が
  matcher `Bash|mcp__.*` で1個だけ存在すること
- 実セッションで `gh pr create`/MCP 経由の deny/ask/pass を確認
