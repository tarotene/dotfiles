# エージェント向けツールの移行

この文書は命名・責務・配置を揃える移行の一覧である。新しい呼び出しには新名称を使う。
旧入口は、適用前の設定を持つホストと継続中のセッションのために残す。削除は別 PR で行う。

## 改名一覧

| 旧名 | 新名 |
|---|---|
| copilot-model-bump | model-pin-update |
| desk-start | daily-desk-reminder |
| esa-index | esa-mirror-context |
| pr-confirm-guard | pr-followup-guard |
| performance-planning | music-performance-planning |
| talk-script | presentation-authoring |
| scope-inventory | requirements-inventory |
| selection-grounding | technology-selection |
| sign-prewarm | gpg-prewarm |
| settings-reconcile | agent-settings-reconcile |
| worktree-fresh-base | worktree-base-sync |
| git-checkout-freshness | git-checkout-sync |
| wrapup-chores | backlog-cleanup |
| skill-gardening | agent-tooling-authoring |
| external-call-scheduling | human-task-scheduling |

`wrapup-stop-gate`・`wrapup-session-start` は `session-wrapup stop|start` と責務別の三つの CLI へ移る。
`hook-io`・`guard-core` の移動先は[ライブラリ一覧](agent-libraries.md)を参照する。
共有の source は `config/claude/skills/` → `config/agents/skills/`、設計文書は
`docs/claude/` → `docs/agents/`、配布宣言は `home/modules/agents.nix` に移した。
製品固有の設定・説明と、現在の配布先・対応範囲は維持する。

## 維持する契約

永続パス、環境変数、JSON 形式、記録 ID、ロック、承認記録、投稿マーカーを維持する。
たとえば `esa-index/config.json`、`WORKTREE_FRESH_BASE_FETCH_TTL`、wrapup の state root、
`<!-- codex-trial:ID -->`、`<!-- wrapup-retro -->` はそのまま使う。
既存の directive 識別子 `esa-index` と gate-event 識別子 `pr-confirm-guard` も、実行名と分けて維持する。
`agent-permissions` の旧 CLI は互換入口として残す。共有権限管理の撤去は
ADR-457 の 2026-10-27 Amendment と `agent-permissions retire` に従う。

旧 CLI は新 CLI と同じ entry 実装を呼び、引数・stdin・終了コード・stdout/stderr を転送する。
旧 skill は旧ディレクトリと同じ name を持ち、新 skill の全文を読む入口だけを持つ。
新旧で手順本文や状態を複製しない。

hook 登録は既存の managed-hooks 所有記録で reconcile し、前回の登録のうち新宣言から消えたものだけを外す。
利用者独自の登録は残す。凍結済みの退役一覧と旧照合文字列は維持する。
所有記録がまだ無いホストへの適用は、既存の初回移行と同じ境界で扱う。

## 旧入口の削除条件

次の四条件をすべて確認してから別 PR で削除する。経過日数だけでは削除しない。

1. vega・arcturus・altair の全ホストへ新設定が適用されている。
2. 旧設定を保持するセッションが終了している。
3. 現行設定・skill・内部呼び出しの旧入口参照がゼロである。
4. 互換入口なしで検証が通る。

ホスト確認では `hms` のあとに各製品の hook 登録と `~/.local/state/dotfiles/managed-hooks-*.json` を読み戻す。
コマンドに新名称があり、旧名称の登録が無く、独自登録が残っていることを確認する。
この PR の build は実機適用の代わりにしない。

参照検索と再検証の入口:

```sh
rg -n 'copilot-model-bump|desk-start|esa-index|pr-confirm-guard|performance-planning|talk-script|scope-inventory|selection-grounding|sign-prewarm|settings-reconcile|worktree-fresh-base|git-checkout-freshness|wrapup-chores|skill-gardening|external-call-scheduling|wrapup-stop-gate|wrapup-session-start' home config crates docs
cargo test --workspace --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
nix flake check --all-systems --no-build
```

検索結果のうち、この移行一覧、互換入口、旧名の照合値、保存先の契約、履歴・引用は削除対象の呼び出しに数えない。
互換入口を除いた候補ツリーで、上の検証と各ホストの activation build を行う。
確認結果はホストの記録・既存 Issue・PR から読み、手書きの恒久台帳を増やさない。

## 改名しなかった対象

