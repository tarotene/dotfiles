# ADR-1719 — 画面のデザインの手順と検出器として、第三者のスキル impeccable を、版を固定して配る

- Status: Accepted
- Date: 2026-11-26
- Issue: No-Issue(外部のスキルを試用し、置き換えの検算と本人の判定を経て、配備と、重なる手順の切り崩しを裁定する)

## Context

画面の見た目を直す手順は、`docs/agents/personal-web-app.md` の「画面の作り方」が、Anthropic の frontend-design の手順を 1 行借りて持っていた。借りた手順は、検出器も、繰り返し呼べる批評も持たない。
基準・トークン・部品・ページの型・描画後の検査(ADR-1677・1685・1687・1698・1700・1701・1703・1704・1708・1711・1712)は、自前で持っている。

外部の impeccable(https://github.com/pbakaus/impeccable、Apache-2.0)は、1 本のスキルが 24 のコマンド(`shape`・`critique`・`audit`・`polish`・`typeset` など)を持ち、
LLM を使わない 62 のルールの検出エンジンを同梱する。試用(2026-11-25)で、次を測った。

- 手元の `visual_diff.py audit`(13 検査)と impeccable 4.11.0 の `detect` を、検査ごとの最小の違反例 8 件と旧版の裁定ページにかけた。**impeccable が同じ違反を落とした検査は 0 件**だった。
  逆向きに、impeccable の「AI の癖」の 31 ルールには、手元に該当する検査が無い。重なりは、検査でなく、手順の文書にあった。
- 配備の形の測定(2026-11-26、`impeccable` 4.11.0・エンジン 0.1.30):
  - エンジン(linux-x64、静的リンクの実行ファイル)の sha256 は、上流の `engine-checksums.txt` の行と一致した。
  - 固定したスキルのランチャー(`scripts/impeccable`)は、`IMPECCABLE_BIN` を、検査なしで実行する。未設定のときは、兄弟の実行ファイル、`~/.impeccable/bin/`、版つきのキャッシュ、**PATH 上の `impeccable`**(ハンドシェイクで確かめる)、ダウンロードの順に探す。
  - PATH 上に wrapper を置くと、ランチャーはダウンロードに進まず wrapper を実行した。`impeccable context` と `detect` が動き、`detect --list-rules` は 62 件、`detect` の静的な走査は `connect` の呼び出しが 0 件だった(`strace`)。
- 上流の既定のインストール(`npx impeccable install`)は hook を入れ、サインインを勧める。hook は、サンドボックスの外で、エージェントの環境とネットワークのまま動く(上流の `skill/reference/hooks.md`)。サインインすると、`DESIGN.md` の本文と評価の報告が、リポジトリ名つきでアカウントに送られる。

## Decision

### D1: dotfiles が、flake の入力に rev を固定して、スキルのディレクトリだけを宣言で配る

`flake.nix` に非 flake の入力 `impeccable` を、`skill-v4.11.0` の commit(`88cfafae649164e6dc48b88d3c0c1bf6662eaade`)で固定する。`home/modules/agents.nix` が、入力の `.claude/skills/impeccable` を `~/.claude/skills/impeccable` に、
`.agents/skills/impeccable` を `~/.agents/skills/impeccable`(Codex・Copilot が読む共有の置き場)に、`home.file` で配る。自作スキルの配布(`config/agents/skills/` の `readDir`)とは別の経路にして、
`my-` で始まらないスキルを `/` の一覧から隠す書き換え(ADR-1400)を掛けない。第三者のスキルは upstream の名前と frontmatter のまま使う(`docs/agents/tooling-conventions.md`)。

先行例: `flake.nix` の `frontend-slides`(rev 固定の非 flake 入力。`home/modules/agents.nix` の `home.file` 配備)と、ADR-0009 D2(`/plugin install` は管理外の手動の状態を生むので採らず、flake の入力で固定して `home.file` で配る) (取得 2026-11-26)
差分: 異なる — `frontend-slides` は、description が自作スキルと発火で競合するので、スキルの走査の外に置いた。impeccable は、UI の作業で自動で発火することが価値なので、走査の中に置く。競合は、`personal-web-app.md` に「基準・複写して固定した区間・第三者の書体の禁止・構造の宣言を、impeccable の助言より優先する」と書いて縛る。
軸: 表現不可能 — pin 固定 > 浮動、宣言 > 手続き(手動の plugin install は、管理外の状態を作る)。

### D2: hook・plugin・サブエージェント・サインインは入れない。検出器は、明示して呼ぶ

hook が担う仕事(編集直後に検出の指摘を返す)は、同じ検出器を明示して呼べば足りる。サンドボックスの外でバイナリを実行する経路は持ち込まない。`impeccable context` の出力が `hooks on` を勧めても実行せず、`impeccable login` を実行しない。
スキルの `reference/degraded/` が、サブエージェントが無いときの手順を持つので、サブエージェントも配らない。

先行例: impeccable `skill/reference/hooks.md`(「it runs the engine unsandboxed with the agent's environment and network access」「installed command hooks run even when you deny the model's own commands」)(取得 2026-11-25)
差分: 異なる — 上流は hook を既定で勧める。
軸: 還元 — より安い手段(検出器の明示の呼び出し)で足りる仕事をする機構は、入れない。

### D3: 検出エンジンは、取得時に sha256 を固定し、PATH 上の `impeccable` として配る。ランチャーの初回ダウンロードには任せない

`home/modules/agents.nix` が、エンジンの 4 つの実行ファイル(linux-x64・linux-arm64・darwin-x64・darwin-arm64)を、`fetchurl` の sha256 で固定し、実行権限を付けた derivation にして、
環境変数(`DO_NOT_TRACK`・`IMPECCABLE_NO_UPDATE_CHECK`・`IMPECCABLE_PRO=off`)を設定する wrapper を `impeccable` として `home.packages` に載せる。ランチャーは、PATH 上の `impeccable` をハンドシェイクで確かめて実行する。
固定した値は、スキルの `scripts/VERSION` と `scripts/engine-checksums.txt` と、評価時の `assert` で突き合わせる。入力の rev だけを上げて、エンジンの固定を直し忘れると、評価が落ちる。

先行例: impeccable `scripts/impeccable`(ランチャー。探索順: `IMPECCABLE_BIN` → 兄弟 → `~/.impeccable/bin/` → 版つきキャッシュ → PATH → ダウンロード。ダウンロードは `engine-checksums.txt` で照合する)(取得 2026-11-26)
差分: 異なる — 上流は実行時に取得して照合する。ここでは取得時に固定し、実行時に網へ出ない。
軸: 表現不可能 — pin 固定 > 浮動、実行時の取得より取得時の固定。

### D4: 切り崩すのは、検算で重なりを示せた文書・手順だけ。`audit` の検査は残す

置き換えるのは、`personal-web-app.md` の frontend-design の手順を借りた 1 行(次の PR)と、評価者を人数で決めて 1 回だけ行った審査の手順(`critique` で繰り返し呼べる)である。
`visual_diff.py audit` の 13 検査・`design-pin`・`web_*` の検査・`plan-precedent-gate` の `構造:` の gate は、impeccable に同じ仕事をする担い手が無いので残す。消すと、保護が減るだけで、重なりは減らない。

先行例: 試用の検算(2026-11-25。最小の違反例 8 件と旧版の裁定ページ、`audit` と impeccable 4.11.0 の `detect` の比較。置き換えられる検査 0 件)と、ADR-1703 D3・ADR-1704(デザイン系の第三者スキルは、本体を入れず、足りない観点だけを自前の表・要件に取り込んだ)(取得 2026-11-26)
差分: 異なる — ADR-1703・1704 が扱った Pick UI Library 系は、観点の列挙で、本体の実行物(検出器)を持たなかった。impeccable は、決定論的な検出器と、繰り返し呼べる手順を持つので、本体を入れる。62 のルールを自前の表に写すと、複写と同期が要る。
軸: 還元 — 重複した機構は消し、より安い手段では担えない仕事をする機構は残す。

## Alternatives considered

- **各リポジトリに `npx impeccable install --scope=project`**: 配備物が面ごとに複製され、hook が同時に入る。版の正本が 1 か所にならない。採らない。
- **`/plugin marketplace add pbakaus/impeccable`**: 管理外の手動の状態になる(ADR-0009 D2)。hook も入る。採らない。
- **上流のランチャーに、エンジンの取得を任せる**: 版は、スキルの `engine-checksums.txt` で固定される。差は、実行時に網へ出ることと、宣言の外に状態(`~/.impeccable/bin/`)ができること。採らない。
- **`audit` の検査のうち、impeccable に近いもの(コントラスト・横スクロール)を移す**: 検算で、暗い配色の違反例と最小の横スクロール例を、impeccable は落とさなかった。保護が減る。採らない。
- **62 のルールを、自前の要件の表に複写する**: 上流が増やすたびに、複写の同期が要る。採らない。

## Consequences

- 反映(`hms .`)の後、`/impeccable` が全機体の Claude Code で使え、`impeccable detect` が PATH に載る。Codex・Copilot には、同じスキルが共有の置き場から読まれる。
- 上流の版を上げるときは、スキルの rev とエンジンの固定を同時に直す(`docs/agents/impeccable.md`)。揃わなければ評価が落ちる。
- 上流のカタログへの取得リクエストは、残る(テレメトリを切っても止まらない。上流の README)。静的な走査には、通信が無い(0 件、`strace`)。
- 描画の走査は Chrome を起動するので、エージェントのサンドボックスの中では動かない。手元の端末で行う。
- 管理外の `~/.claude/skills/impeccable` が先にあると、`hms` が `-b backup` で退避する。

## 執行点

- `flake.nix`
- `home/modules/agents.nix`
- `home/tests/agent-skills.nix`

## Verification

- `nix build .#checks.x86_64-linux.agent-skills --no-link`: 配布先に `impeccable` が名指しで入り、`user-invocable: true`(Claude 用)、ランチャーが実行可能、hook の置き場が無いことを検査して通る。評価時の `assert` が、エンジンの版と 4 つの digest を、固定したスキルの `scripts/VERSION`・`engine-checksums.txt` と突き合わせる。
- `home.packages` の `impeccable` をビルドし、固定したスキルのランチャー経由で `detect --json` が動く(終了コード 2)。`detect --list-rules` が 62 件を返す。`strace -f -e trace=connect` の `connect` が 0 件。
- `nix build .#checks.x86_64-linux.dotfiles-tools-clippy --no-link`(CI の clippy の再現)。
