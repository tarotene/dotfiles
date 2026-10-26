# claude-routines — Claude Code routine を as-code で扱う仕組みの設計記録

Claude Code routine(claude.ai 上の scheduled cloud agent)は Web UI・
`/schedule`・`RemoteTrigger` で個別に作るしかなく、定義(prompt・cron・
対象 repo)がどこにも版管理されていなかった。`/grill-me` と、その後の
Plan agent による敵対的レビュー(replan)を経て、対象リポジトリ自身の
宣言を正本にする形に決めた(ADR-503〔rulesets の宣言〕と同型)。

実装は `config/agents/skills/claude-routines/`(手順は `SKILL.md`〔ローカル
session での create/adopt/apply〕と `auditor.md`〔クラウドの自己監査
routine の汎用手順〕、決定論的な差分コアは `scripts/routines-plan.sh` +
`scripts/lib.jq`)。機構側は `crates/routines-write-guard`(段2)と
`github-audit`(`crates/github-audit`)の routines ドメイン(段3)。詳しい設計判断の一覧は
`docs/adr/519-routines-declaration-in-repo.md` を参照。

## 決定表

| 論点 | 決定 |
|---|---|
| 宣言の場所 | 対象リポジトリ自身の `.claude/routines/<name>.{json,md}`(dotfiles には置かない) |
| prompt 本体 | `<name>.md` が正本。live prompt は `Read and follow .claude/routines/<name>.md in <home_repo>.` という薄いポインタ |
| 設定 | `id`/`state`/`cron_utc`/`model`/`environment_id`/`home_repo`/`sources`/`allowed_tools`/`role` と、任意の `auto_mode_allow`/`auto_mode_environment` の JSON |
| 対象範囲 | cron routine の主要項目のみ。run-once/webhook trigger は宣言できない(表現不可能) |
| 同定 | live に書き戻した `trig_` ID を pin。`name` は `<home_repo>:<name>` の名前空間キーで検証 |
| 差分検出 | prompt 末尾の `routine-spec: <sha256>` 注記(last-applied 相当)と、宣言/live それぞれから再計算したハッシュを3方比較 |
| update の挙動 | `job_config.ccr` に触れると全置換(2026-09-27 実測)。CLI は常に完全な body を組み立てて送る |
| 実行主体 | ローカルは `SKILL.md`(create・adopt・update を 1:1 中継)。クラウドの自己監査 routine(週次)は `auditor.md` に従い update のみ行う |
| auditor の権限 | update のみ。create はしない(未マージの書き戻し PR がある間の重複 create を避ける — delete API が無い)。自分自身の宣言は report のみ |
| 廃止 | `state: retired` で `enabled:false` 固定。宣言ファイルは残す(delete API が無い) |
| ローカルの宣言外書き込み | `crates/routines-write-guard`(PreToolUse)が、名前空間キー + routine-spec 注記という構造を満たさない `RemoteTrigger` create/update を deny する(bypass なし) |
| sources 網羅性 | `github-audit`(`crates/github-audit`)の routines ドメインが、`.claude/routines/` を持つのに auditor の宣言に含まれない repo を検出する |

## 自動モードの分類器への許可(`auto_mode_allow` / `auto_mode_environment`)

cloud routine は auto mode で動き、分類器が、共有の資源への書き込みなどを「Modify Shared
Resources」として拒否することがある。`allowed_tools` に道具を足しても、分類器の判定は別なので
通らない(`ArtifactData` が `allowed_tools` にあっても、新しい文書の作成が拒否された事例がある)。
公式文書は、拒否を直す手段として、`autoMode.allow`(`soft_deny` への例外)と
`autoMode.environment`(信頼する先)を、どちらも散文の配列として定義している
(Anthropic, "Configure auto mode", https://code.claude.com/docs/en/auto-mode-config 、取得 2026-10-21)。
project の `.claude/settings.json` は読まれない(リポジトリに入った設定が自分で許可を足せないため)。

宣言に、任意の `auto_mode_allow` と `auto_mode_environment`(文字列の配列)を足せる。`build-body` は
空でないものだけを `session_context` に載せる。**値の順序は意味を持つので sort しない。**
`"$defaults"` を配列に含めないと、その節の組み込みの規則がすべて置き換わる(公式文書の注意)ので、
`"$defaults"` を先頭に置く。

- 空のときは射影に載せない。載せると、このフィールドを持たない既存の宣言のハッシュが変わり、
  全 routine が declared-ahead に見える(`selftest.sh` の 12a が、変更前の値で固定している)。
- **実測(2026-10-22): live は、この経路を受け付けない。** update に `session_context.auto_mode_allow` と
  `auto_mode_environment` を付けて送ったが、update の応答と、別に実行した `RemoteTrigger get` の
  両方で、`session_request.config.auto_mode_allow` と `auto_mode_environment` が `[]` のままだった
  (同じ update の `allowed_tools`、プロンプト、cron、`sources` は反映された)。公式の文書で、この欄の書き方は
  見つけられなかった(検索で見つかった routines の API の文書は fire だけだった。Anthropic,
  "Trigger a routine through the API"、https://platform.claude.com/docs/api/claude-code/routines-fire 、
  取得 2026-10-22。文書の全文は読んでいない)。受理される経路が分かるまで、宣言に書かない。書くと、
  `classify` が、宣言と本番が一致しないので、毎回 declared-ahead と判定し、再適用を促し続ける。
  `build-body` が `auto_mode_*` を載せる機構(`lib.jq`、`selftest.sh`)は、経路が分かったときのために残す。

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

## クラウド到達範囲(ADR-590 の Amendment 2026-10-01)

`.claude/routines/*.json` の宣言が「実行すべき routine」を表す一方、
Claude のクラウド sandbox がその repo に到達できるかは、GitHub の接続方法で
決まり、この repo の宣言では固定できない。

- **Claude GitHub App**(ブラウザで claude.ai/code から接続): public は常に、
  private は App が install された repo にだけ届く。install は
  **Only select repositories** で絞る。到達範囲を絞れるのはこの方法だけで、
  推奨する。
- **`/web-setup`**: `gh` のトークンを Anthropic に送り、そのトークンが触れる
  全リポジトリに届く。fine-grained PAT は使えない(classic の `repo` スコープ
  が必須、2026-10-01 実測)。

`.claude/routines/*.json` を置く private repo は、Claude GitHub App の
install 先に含める。到達範囲の機械的な検出は未実装で、別 Issue で扱う
(`github-audit` は Claude App の install 先を読めない)。詳細:
`docs/github-app-snapshot.md`「Claude's cloud sandbox reach」節。

先行例・各判断の対比は `docs/adr/519-routines-declaration-in-repo.md`
「先行例との対比」を参照(precedent-grounding/technology-selection スキル)。