Rust 対象はソース先頭の責務説明と利用側、skill は frontmatter と配布宣言を点検した。
下表の維持理由は名前と責務の対応を示す。ホスト運用 CLI は隣接対象として残す。

| 対象 | 維持理由 |
|---|---|
| `adr-number` | ADR の採番を表す。 |
| `agent-turn-log` | エージェントのターン記録を表す。 |
| `apply-rulesets` | 宣言した ruleset の反映を表す。 |
| `attribution-guard` | 生成元表示の欠落から投稿を守る。 |
| `claude-plan-model` | Claude の計画用モデル対を扱う。 |
| `claude-statusline` | Claude の statusline を描く。 |
| `claude-usage` | Claude の使用量を表示する。 |
| `cmd-hash-log` | コマンドのハッシュ記録を表す。 |
| `codex-plan` | Codex の計画モード用モデル対を扱う。 |
| `codex-plan-gate` | Codex の計画完了条件を止める。 |
| `copilot-plan-review` | Copilot による計画レビューを表す。 |
| `decision-colocation` | 決定成果物と執行点の同梱を判定する共通実装。 |
| `detach-open` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `detect-drift` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `dotfiles-doctor` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `dotfiles-identity` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `due-remind` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `esa-mcp-launcher` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `external-send-guard` | 外部への直接送信から下書き手順を守る。 |
| `feedback-target-guard` | 実在対象の無い feedback 投稿を防ぐ。 |
| `gate-failopen-advisory` | gate の fail-open を通知する。 |
| `gh-edit-allow` | 検証した GitHub 編集を許可する。 |
| `git-audit-worktrees` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `git-prune` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `git-shelve` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `git-stash-guard` | 復元条件を欠く stash 操作を防ぐ。 |
| `git-worktree-allow` | 検証した worktree 操作を許可する。 |
| `git-worktree-create-guard` | 直接の worktree 作成を防ぐ。 |
| `github-app-registry-check` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `github-app-snapshot` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `github-audit` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `gpg-subkey` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `herdr-agent-metadata` | herdr へ渡すエージェント情報を扱う。 |
| `herdr-issue-counts` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `herdr-status-advisory` | herdr の版の食い違いを通知する。 |
| `hms` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `hook-router` | hook 判定の配線を担う。 |
| `hub-resolve` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `issue-index` | Issue の索引を注入する。 |
| `kiriban` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `main-checkout-guard` | 本物の checkout を変更から守る。 |
| `new-tool-guard` | 既存手段の検討なしの道具追加を防ぐ。 |
| `pipeline-map` | 自動発火する機構の地図を生成・検査する。 |
| `pkexec-guard` | 表示可能範囲を超える管理者コマンドを防ぐ。 |
| `plan-fresh-gate` | 計画の鮮度条件が揃うまで進行を止める。 |
| `plan-precedent-gate` | 計画の先行例確認が揃うまで進行を止める。 |
| `plan-scope-gate` | 計画の要求網羅が揃うまで進行を止める。 |
| `plan-view` | 計画を表示する。 |
| `pr-contract-check` | PR 契約の適合性を判定する。公開パスと依存ゼロを保持。 |
| `pr-gate` | PR 完了条件が揃うまで終了を止める。 |
| `pr-title-check` | PR タイトルの適合性を判定する。公開パスと依存ゼロを保持。 |
| `pr-title-guard` | 契約に反する PR タイトルを防ぐ。 |
| `promotion-detect` | 手順を決定論化する候補を検出する。 |
| `repo-create-guard` | 作成手順を通らない repo 作成を防ぐ。 |
| `routines-write-guard` | 宣言外 routine の書き込みを防ぐ。 |
| `rulesets-write-guard` | 宣言外 ruleset の書き込みを防ぐ。 |
| `stack-base-guard` | PR チェーンに反する base 指定を防ぐ。 |
| `tailscale-prefs` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `update-own-tools` | ホスト運用の隣接 CLI。担当する操作・対象を名前で表している。 |
| `verdict-escalate` | 反復した判定を集約して Issue 候補へ上げる。 |
| `worklog` | 作業記録を扱う。 |

共有 skill の維持対象:

