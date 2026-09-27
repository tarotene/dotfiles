# claude-routines — Claude Code routine を as-code で扱う仕組みの設計記録

Claude Code routine(claude.ai 上の scheduled cloud agent)は Web UI・
`/schedule`・`RemoteTrigger` で個別に作るしかなく、定義(prompt・cron・
対象 repo)がどこにも版管理されていなかった。`/grill-me` と、その後の
Plan agent による敵対的レビュー(replan)を経て、対象リポジトリ自身の
宣言を正本にする形に決めた(ADR-503〔rulesets の宣言〕と同型)。

実装は `config/claude/skills/claude-routines/`(手順は `SKILL.md`〔ローカル
session での create/adopt/apply〕と `auditor.md`〔クラウドの自己監査
routine の汎用手順〕、決定論的な差分コアは `scripts/routines-plan.sh` +
`scripts/lib.jq`)。機構側は `crates/routines-write-guard`(段2)と
`scripts/github-audit` の routines ドメイン(段3)。詳しい設計判断の一覧は
`docs/adr/519-routines-declaration-in-repo.md` を参照。

## 決定表

| 論点 | 決定 |
|---|---|
| 宣言の場所 | 対象リポジトリ自身の `.claude/routines/<name>.{json,md}`(dotfiles には置かない) |
| prompt 本体 | `<name>.md` が正本。live prompt は `Read and follow .claude/routines/<name>.md in <home_repo>.` という薄いポインタ |
| 設定 | `id`/`state`/`cron_utc`/`model`/`environment_id`/`home_repo`/`sources`/`allowed_tools`/`role` の JSON |
| 対象範囲 | cron routine の主要項目のみ。run-once/webhook trigger は宣言できない(表現不可能) |
| 同定 | live に書き戻した `trig_` ID を pin。`name` は `<home_repo>:<name>` の名前空間キーで検証 |
| 差分検出 | prompt 末尾の `routine-spec: <sha256>` 注記(last-applied 相当)と、宣言/live それぞれから再計算したハッシュを3方比較 |
| update の挙動 | `job_config.ccr` に触れると全置換(2026-09-27 実測)。CLI は常に完全な body を組み立てて送る |
| 実行主体 | ローカルは `SKILL.md`(create・adopt・update を 1:1 中継)。クラウドの自己監査 routine(週次)は `auditor.md` に従い update のみ行う |
| auditor の権限 | update のみ。create はしない(未マージの書き戻し PR がある間の重複 create を避ける — delete API が無い)。自分自身の宣言は report のみ |
| 廃止 | `state: retired` で `enabled:false` 固定。宣言ファイルは残す(delete API が無い) |
| ローカルの宣言外書き込み | `crates/routines-write-guard`(PreToolUse)が、名前空間キー + routine-spec 注記という構造を満たさない `RemoteTrigger` create/update を deny する(bypass なし) |
| sources 網羅性 | `scripts/github-audit` の routines ドメインが、`.claude/routines/` を持つのに auditor の宣言に含まれない repo を検出する |

## 実測(2026-09-27、`RemoteTrigger` + 段0 プローブ)

- API(`/v1/code/triggers`)は list/get/create/update/run/
  create_webhook_trigger。**delete は無い**。`list` は cursor を無視し
  1 ページ目(20 件、newest-first)しか返さない。
- `update` は `job_config.ccr` に触れると全置換。`environment_id` を
  省略すると 400。`session_context` を省略すると `sources`/`model` が
  消え、`allowed_tools` がサーバ既定値に置き換わる。
- cron は UTC 解釈(`0 3 * * *` → next_run_at 03:03 UTC 付近、ジッタ数分)。
- 全 routine で `mcp_connections` が同一の 5 connector(uuid まで一致)=
  アカウント既定セットで、宣言対象ではない。
- `sources` が空の routine が実在する(prompt 内で `gh repo clone` を
  手動実行する形)。家 repo は `sources[0]` から導出できない。
- クラウド sandbox に `gh` CLI は無い(`mcp__github__*` MCP tool を使う)。
  `jq`/`yq` は両方ある。
- GitHub アクセスは routine の `sources` に scope される。
- クラウドの meta connector(`Claude_Code_Remote`)には `delete_trigger`/
  `list_triggers` がある(ローカルの `RemoteTrigger` ツールには無い)。
  delete を使わない設計判断(D8)はこの発見後も維持した。

先行例・各判断の対比は `docs/adr/519-routines-declaration-in-repo.md`
「先行例との対比」を参照(precedent-grounding/selection-grounding スキル)。
