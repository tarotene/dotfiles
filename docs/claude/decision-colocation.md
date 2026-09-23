# decision-colocation — 決定成果物と執行点を同じ PR に強制する

設計判断の記録: `docs/adr/<番号>-decision-colocation.md`
checker(単一ソース): `scripts/decision-colocation-check`
サーバ側 required check: `.github/workflows/ci.yml` の `dry-run` job
client guard: `config/claude/hooks/decision-colocation-guard.sh`(段2)
規範: `config/agents/AGENTS.md`「決定成果物は執行点と同じ PR に出す」(段2)

「ADR だけ残して実装を後続 Issue に先送りする」判断をエージェントができない
ようにする決定(ADR-0000)の判定エンジン単一ソース。repo 内の実測(27 本の
ADR のうち 12 本が docs-only、うち 2 本は追跡 Issue が今日まで一度も
作られていない)を根拠にする。

## トリガと検査

| トリガ | 検査 |
|---|---|
| `docs/adr/**` への新規ファイル追加 | そのファイルが `## 執行点` を持つ |
| `docs/claude/**` / `config/claude/skills/**` への新規ファイル追加 | PR の diff 全体に非 `.md` 非 `docs/` のパスが 1 つ以上ある(節は要求しない) |
| 既存 `docs/adr/*.md` への `^## Amendment` 見出しの追加 | その Amendment ブロックが `### 執行点` を持つ |

いずれのトリガにも触れない diff(索引修繕・typo・既存ファイルの編集のみ)は
無条件で適合。既存 ADR への遡及適用はしない — ADR-0007 の「既存ファイルの
一括リネームはしない」、ADR-0020 の `createdAt` grandfathering、ADR-380 の
「`n < 0100` は grandfathered」と同じ扱い。

`docs/adr/**` への新規追加は同時に「ADR 以外」のトリガにも該当しうる
(`docs/adr/*.md` は `docs/claude/**`/`config/claude/skills/**` のどちらにも
当たらないため実際には排他だが、1 PR が両方のトリガを持つケース — 例えば
新規 ADR + 新規 skill を同じ PR に含める — では両方の検査が独立に走る)。

## `## 執行点` の書式

```markdown
## 執行点

- config/claude/hooks/foo-gate.sh
- `home/modules/claude.nix` — hook 登録
```

- 箇条書き 1 行 1 パス。バッククォートは任意。パスの後にスペース区切りで
  説明を続けてよい(最初の空白区切りトークンだけをパスとして読む)。
- 列挙した全パスは実在しなければならない(壊れた参照の禁止)。
- そのうち少なくとも 1 つが「執行点として認めるパス」であり、かつこの PR
  の diff(base..HEAD)に含まれていること。

Amendment ブロック内では見出しを `### 執行点`(H3)にする — Amendment 自体
が `## Amendment (...)` という H2 見出しのため、その配下は H3 から始まる。

## 執行点として認めるパス

「非 `.md` かつ `docs/` 配下でない」の 2 述語のみ。パス分類台帳を持たない
— repo 内の既存 ADR を全数検算した結果、決定成果物は例外なく `.md`、執行
実体は例外なく非 `.md` だった(`config/shell/profile` のような拡張子なし
ファイルも非 `.md` として扱われるため取りこぼさない)。新しい文書置き場が
増えても同期漏れという不正状態そのものが発生しない。

## なぜ「実在するだけ」では足りないか(D5)

執行点のパスが実在するだけでは合格にしない。少なくとも 1 つがこの PR の
diff に含まれる(新規または変更)ことを要求する。

実例: ADR-387(`wrapup-chores` を裁定前倒し型に反転する決定)の導入 PR は
6 ファイル全てが `.md` で、本文は「検査器は新設せず `plan-scope-gate.sh`
を再利用し」と明言している。`plan-scope-gate.sh` は実在するが、この PR
では一切変更されていない。「既存の何かが執行する」という主張は、その
再利用が成立することを示す変更(selftest への新規ケース追加など)を伴わ
なければ、検証を伴わない自己申告と区別できない。これを許すと、ADR-0024
が `## 執行点: config/claude/hooks/stack-base-guard.sh`(実在するが無変更)
と書くだけで docs-only のまま抜けられてしまう。

本 ADR は ADR-387 をこの規則の下で意図的に不合格として扱う(遡及適用は
しない — ADR-387 自体は grandfathered)。

## 部分実装は許す(D6)

「決定の完全実装」は ADR 本文から対象範囲を導けず機械判定不能であり、
判定不能な要求は自己申告に堕ちる。執行点が 1 つでもあれば合格とし、残り
の後続 Issue 化は自由にする(walking skeleton / tracer bullet の型)。
ADR-0024 で言えば、Rust 移植を 1 本だけ同梱すれば合格し、残り 39 本の
移植は依然として後続 Issue に出せる。

## AskUserQuestion を gate しない理由(D7)

CI required check が「実装を後続 Issue に分離する」という選択肢を実行
不能にした以上、`AskUserQuestion` の選択肢文言を語彙マッチで deny する
新しい機構には仕事が無い。この repo には既に「`AskUserQuestion` の選択肢
空間は機械検査に向かない」という裁定(`docs/claude/stacked-pr.md:57-61`)
があり、自由記述への語彙マッチは言い換えとのいたちごっこになる
(`docs/claude/scope-inventory.md:113-116` が `Conflicts:` タグを同じ理由
で棄却した先例と同型)。代わりに `config/agents/AGENTS.md` に規範として
明記する(段2)。

## `github-audit` にドメインを作らない理由

本件の判定はこの repo 固有のパス述語(`docs/adr/`・`docs/claude/`・
`config/claude/skills/` というこの repo 特有のディレクトリ構成)に依存し、
横断適用できない。`adr-numbering.md` が同じ理由で `github-audit` ドメイン
を作らなかったのと同型。

## `pr-gate.sh` に新判定を足さない理由

`pr-gate.sh` の既存 `G_CI` 判定が「required check が失敗していれば Stop を
ブロックする」という仕事を既に持つ。`decision-colocation-check` が CI
required check として登録されていれば、それだけで `G_CI` 経由で Stop も
ブロックされる — 新しい `G_*` を足す仕事が無い(還元性)。

## スコープ外(意図的)

- Codex / Copilot 向け adapter — CI required check が全エージェント共通の
  backstop として機能するため、client 側は Claude Code のみで足りる。
- ExitPlanMode gate — 計画本文のパースは本質的に曖昧で、CI が既に表現
  不可能性を担保している。ExitPlanMode の PreToolUse は既に 5 本直列
  している。
- 既存 ADR のバックフィル — トリガの定義上、対象外(遡及適用しない)。
