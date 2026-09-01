# herdr-sidebar-metadata — エージェント毎のモード/モデルを Herdr サイドバーに常時表示する

複数の Claude Code エージェントを Herdr で並走させると、「どのペインが plan モードで
止まっているのか」「どれが bypass で走っているのか」がサイドバーから読めない。
Herdr が自前で検出するのは agent_status(working/blocked/…)とターミナルタイトル
だけで、Claude Code 側の permission mode・モデル・context 使用率は Herdr のデータ
モデルに存在しないからだ。

これを Herdr の **pane metadata**(`pane.report_metadata` API + サイドバー行の
`$name` トークン)で埋める。表示側の行定義は `config/herdr/config.toml`、報告側は
Claude Code の hook / statusline スクリプト 2 本(`config/claude/hooks/`)。

## なぜ 2 チャネルか

Claude Code は必要な情報を 1 か所で公開していない:

| 情報 | hook input JSON | statusline JSON |
|------|-----------------|-----------------|
| `permission_mode` | ✅ あり | ❌ 無い(mode 変更時に再実行はされる) |
| model / context% / cost / effort | ❌ 無い | ✅ あり(プリ計算済み) |

そのため `herdr-claude-metadata.sh`(hook、source=`claude-hook`)が mode を、
`claude-statusline.sh`(statusline、source=`claude-statusline`)がモデルとメトリクス
を、それぞれ独立に報告する。トークン名は source 間で完全に分離してあり
(`mode_*` vs `model`/`ctx`/`cost`/`effort`)、Herdr 側のマージ仕様がどちらでも
壊れない。実測では source を跨いでトークン名単位でマージされる(0.7.5、未再検証)。

`herdr-claude-metadata.sh` はもう一つ、hook input JSON の `.cwd` から
`git branch --show-current` を取った `branch` トークンも同じ source で報告する
(`worktree/` プレフィクスは表示幅節約のため落とす)。git 失敗時・非 git cwd では
空 = `null` を送るだけで、herdr 外でも無害。mode の変更検知でスキップする経路
では git を呼ばないので、頻発イベント(PreToolUse)でのコストは増えない。

## モードの色分けは「モード毎に別トークン」で表現する

Herdr のサイドバートークンは `{ token = "$mode", fg = "#..." }` の**静的**スタイル
しか持てず、値によって色を変える手段がない。そこで mode は 1 トークンではなく
4 トークン(`mode_plan` / `mode_default` / `mode_accept` / `mode_bypass`)にし、
hook がアクティブな 1 つにだけ値を入れ、残りを `null` でクリアする。config.toml
側は 4 トークンを同じ行に並べて別々の fg を割り当てる — 値を持つのは常に 1 つ
なので、行には 1 色のモードだけが現れる。0.8.2 のバイナリを逆アセンブルして
確認した制約: サイドバー行トークンのスタイルは `fg`(`#RGB`/`#RRGGBB` 厳格。
named color は不可)+ `bold`/`dim` のみで、`bg` は指定できない。

さらに色だけに頼らないよう、ラベルの記号も形で差別化する(色覚多様性対策。
状態は色 + 形の冗長エンコードが定石):

- `◇ plan` — 輪郭のみ(低リスク)
- `◆ default` — 塗り(基準)
- `✓ accept` — チェック(承認済み)
- `▲ bypass` — 三角(警戒)

未知のモード(`auto` / `dontAsk` など)は `mode_default` トークンに `◆ <実名>`
で流す。古い表示を残すより実名表示のほうが正直。

## 配色: Catppuccin Mocha の役割トークン

Dracula から Catppuccin Mocha への着せ替え(2026-09)で、`config/herdr/config.toml`
/ `claude-statusline.sh` / `config/alacritty/alacritty.toml` の 3 ファイルが共有する
role→color 対応表。hex は ADR-0002 に従い各ファイルにリテラルで置く(共通定義
ファイルは持たない)ので、色を変える際は 3 箇所とも手で揃えること。

| 役割 | Mocha トークン | hex | 使用箇所 |
|---|---|---|---|
| mode: plan | blue | `#89B4FA` | sidebar `$mode_plan` |
| mode: default | mauve | `#CBA6F7` | sidebar `$mode_default`(未知モードもここ) |
| mode: acceptEdits | green | `#A6E3A1` | sidebar `$mode_accept` |
| mode: bypass | red | `#F38BA8` | sidebar `$mode_bypass` |
| model 名 | pink | `#F5C2E7` | sidebar `$model` / statusline model |
| リポ名 | peach | `#FAB387` | statusline 先頭(セッション識別子、狭幅でも残す) |
| ctx OK(<60%) | green | `#A6E3A1` | statusline |
| ctx 注意(60–79%)/ fast | yellow | `#F9E2AF` | statusline |
| ctx 危険(>=80%) | red | `#F38BA8` | statusline |
| cost | teal | `#94E2D5` | statusline |
| effort | lavender | `#B4BEFE` | statusline |
| 控えめ情報(branch, metrics 行) | overlay1 | `#7F849C` | sidebar `$branch`/`$ctx`/`$cost`/`$effort` |
| 区切り | overlay0 | `#6C7086` | statusline `·` |
| sidebar active row 背景 | surface1(手動上書き) | `#45475A` | `config/herdr/config.toml` `[theme.custom].active_row_bg` |
| ui accent(ハイライト/ナビ) | lavender(手動上書き) | `#B4BEFE` | `config/herdr/config.toml` `[theme.custom].accent` |

green=「許可/OK」、red=「危険」、yellow=「注意」で全ファイル一貫させ、
サイドバーと statusline で色→意味が食い違わないようにしている(旧 Dracula 版は
cyan がサイドバーでは plan・statusline では cost という食い違いがあった)。

