# ADR-0027 — セッション内 PR は依存予測をやめ常時単一チェーンに積む(uncertainty-first stacking)

- Status: Accepted
- Date: 2026-09-21
- Issue: No-Issue(ユーザー依頼の grill-me セッションで確定)
- Supersedes: `config/claude/skills/stacked-pr/SKILL.md` §1 の判定条件
  (a)/(b) を「積むか否か」の判定としては廃止する(段の切り方の設計原則
  としては §2 に存続させる)。`docs/claude/stacked-pr.md` §8「なぜ
  pr-gate.sh を触らなかったか」の裁定を上書きする。ADR-0024 の「新規 hook
  は Rust を既定とする」から D6 の範囲で明示的に逸脱する(Consequences 参照)。

## Context

2026-09-21、ある private リポジトリでの開発セッションで 1 セッション約 10
本の PR を作成した際、ブランチは物理的に直列に積まれていたのに PR の
`base` 宣言が不整合になった(具体的にどのリポジトリかは ADR-0014 の方針に
より本 ADR には書かない — private/company リポジトリ名は dotfiles の
成果物に残さない):

- ある PR は先行 PR の head を `base` にすべきところ default branch のまま
  積み残され、他の複数 open PR のコミットを含む汚染 diff になっていた
  (先行 PR の作り直しとして、実体は別の PR の上に積まれていたため)。
- `gh stack link` が実行されないまま Web UI で手動 stack を試み、
  束ねきれない orphan PR が発生した。
- 結果として並列 2 チェーン + 独立 PR 1 本に分裂した。

`config/claude/skills/stacked-pr/SKILL.md` §1 の既存規律は、後続の変更が
先行 PR の成果物を参照するか(a)、同一ファイルの同じ節を逐次編集するか(b)
を判定してから積むかどうかを決める、依存予測ベースの設計だった。この判定は
セッション中の LLM 判断に委ねられており、今回の事故ではその予測が系統的に
外れた。

`docs/claude/stacked-pr.md` §8「なぜ pr-gate.sh を触らなかったか」は、
機械的な `G_stack` block(「親のコミットを含むのに base が default branch」
の取り違えを検出する案)を検討した上で明示的に保留していた:

> 今回は指示文とスキルの運用を先に確立し、実際に取り違えが起きてから
> block 化を検討することにした。

上記のインシデントは、この保留条項の発火条件そのものである。

ユーザーの提案は次の通り: 機能的な依存関係とコード競合ベースの依存関係は
別物であり、後者は実際に PR を作るまで予測できない。だから前提を
「予測できる」側ではなく「予測できない」側に倒し、セッション内で複数 PR を
作るときは常に単一チェーンに積む前提を機械的に強制する。

この考え方を調査・評価した結果、支持する:

1. 1 つの worktree でのセッション内作業は物理的に直列(各ブランチは直前の
   状態の上に積まれる)。「独立 PR として `base:main`」と宣言する方が
   フィクションであり、そのフィクションを保つためのブランチ操作
   (`#112` の作り直し)が汚染を誘発した。
2. コストの非対称性が明確: 本来独立な変更を誤ってチェーンに入れてしまう
   コストは低い(`gh stack merge` の atomic prefix merge で下から順に
   マージできる。solo 開発・即セルフマージ運用では待ち行列コストも小さい)。
   一方、本来依存する変更を誤って並列に出してしまうコストは高い(orphan・
   汚染 diff・手動修復)。不確実な状況では、常に妥当な唯一の構造(作成順の
   線形チェーン)を選ぶ方が合理的。
3. `docs/claude/stacked-pr.md` の保留条項が明示的に想定していた発火条件が
   成立しており、block 化は場当たり的な追加ではなく既定路線の実行にあたる。

## Decision

### D1: セッション内の複数 PR は依存を予測せず常に作成順の単一チェーンに積む

`config/claude/skills/stacked-pr/SKILL.md` §1 の判定条件 (a)/(b) は、
「stack するかどうか」の判定としては廃止する。同一セッション・同一
worktree で 2 本目以降の PR を作るときは、常に直前の PR の head branch を
`base` にする。(a)/(b) 自体は「段の切り方(何を 1 段にまとめるか)」の
設計原則として §2 に存続させる — 廃止するのは予測に基づく分岐判断であり、
段構成のガイドラインではない。

### D2: 離脱は閉じた本文タグ `Independent-PR: <理由>` のみ

真に独立な PR(例: 無関係な緊急 hotfix)をチェーンから外す必要があるときは、
PR 本文に `Independent-PR: <理由>` を書く。理由は非空でなければならない。
このタグは `No-Issue:` / `No-Visual:` / `No-Attribution:` と同じ「理由必須の
閉じたタグ」家系であり、**Claude 自身の判断(依存の有無を先読みした結果)で
チェーンから外れることはできない** — 外れる決定はこのタグを書く行為でのみ
成立し、外れて良いかどうかの判断そのものは行わない。

### D3: 強制は作成時と完了時の両端に置く

