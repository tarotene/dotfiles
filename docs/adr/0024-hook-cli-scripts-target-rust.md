# ADR-0024 — hook / CLI スクリプト群の実装技術を Rust とする

- Status: Accepted
- Date: 2026-09-21
- Issue: No-Issue(ユーザー依頼の技術調査 — grill-me セッション中に確定)
- 調査記録: [`docs/shell-successor-research.md`](../shell-successor-research.md)
  (一次情報の URL・取得日・PoC 実測値はすべてこちらにあり、本 ADR には
  埋め込まない — ADR-0008 の分離規約)

## Context

このリポジトリは home-manager (nix) を決定論的な source of truth とする
一方、実際に動く道具は 87 本・約 17,900 行のシェルスクリプトで、外部コマンド
依存(jq・gh・git)が nix の closure に固定されていない。加えて上位 4 本
(pr-gate.sh 1634 行、copilot-plan-review.sh 1588 行、github-audit 1511 行、
gpg-subkey 1120 行)は bash の表現力の限界に達しつつある。

グリルセッションで、痛みの優先順位は (b) 決定論との不整合 > (a) 堅牢性 >
(c)/(d) 保守コスト・表現力、非機能制約は次の 4 点と確定した:

1. nix flake でビルドでき、依存が closure に固定されること(must)
2. hook 起動が 50ms 未満であること(must)
3. 単一言語に寄せること(should)
4. 既存の `--selftest` を通常のユニットテストへ移行できること(must)

調査対象スコープ(想定ワークロード)は Claude Code hooks + statusline
(18 本 / 8,918 行)と `~/.local/bin` デプロイの CLI 群(19 本 / 5,770 行)ほか
activation スクリプト・codex/copilot hooks を合わせた約 40 本 / 15,000 行。
escape-hatch(bootstrap.sh 等)、他リポジトリへ配布する skills の
templates/scripts、git hooks、zsh モジュールは対象外(ADR-0007 の性質分類、
可搬性維持のため)。

深掘り 3 候補(ベースライン: bash 続投 + writeShellApplication、Rust、
Deno + TypeScript)を、代表 hook(`git-stash-guard.sh`, 320 行)の実移植 +
実測で比較した。浅掘り 3 候補(Go・Babashka・Nushell)は一次情報 1 点以上を
引いて不採用と判定済み(詳細は調査記録)。

## Decision

**対象スコープ(約 40 本 / 15,000 行)の新規実装・書き換え先を Rust とする。**
nix ビルドは `pkgs.rustPlatform.buildRustPackage` または `crane`
(多数の小バイナリが依存を共有する構成では crane の依存キャッシュ共有が
有利と考えられる — 調査記録参照)を用い、stdin/stdout の JSON 契約は
`serde`/`serde_json` で型化する。`--selftest` は `cargo test` の
`#[test]` 関数群へ 1:1 で移す。

**Deno + TypeScript は評価した上で不採用**とする。主因は制約 1(must:
closure 固定)を満たす成熟した nix 統合手法が無いこと ——
外部 import を closure にロックするコミュニティツール deno2nix は
2024-06-07 にアーカイブ済みで後継が無い。副因として、起動レイテンシは
50ms 未満を満たすが Rust に対し 20〜25 倍遅く(mean 1.2ms vs 23〜30ms)、
`deno compile` は hook 1 本あたり 89MB の自己完結バイナリを生成するため
40 本に適用すると真の限界コストが積み上がる(詳細: 調査記録 §6.3)。

**ベースライン(bash 続投 + writeShellApplication)も不採用**とする。
`runtimeInputs` による closure 固定自体は成熟しているが、(a) 保守性・
表現力(型なし、jq 依存のまま)が主因である d) の解消にならない、
(b) 実測で「既存スクリプトをそのまま載せてもゼロコストではない」
(SC2016 info レベルの build 失敗を実際に踏んだ)ことを確認した。

## Alternatives considered

- **Deno + TypeScript**: 上記の通り、closure 固定手法の未成熟(must 制約
  未達)により不採用。ADR-0002 の「literal ファイルをそのまま配る」思想とは
  親和的だったが、nix 面の欠落がそれを上回った。deno2nix が再メンテナンス
  されるか代替が確立されれば再検討の余地はあるが、現時点では推奨しない。
- **bash 続投 + writeShellApplication (+resholve)**: closure 固定という
  制約 1 単体は解けるが、動機の主眼だった保守性・表現力(c/d)を解決しない。
  87 本全部をこの経路に載せる作業自体が無視できないコスト(shellcheck 適合の
  個別調整)であることも実測で確認済み。
- **Go**: closure 固定・起動速度とも良好だが、このリポジトリ・エコシステムに
  Go の既存資産が無く、Rust に対する差別化が薄い(浅掘り、調査記録参照)。
- **Babashka**: 起動速度(公称 0.026 秒)は魅力的だが、Clojure の習熟コストが
  見合わない(浅掘り)。
- **Nushell**: 設計の重心が対話的シェルにあり、hook のような単発起動用途とは
  ミスマッチ(浅掘り)。

## Consequences

- **一括移行はしない。** 15,000 行を一度に書き換える計画は本 ADR のスコープ
  外で、後続 Issue に段階的な移行計画を切り出す。優先順位は「jq 密度 ×
  行数」が高いもの(pr-gate.sh、copilot-plan-review.sh、github-audit、
  gpg-subkey、stdin JSON を読む hook 群)から。
- 新規に書く hook / CLI(対象スコープ内)は、この ADR 以降 Rust を既定とする。
  既存スクリプトは移行 Issue が個別に消化するまで bash のまま残る
  (ADR-0007 の「既存ファイルの一括リネームはしない」と同じ漸進方針)。
- escape-hatch・zsh モジュール・git hooks・skills の配布物は対象外のまま
  シェルで残る(スコープ判断は変えない)。
- `rust-repo-governance` スキルが持つ CI/release-plz/Renovate 資産は、
  dotfiles 自体が Rust コードを持つようになった場合の適用候補になるが、
  適用要否は移行 Issue 側で個別判断する(このリポジトリ自体を
  `rust-repo-governance` の対象にするかは未決定、本 ADR は決めない)。
- 20〜40 個の小バイナリを 1 cargo workspace にまとめる場合の具体的な
  ディレクトリ構成(`src/bin/` 複数 vs workspace メンバー分割)は、移行
  Issue の設計時に決める(本 ADR は言語選定のみを決定する)。

## Verification

- 調査記録([`docs/shell-successor-research.md`](../shell-successor-research.md))
  に記載の PoC(`git-stash-guard.sh` → Rust / Deno の実移植)で、Rust 版が
  bash 版と出力完全一致・起動 1.2ms(p95 1.7ms、50ms 予算の 1/30 以下)・
  `cargo test` 9 関数全 pass を実測済み。
- `nix flake check` は本 ADR・調査記録の追加(docs のみ)による影響を受けない
  ため、回帰がないことの確認として実行する。
