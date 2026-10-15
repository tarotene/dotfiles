# ADR-0000 — 社内 wiki のミラーの所在を SessionStart hook で注入する(esa-index)

- Status: Accepted
- Date: 2026-10-15
- Issue: No-Issue(別リポジトリの設備調達の Issue を読んでいる最中に、社内 wiki
  の日次ミラーから設備仕様が取れると分かり、「wiki を見に行かないことが増えた」ので
  社内限定のルール、というよりフック扱いにしたい、と依頼された)

## Context

社内 wiki(esa)の全記事は、private リポジトリに毎日ミラーされ、ローカルに clone してある。
エージェントが社内の事実(設備・規程・手順・用語)を述べる前にこれを読む運用が、守られなく
なってきた。共有 AGENTS.md に一文足しても、実行はモデルの気分次第になる
(`docs/claude/issue-index.md`)。

さらに制約が 2 つある。

- ミラーの場所と対象 org は私的な値で、公開リポジトリのソースに書けない(ADR-0034)。
- 個人の esa MCP(ADR-0022)は個人 identity にしか配らず、会社ホストには無い。ランタイムで
  MCP 経由で読む経路は、会社ホストでは使えない。

## Decision

1. **SessionStart hook `esa-index` を新設する**(`startup|resume|compact`)。cwd の origin owner
   が設定の org に含まれるとき、ミラーのパス・検索手順・引用形式・最終ダンプ日を
   `additionalContext` で注入する。fetch はしない。
2. **実値は private wrapper flake が宣言する**。公開側は `dotfiles.esaIndex.{mirrorPath,orgs}` の
   口だけを持ち、両方が設定されたときだけ `~/.config/esa-index/config.json` を描画する。
   設定が無いホストでは hook は無音で終わる。
3. **鮮度は既存の `git-checkout-freshness` タイマーに任せる**。対象に足すため
   `dotfiles.checkoutFreshness.extraPaths` の口を足す。
4. **出力は ADR-625 の `<hook-directive>` 形式**とし、ADR-625 に Amendment を足す。新規 crate
   に限って許可リストに加える。
5. **静的な規範は共有 AGENTS.md の 1 行に留める**(「組織内の先行実装・過去議論」に社内 wiki
   とそのローカルミラーを加える)。社名・URL は書かない。

## Alternatives considered

- **共有 AGENTS.md / CLAUDE.md の散文だけ**: 私的な値を書けず、実行もモデル任せになる。棄却。
- **esa MCP をランタイムで読む**: 会社ホストにトークンの復号経路が無い(ADR-0022)。棄却。
- **プロジェクトスコープの hook(会社リポジトリごとの `.claude/settings.json`)**: リポジトリ
  ごとに複写が要る。感触で外した。分析ではない。
- **ホストに手置きする設定ファイル**: 宣言ではなく手続きになる。wrapper flake の方が単一
  正本になる。棄却。
- **UserPromptSubmit でプロンプトの語を index に突き合わせて該当記事を直接注入**: 参照の
  強制度は最も高い。日本語の語抽出の品質が読めないので、感触で外した。分析ではない。
  SessionStart の注入で足りなければ昇格先にする(ADR-543)。

## Consequences

- 設定を注入したホストでは、会社の org のリポジトリのセッションに、起動のたびに注入ブロックが
  1 つ入る(英語本文、十数行)。
- 鮮度の表示はダンプのコミット時刻で、タイマーの健全性とは区別できない。
- 個人ホストと wrapper 未適用のホストでは何も変わらない。
- 「観察された運用が記事と違うときは明示する」を注入文に含めたので、記事に従うだけの運用を
  強めない(記事が移行に追随せず、規程と実務が乖離した前例がある)。

## 執行点

- crates/esa-index/src/lib.rs
- crates/esa-index/src/main.rs
- home/modules/esa-index.nix
- home/modules/checkout-freshness.nix
- home/modules/claude.nix
- home/modules/worktree.nix
- config/agents/AGENTS.md

## Verification

- `cargo test -p esa-index` と `cargo test -p wrapup-stop-gate --test directive_scope`。
- `nix fmt`、`nix flake check`。
- 注入を設定したホストで新規セッションを開き、システム通知に注入ブロックが入ること、個人
  リポジトリでは入らないことを見る。