- 作成時: 新規 PreToolUse hook `stack-base-guard.sh` が `gh pr create` /
  `gh pr edit --base` / 相当する MCP GitHub ツール呼び出しを検査し、base
  が誤っていれば deny する。
- 完了時: `pr-gate.sh`(Stop hook)に `G_stack` judgement を追加し、
  セッション内チェーンが GitHub 上の stack にリンクされていなければ
  ターン終了を block する。

### D4: 二層の状態設計

- 層(i) 状態レスの祖先一致検査: HEAD が他の open PR のコミットを祖先として
  含む場合、`base` はその PR の head branch でなければならない。これは
  `Independent-PR:` タグでも抜けられない — 祖先を物理的に含む以上、base を
  別にすれば差分汚染が機械的必然になるため(`#112` 型事故の直接的な原因)。
  状態を持たないため、手動操作や別セッションからの操作にも効く。
- 層(ii) セッション ID 単位の状態: セッション内で作成した PR の head
  branch を記録し、2 本目以降でチェーン外のブランチ(`main` 等から新規に
  切ったブランチ)から PR を作ろうとした場合は `Independent-PR:` タグを
  要求する。新しいセッションは新しいチェーンを開始できる。

### D5: リンク不能環境では advisory に縮退する

`gh stack link` の実行(GitHub 上の stack オブジェクトへのリンク)が
機械的に不能と判定できる場合(`gh-stack` 拡張が未導入、または
`GET /repos/{owner}/{repo}/stacks` が機能撤収を示すエラーを返す場合)、
`G_stack` は block ではなく advisory(警告してそのまま通す)に降格する。
base チェーンの正しさ(D1・D3 の作成時強制)は `gh` 本体の CLI 引数検査
だけで完結するため、この縮退の影響を受けず全環境で維持される —
リンク不能環境でも orphan PR(base 宣言の不整合)自体は発生しない。

### D6: 実装言語は bash とする(ADR-0024 からの明示的逸脱)

新規 hook `stack-base-guard.sh` は bash で実装する。ADR-0024 は「対象
スコープ内の新規 hook は Rust を既定とする」と決定しているが、本 hook は
`attribution-guard.sh` が持つ実戦検証済みの判定エンジン(コマンド位置判定・
heredoc 分離・トークナイザ)を `source` して直接再利用する設計を取るため、
同一言語(bash)とする。このリポジトリには現時点で Rust の cargo workspace
や nix ビルド配線が存在せず、それらを本 ADR のスコープで同時に立ち上げると
判定エンジンの再実装・再検証という直交する大工事を抱え込むことになる。
`pr-gate.sh` / `attribution-guard.sh` を含む一括の Rust 移植(ADR-0024
Consequences が後続 Issue に切り出した本丸)が実施される際に、
`stack-base-guard.sh` もまとめて移行する。

## Alternatives considered

- **依存予測の継続**(現状維持): 今回発火した保留条項そのものであり、
  同種のインシデントの再発を防げない。棄却。
- **`gh stack init/add/submit/sync` のローカル追跡系コマンドの採用**:
  `config/claude/skills/stacked-pr/SKILL.md` §5 の既存裁定(force-push に
  よる履歴破壊バグを理由に不採用)をそのまま維持する。本 ADR では見直さない。
- **`G_stack` を fail-closed にする(判定不能時も block)**: `gh-stack`
  拡張が未導入の環境や `gh api` が一時的に失敗する状況でデッドロックする。
  D5 の advisory 縮退を採用し、判定不能とチェーン不整合を区別する。
- **stack-base-guard を Rust で新規実装する**: D6 で述べた理由(判定エンジン
  の再実装コストと、cargo workspace 未整備での二重の大工事)により棄却。

## Consequences

- `config/claude/skills/stacked-pr/SKILL.md`(§1 全面改稿、§3 の
  `gh stack link` を必須化)、`docs/claude/stacked-pr.md`(保留条項発火の
  記録)、`config/claude/CLAUDE.md`(「依存する変更は stacked PR に積む」→
  「セッション内の PR は単一チェーンに積む」)、
  `config/claude/skills/scope-inventory/SKILL.md` §5 を改訂する(後続段)。
- `config/claude/hooks/stack-base-guard.sh` を新設し、
  `home/modules/claude.nix` に配線する(後続段)。
- `config/claude/hooks/pr-gate.sh` に `G_stack` を追加し、`MAX_BLOCKS` を
  5 から 6 に引き上げる(後続段)。
- ADR-0024 の Rust 移行対象リストに `stack-base-guard.sh` を加える必要が
  ある(移行 Issue 未起票の場合は wrap-up inbox で起票する)。
- Context の private リポジトリの現行 open PR 群の修復は本 ADR のスコープ
  外(手動対応済み・別リポジトリ側の作業)。

## Verification

- `config/claude/hooks/stack-base-guard.sh --selftest` と
  `config/claude/hooks/pr-gate.sh --selftest` の拡張ケース(後続段で実装)。
- 本 ADR 自体は docs のみの変更のため `nix flake check` への影響はない
  (回帰確認として実行する)。