| 対象 | 維持理由 |
|---|---|
| `astro-site-governance` | Astro サイトのリポジトリ規約を整える。 |
| `browser-login-handoff` | ブラウザのログインを本人へ引き継ぐ。フォーム操作は共有参照へ分離。 |
| `claude-routines` | Claude の routine 宣言と運用を扱う。 |
| `daily-desk` | 朝の作業を順に進める入口。通知役の daily-desk-reminder と区別。 |
| `diagramming` | 図を作る判断と手順を扱う。 |
| `drive-upload` | Drive へのアップロード手順を表す。 |
| `gas-clasp-ops` | clasp を使った GAS 運用を表す。 |
| `github-audit-triage` | GitHub 監査の結果を仕分ける。 |
| `gpg-subkey-rotation` | GPG サブ鍵のローテーション手順を表す。 |
| `handoff` | 作業の引き継ぎを整える。予定化は human-task-scheduling に委ねる。 |
| `inbox-adjudication` | 受信項目への対応を裁定する。CI 障害手順は共有参照へ分離。 |
| `issue-hygiene` | Issue の状態と記述を整える。 |
| `issue-ref-freshness` | Issue 参照の鮮度を確認する。 |
| `living-description` | 進行に合わせて説明を更新する。 |
| `pr-description` | PR 本文の記述手順を表す。 |
| `precedent-grounding` | 設計判断を先行例と対比する。技術選定以外も対象。 |
| `repo-charter` | リポジトリの目的と責務を決める。 |
| `retro` | セッションの振り返り手順。session-retrospective は実装側。 |
| `rust-repo-governance` | Rust リポジトリの規約を整える。 |
| `slot-availability` | 予定の空き枠を判定する。 |
| `stacked-pr` | PR のチェーン運用を表す。 |
| `test-grounding` | テストの根拠を確かめる。 |
| `tracking-issue` | 追跡用 Issue の構成と運用を表す。 |
| `typst-repo-governance` | Typst リポジトリの規約を整える。 |
| `venue-search` | 会場探しの手順を表す。 |
| `writing-style` | 文体規約の正本を参照する。 |

同じ crate が配る追加の CLI も維持する。

| CLI | 維持理由 |
|---|---|
| `agent-permissions` | 旧共有権限の撤去と既存 CLI の互換入口。通常実行の権限判定には使わない。 |
| `github-rulesets-apply` | 複数リポジトリへ ruleset の適用を回す。単一対象の apply-rulesets と区別する。 |
| `decision-colocation-check` | CI で決定成果物と執行点の同梱を判定する。 |
| `decision-colocation-guard` | 投稿時に同梱条件を満たさない PR を防ぐ。 |
| `git-prune-branches` | 追跡先が消えたブランチを整理する。 |
| `git-prune-worktrees` | 残骸 worktree を整理する。 |
| `git-unshelve` | 当該 worktree が退避した作業を戻す。git-shelve と対になる。 |
| `performance-hub` | 演奏予定のハブを解決する補助入口。planning の手順とは異なる操作を指す。 |
| `writing-style-hub` | 文体規約のハブを解決する。 |
| `pr-merge-settings-check` | PR のマージ設定の適合性を判定する。 |
| `rulesets-context-check` | ruleset の必須チェック名と実装の対応を判定する。 |
| `workflow-naming-check` | workflow 名の適合性を判定する。 |

第三者製 frontend-slides・plugin は upstream の名前と pin に従う。
`repo-governance-common` は参照資料とテンプレートを配る共有ディレクトリで、独立した skill ではない。

## 参照する既存課題

- Reference-Only: #991 — ルータ配置見直し。この改名で配置判断を完了扱いにしない。
- Reference-Only: #990 — 3 ホストのルータ実機確認。build を実機確認の代わりにしない。
- Reference-Only: #979 — 全ホストの所有記録確認後の退役一覧撤去。凍結一覧と撤去条件を維持する。
- Reference-Only: #1028 — trial 待機通知の重複。責務分割だけで解決扱いにしない。

各 Issue の原文を GitHub CLI で確認した（取得 2026-10-26）。

## 公開情報検査に伴う説明文の言い換え

移動した既存ファイルの一般語が公開情報検査の語彙と衝突したため、既存の運用規約
（public-publish-guard.md の「一般語の一致で push が止まったとき」）に従って説明文を言い換えた。
診断ログと inbox 手順書の stdout にも表現の変更がある。判定、引数、終了コード、状態形式は変えない。
ガードの設定・照合条件・許可リストは変更していない。
