# agent-mode — 決定の記録に縛られない整理のための、セッション単位のモード

実装: `crates/agent-mode`(Rust)。宣言: `config/agents/modes/<名前>.toml`(配備先 `~/.config/dotfiles/agent-modes/`)。
起動: zsh の `claude` 関数(`config/zsh/modules/53-tools-claude.zsh`)。
決定の記録: ADR `docs/adr/1633-agent-session-mode.md`(仮番号。PR を作ったあとに改番する)。
Issue: No-Issue(巨大なリポジトリで、ADR のような覆しにくい決定に引きずられずに整理を進めたい、という本人の依頼。2026-11-18)

## 動機

決定の記録(ADR・設計文書)は、accepted のあとは変えず、覆すときは新しい ADR を足す。これは普段は健全だが、巨大なリポジトリで
大きな整理をするときは、エージェントが記録を読み込む量と、記録に合わせようとする拘束が、整理の邪魔になる。
dotfiles では、`docs/adr` が 96 本(約 1.1 MB)、`docs/agents` が 97 本(約 1.1 MB)、毎セッション読まれる共通規約が
1,084 行ある(2026-11-18 に数えた)。

欲しいのは、**そのセッションだけ**「決定の記録を読まず・書かず、落ちる検査(テスト・型・CI)だけに従って、破壊的に変える」
状態に切り替える 1 つの印である。ただし、安全に関わる検査(外部送信・機体の契約の外の非公開リポジトリ・本物の checkout の
保護)は止めない。

## 使い方

```
CLAUDE_MODE=refactor claude          # モードで起動する
agent-mode check refactor            # 展開の結果(設定の JSON・追記する文・環境変数)を、起動せずに見る
```

- `CLAUDE_MODE` が空なら、これまでどおりの通常のセッション。
- モードが指定されたのに、`agent-mode` が無い・宣言が読めない・検証に落ちるときは、**通常のセッションに倒さず、起動しない**
  (拘束を外すつもりで、拘束したままの(または、外れたつもりの)セッションを黙って始めないため)。
- `claude update` / `claude upgrade` はセッションではないので、モードを無視する。
- resume するときも、同じ `CLAUDE_MODE` を付けて起動する。

## 宣言の書式

`config/agents/modes/<名前>.toml`。未知のキーは受けない。

| キー | 意味 |
|---|---|
| `description` | 1 行の説明 |
| `prompt` | 追記する文のファイル名(同じディレクトリ)。`--append-system-prompt-file` で渡す |
| `injection` | `"off"` で、セッション開始時の文脈の注入を止める(下の「セッション開始時の注入を止める」。`inject-gate` が読む) |
| `skip` | 止める検査の stem(`crates/agent-session/src/escape.rs` の `ALL`)。`SKIP_<STEM>=1` が立つ |
| `[records].globs` | 読めず・書けないパス(gitignore 形式)。`permissions.deny` の `Read(..)` と `Edit(..)` になる |

`skip` の stem は、逃げ道の表が唯一の正本で、宣言には stem だけを書く(名前の文字列を手で書かない。ADR-1040 の
2026-11-09 の Amendment)。表に無い stem と、呼び出し単位だけの guard(セッション単位を持たないもの)は、起動を失敗させる。

## 何が効いて、何が効かないか

