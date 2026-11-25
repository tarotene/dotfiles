# impeccable — 画面のデザインの手順と検出器を、版を固定して配る第三者スキル

実装: `flake.nix`(入力 `impeccable`)、`home/modules/agents.nix`(スキルの配備、検出エンジンの固定、`impeccable` コマンド)、`home/tests/agent-skills.nix`(配布範囲の検査)
決定: `docs/adr/1719-impeccable-pinned-skill.md`
上流: https://github.com/pbakaus/impeccable (Apache-2.0。Paul Bakaus。スキル 4.11.0、エンジン 0.1.30、2026-11-24)
隣の標準: [`personal-web-app.md`](personal-web-app.md)(画面の作り方。基準・トークン・検査の正本は `config/web/`)

## 何を配るか

| 配るもの | 置き場 | 中身 |
|---|---|---|
| スキル(Claude Code) | `~/.claude/skills/impeccable` | `SKILL.md` + `reference/` + `scripts/`(ランチャー)。`/impeccable <command>` で呼ぶ |
| スキル(Codex・Copilot) | `~/.agents/skills/impeccable` | 同じ内容の共有版(`$impeccable <command>`) |
| 検出エンジン | `impeccable` コマンド(PATH) | 固定した版の静的バイナリ。環境変数でテレメトリ・更新確認・Pro を止める薄い wrapper |

スキルは、24 のコマンド(`shape`・`critique`・`audit`・`polish`・`typeset`・`layout` など)を持つ 1 本。検出エンジンは、LLM を使わない 62 のルール
(`impeccable detect --list-rules`)で、「AI の癖」(`side-tab`・`gradient-text`・`nested-cards` など)と、描画の品質(`low-contrast`・`tiny-text`・`text-overflow` など)を見る。

スキルは `config/agents/skills/` の外から、flake の入力(rev 固定)で配る。`my-` で始まらない自作スキルを `/` の一覧から隠す書き換え
(`docs/agents/tooling-conventions.md`)は、第三者のスキルには掛けない。`/impeccable` で呼べることがこのスキルの価値だからである。
`checks.<system>.agent-skills` が、配布先の集合に `impeccable` が名指しで入っていること、`user-invocable` が上流のままであること、
ランチャーが実行できること、hook の置き場が無いことを検査する。

## 入れないもの

上流の既定のインストール(`npx impeccable install`)は、次の 3 つまで入れる。ここでは入れない。

- **hook**(編集のたびに検出器を動かす)。上流の `reference/hooks.md` は、hook が「サンドボックスの外で、エージェントの環境とネットワークのまま動く」こと、
  Claude Code ではモデル自身のコマンドを拒否しても動くことを記している。同じ仕事は、検出器を明示して呼べば足りる。`impeccable context` の出力が
  `hooks on` を勧めても、実行しない。
- **plugin**(`/plugin marketplace add pbakaus/impeccable`)。管理外の手動の状態になる(ADR-0009 D2)。
- **サインイン**(`impeccable login`)。サインインすると、`DESIGN.md` の本文と評価の報告が、リポジトリ名つきでアカウントに送られる。実行しない。

サブエージェント(`.claude/agents/impeccable-*`)も配らない。スキルの `reference/degraded/` が、サブエージェントが無いときの手順を持つ。

残る通信: 新しいデザインを始めるとき、上流のカタログ(impeccable.style)へ取得のリクエストが出る。上流の README は、テレメトリを切っても止まらないと記す。
静的な走査(`impeccable detect <ファイル>`)は、`strace` で `connect` が 0 件だった(2026-11-26、`scripts/impeccable` 経由)。

## 使い方

- 手順: `/impeccable shape`(計画のみ。構造の選択が先)→ 実装 → `/impeccable critique`(読み取りだけの評価)→ `/impeccable polish`。
  `/impeccable context`(セッションの最初に 1 回。`PRODUCT.md`・`DESIGN.md` を読み込む)は、ランチャーが走らせる。
  画面の基準(`config/web/`・ADR-1677)・複写して固定した区間(`design-pin`)・第三者の書体の禁止・構造の宣言は、impeccable の助言より優先する。
- 検出器: `impeccable detect --json <ファイル|ディレクトリ>`。終了コードは 0(指摘なし)・2(primary の指摘あり)・1(走査できない)。
  描画の走査(`file://…`、`--viewport 390x844`)は Chrome を起動するので、エージェントのサンドボックスの中では動かない。手元の端末で打つ。
  CSS・JSX・Svelte・Astro のファイルには、パターンのルールだけが効く。`low-contrast`・`tiny-text` などは、HTML か描画の走査でしか動かない。
- 手元の `visual_diff.py audit`(描画後の HTML を測る検査)を、置き換えない。最小の違反例と旧版の裁定ページで検算した結果、`audit` の検査のうち
  impeccable が同じ違反を落としたものは 0 件だった(ADR の D4)。

## 更新の手順(版を上げる)

スキルの rev と、エンジンの固定は、同じ版に揃える。揃っていないと、評価時に落ちる(`home/modules/agents.nix` の `assert`)。

1. `flake.nix` の `impeccable` の rev を、上げたいタグ(`skill-v<版>`)の commit にする。`nix flake lock --update-input impeccable`。
2. `home/modules/agents.nix` の `impeccableEngineVersion` を、新しい `<スキル>/scripts/VERSION` に合わせ、`impeccableEngineAssets` の 4 つの sha256 を、
   上流の `engine-checksums.txt`(同じ rev の `.claude/skills/impeccable/scripts/engine-checksums.txt`)の値に直す。
3. `nix build .#checks.x86_64-linux.agent-skills` が通ること。`impeccable detect --list-rules` が通ること。

Renovate は、rev を直接書いた URL の入力を上げる PR を作らないことがある(推測。確かめていない)。上の手順が正本である。

## 反映と元に戻す

`hms .` で反映する。反映前に、同じ場所に管理外の `impeccable` が無いこと(`~/.claude/skills/impeccable`・`~/.agents/skills/impeccable`)を確かめる(2026-11-26 に、この機体で無いことを確かめた)。
外すときは、`flake.nix` の入力、`home/modules/agents.nix` の 4 つの定義(let の束縛と 3 つの `home.file`/`home.packages`)、`home/tests/agent-skills.nix` の `thirdParty` を消す。