## イベント選定(hook)

同一 command を 5 イベントに登録する(スクリプトが `hook_event_name` で分岐):

- **SessionStart** — 初期値の報告と、前セッションの残留トークンの上書き。
- **UserPromptSubmit** — 最頻の遷移「アイドル中に Shift+Tab → プロンプト送信」を
  1 プロンプト 1 回のコストで拾う。
- **PreToolUse**(全ツール)— ターン中の遷移(plan 承認 → acceptEdits)を最小
  遅延で拾う。全ツール呼び出しで発火するので、前回報告したモードを
  `$XDG_RUNTIME_DIR/herdr-claude-mode.<pane>` にキャッシュし、同じなら jq 1 回で
  即抜ける(python3 の起動コストを毎回払わない)。
- **Stop** — ttl のリフレッシュ。
- **SessionEnd** — 自 source の全トークンを null クリア(残留表示の即時解消)。

PostToolUse は登録しない — PreToolUse と同じ情報で遅延だけ悪い。

## 間引き(statusline)

statusline はストリーミング中 ~300ms 毎に再実行され得る。ソケット書き込みは
(1) 表示を先に stdout へ出してから detach したサブシェルで行い、
(2) 丸め後の値のフィンガープリントが前回と同じなら送らず、
(3) 前回送信から 2 秒未満も送らない。丸め(ctx は整数%、cost は 2 桁 USD)に
よって、実際の書き込みはターンあたり数回に落ちる。

## 消し忘れ対策

主経路は SessionEnd での null クリア(チャネル A)。チャネル B(statusline)には
終了イベントが無いので、両チャネルとも `ttl_ms = 4h` を保険にする — クラッシュや
kill でも残骸は 4 時間で消える。SessionEnd がチャネル B を `applies_to_source` で
横断クリアできるかは未検証のため、初版では ttl 任せにしている。

## アクティブ行の視認性(`[theme.custom]`)

catppuccin テーマ既定の `active_row_bg` は base(`#1E1E2E`)とほぼ同系の暗色で、
~17 workspace を並走させるとサイドバーのどの行がフォーカス中かが判別しづらい。
`config/herdr/config.toml` の `[theme.custom]` でテーマの上から
`active_row_bg = "#45475A"`(surface1)・`accent = "#B4BEFE"`(lavender)を
上書きし、ハイライトとナビ UI 全体で明確な差を作る。`herdr --default-config`
(0.8.2)で確認した通り `[theme.custom]` はテーマ本体を書き換えずに個別トークン
だけ差し替えられるので、テーマ更新に追従したまま維持できる。

## 既知の制約・運用ノート

- **statusline の巻き戻り**: `~/.claude/settings.json` の `statusLine` は activation
  (`registerClaudeStatusLine` → `syncStatusLine`)が宣言値に合わせるので、
  `/statusline` で手動変更しても次の `home-manager switch` で戻る。変更はこの
  リポジトリの `config/claude/hooks/claude-statusline.sh` を編集すること。
- **hook / statusLine の撤回は forward switch でのみ効く**: `registerHooks` /
  `syncStatusLine` は `retiredHookEntries` / `retiredStatusLineCommands`
  (`home/modules/claude.nix`)に載っている command を完全一致で settings.json から
  削除する。これが効くのは新しい generation への **forward** switch だけで、
  home-manager generation の `--rollback` では効かない — rollback 先の世代の
  activation は当時のコードをそのまま実行するため、撤回機構自体がまだ無い世代に
  戻れば孤児が再発し得る(`retiredPermissionRules` も同じ限界を持つ、
  home-manager の generation モデル一般の制約)。緊急 rollback 後は
  `docs/operations.md` の孤児チェックを走らせること。
- **herdr 統合 hook との共存**: herdr は自分の `herdr-agent-state.sh`(編集禁止、
  integration 更新で上書き)を settings.json に登録する。registerHooks は command
  文字列が異なるエントリに触れず、herdr も自ファイル以外に触れないので衝突しない
  (herdr の書き込みは `.hooks.SessionStart` の自ファイルと `~/.claude/hooks/`,
  `~/.codex/`, `~/.copilot/hooks/`, `~/.config/devin/` への統合ファイル配備だけで、
  `statusLine` には一切触れない — 実行バイナリの静的解析で確認済み)。
- **Herdr 外では無害**: どちらのスクリプトも `HERDR_ENV=1` と socket/pane 環境変数を
  ガードにしており、素のターミナルでは statusline の表示だけが動く(ADR-0005 の
  binary-existence gating に倣い、欠如時は黙って no-op)。
- **herdr バイナリは nix 管理**(`home/modules/herdr.nix`)。nixos-26.05 に herdr が
  無いため flake の `nixpkgs-unstable` input から overlay で取っている — 安定
  チャネルに入ったら input ごと畳むこと(Issue #42)。旧 self-installed
  `~/.local/bin/herdr` は `home.activation.quarantineSelfInstalledHerdr` が
  `.pre-nix` へ自動的に退避する(手動削除の手順は置かない — ADR-0001 のゼロ手
  作業の原則)。
- **config.toml は store symlink**。herdr の実行時書き込み(in-TUI の theme /
  sound / toast / status indicators / agent border labels トグル、onboarding、
  channel set)はすべて失敗する。CLI 経路(`herdr channel set`)は
  `Permission denied` を返して停止することを実機で確認済み。in-TUI 経路は
  `logging::config_write_failed` に記録されて飲み込まれ、UI のトグルが黙って
  元に戻るだけ(見える失敗ではない)。設定変更はこのリポジトリの
  `config/herdr/config.toml` を編集して `home-manager switch` + 反映は
  `herdr server reload-config`。