- **読み取りの禁止**: Claude Code の `Read` の deny 規則は、同じパスへの Edit・Write も止める。Bash の `cat` などの
  ファイルを読む既知のコマンドと、リダイレクトの対象にも効く。Grep・Glob には best-effort で効く
  (Anthropic, "Configure permissions", https://code.claude.com/docs/en/permissions, 取得 2026-11-18)。
  ファイルを名指しせずに読むコマンド(`grep -r pattern .`)や、スクリプトが自分で開くファイルには、sandbox を有効にしない限り効かない。
  したがって、第 1 段のモードで記録を**書けない**のは、この性質による(覆す ADR は通常のセッションで足す)。
- **止める検査**: `SKIP_<STEM>=1` はセッション単位の逃げ道で、hook のプロセスの環境変数として読まれる。hook は
  起動したシェルの環境変数を受け継ぐ(Anthropic, "Hooks reference", https://code.claude.com/docs/en/hooks, 取得 2026-11-18)。
- **止まらないもの**: CI の検査(`decision-colocation-check`・`adr-number-check`・`docs-index`)は、セッションのフラグでは外れない。
  `pr-gate` も止めない(PR を作る完了の定義と、closing keyword・stack の検査まで外れるため)。共通規約の本文
  (`@~/.agents/AGENTS.md` の import)は、段1 では読み込まれたままで、追記する文で「このセッションでは適用しない」節を宣言する
  (文脈として扱われるので、守られる保証はない。段3 が、読み込まれない状態にする)。

## セッション開始時の注入を止める(`injection = "off"`)

`home/modules/agents.nix` の `claudeHookDeclarations` のうち、SessionStart の宣言は、bool の `injects` を必ず持つ。付け忘れ・
SessionStart 以外への付与は、nix の評価で落ちる(`injectGate`)。

| `injects` | 意味 | 該当(2026-11-18) |
|---|---|---|
| `true` | 文脈を足すだけの hook。Claude の登録だけを `agent-mode inject-gate -- <元のコマンド>` で包む | `issue-index`・`esa-mirror-context`・`daily-desk-reminder`・`worklog`・`herdr-status-advisory`・`gate-failopen-advisory`・`worktree-audit-context`・`calendar-handshake-reminder` |
| `false` | 注入のほかに、状態の書き込み・登録を担う hook。包まない | `session-wrapup-start`(feedback の印)・`pr-gate-session-start`(Stop の gate の対象登録)・`gpg-prewarm`・`worktree-base-sync`・`herdr-agent-metadata` |

- `inject-gate` は、`DOTFILES_AGENT_MODE_INJECTION=off`(モードの `injection = "off"` が立てる)のとき、元の hook を実行せず、
  無音で終わる。そうでなければ、元の hook をそのまま exec する(標準入力・終了コード・出力は変わらない)。
- Codex・Copilot の登録は包まない。command が変わると、Codex の hook の信頼が、機体ごとにやり直しになる。
- 残る注入: `session-wrapup-start`(未裁定の気づきの案内)と `pr-gate-session-start`(残骸ブランチの通知)。どちらも、
  状態の書き込み・登録と同じ hook の中にある。注入だけを止めるには、hook の中で出力を止める必要がある(後続の改善)。

## 2 段階の運用

1. **第 1 段(このモード)**: 記録を読めも書けもしない。拘束するのは、落ちる検査だけ。検査を消す・変えるのは自由だが、PR 本文の
   `## 触れた決定` に、消した・変えたものを残す。PR は `--draft` で出す。
2. **第 2 段(通常のセッション)**: 一覧に出た決定だけを読み、覆す決定ごとに、新しい ADR(見出しの `- Supersedes:` で宣言する。
   ADR-0008 の 2026-11-09 の Amendment)を、**同じブランチ**に足す。その ADR の執行点は、第 1 段の変更に含まれる。

`## 触れた決定` の節は、`decision-colocation-touched --base origin/main` が出す(`docs/agents/decision-colocation.md`
「旧 ADR の執行点に触れた一覧」)。base 時点の ADR の `## 執行点` のうち、変更・削除・改名されたものを ADR ごとに出し、
あわせて、消した・変えたテストと CI の設定を出す。PR 本文にそのまま貼る。
通常のセッション(第 2 段)は、一覧の各 ADR について、覆すなら新しい ADR を足し(見出しに `- Supersedes: ADR-<番号>`)、
維持するなら理由を PR 本文に 1 行書く。一覧が空のときは、旧 ADR の執行点に触れていない。

## 実測(段1)

次の 4 点は、plan mode では実行できなかったので、実装の段で確かめる。結果は、確かめたあとに、この節へ書く。

- (a) `--settings` の `permissions.deny` が、既存の設定に足し合わされ、`**/docs/adr/**` がどこでも当たるか。
- (b) Read の deny が、Bash の `cat` と、sandbox の中の子プロセスにも効くか。
- (c) hook が、起動したシェルの環境変数(`SKIP_*`)を継承するか。
- (d) `--append-system-prompt-file` が、resume でどう扱われるか。

未確認: herdr のペインが zsh の `claude` 関数を通るか。通らない場合は、`agent-mode exec refactor -- claude` を直接呼ぶ。

## 出典

- Anthropic, "CLI reference", 公開日記載なし, https://code.claude.com/docs/en/cli-reference (取得 2026-11-18)。`--settings`・
  `--append-system-prompt-file`・`--bare` の各行。
- Anthropic, "Configure permissions", 公開日記載なし, https://code.claude.com/docs/en/permissions (取得 2026-11-18)。
- Anthropic, "Settings files and precedence", 公開日記載なし, https://code.claude.com/docs/en/settings (取得 2026-11-18)。
  `--settings` はコマンドラインの層で、配列は層をまたいで足し合わされる。
- Anthropic, "Hooks reference", 公開日記載なし, https://code.claude.com/docs/en/hooks (取得 2026-11-18)。
