# ADR-0025 — 自作 pre-release CLI の導入をホストローカルレジストリで opt-in する

- Status: Accepted
- Date: 2026-09-21
- Issue: #208(design 起票)、grill-me セッションで裁定
- Amends: なし。ADR-0001(home-manager as source of truth)への scoped
  exception を新設する。

## Context

自作・タグ付きリリース未達の Rust CLI(`tarotene/telepath` が公開リポの
実例、private リポにも同型の存在がある)は、現状 `cd <repo> && cargo
install --path .` を都度手動実行して `~/.cargo/bin` に置くしかない。これは
#4 が扱う「野良インストールの回収漏れ」パターンの一種だが、`docs/
operations.md` のツール層判定フローが想定する二択(home-manager に足す /
自身のリリース機構を待つ)のどちらにも当てはまらない:

- home-manager は `flake.lock` を更新のたびに pin し直す必要があり、週に
  複数回 merge されタグ付きリリースを持たないリポには重い。
- リポ自身が最初の安定リリースを打つまで待つのは、開発中のツールを
  試したいという動機と両立しない。

最初に検討した設計は、このリポ自身に `(リポ名, crate ディレクトリ)` の
組を静的に列挙した `update-own-tools` スクリプトを置く案だった。これは
本リポの `publish-guard` 統合(ADR-0009)と正面から衝突する: あの guard は
company/org 名だけでなく **メンテナー自身の private リポ名**もアウトに
する設計であり、本リポ(public)のソース・Issue・PR に「この名前の private
リポが存在する」という情報を書くこと自体が、guard が防ごうとしている
漏洩の一種になる。

そこで #208 は「dotfiles 側はリポ名を一切知らない」汎用機構へ倒す
方向性(各リポが自分自身にマーカーファイルを置いて opt-in する)を提案
していた。grill-me セッションでこの前提そのものを検証し、次の指摘が
出た: 開発中の自作 OSS のリポジトリ内に
「dotfiles 側のこの仕組みを使っている」ことが分かるマーカーファイルが
コミットされていること自体が、OSS として不自然な露出になる。個別の
OSS・その利用者がこの仕組みの存在を意識しなくて済む形にしたい、という
要件が新たに確定した。

## Decision

1. **登録先はホストローカルの設定ファイル**にする
   (`~/.config/update-own-tools/registry.toml` 想定、`(repo url or path,
   crate サブディレクトリ, install コマンド)` を宣言)。**対象リポ自体には
   一切触れない** — マーカーファイルも、opt-in を示すいかなる痕跡も置か
   ない。登録は dotfiles が配る側のスクリプトとスキーマだけで完結する。
2. 先行例として `home/modules/github-audit.nix` が配る closed vocabulary
   の PRIVATE エントリ運用(`config/github-audit/*.tsv` は public 分のみ
   commit し、private 分は `*.local.tsv` としてホストの `~/.config/
   github-audit/` に置くだけで git 管理外、ADR-0020)と、ADR-0022 の
   ホストローカル GPG ファイルパターンをそのまま踏襲する。どちらも
   「dotfiles のソースツリーには秘匿対象の**存在**すら書かない」という
   同じ形。本 ADR はこのパターンを「私設リポジトリ名」という新しい種類の
   秘匿対象に適用する初めての例になる。
3. **on-demand 実行のみ**(systemd timer 化はしない、#208 の当初案を踏襲)。
   実行時に対象リポそれぞれについて `git worktree add --detach` で
   一時 worktree を作り、現在チェックアウト中のツリーを乱さずに
   `origin/main` から `cargo install --locked --path <dir> --target-dir
   <cache>` でビルドする。
4. **exit 条件**: 対象リポがタグ付きリリースのバイナリ配布を始めたら、
   ホストローカルレジストリからそのエントリを削除し、home-manager の
   `home.packages`(または nixpkgs 化)へ移行する。dotfiles 側のソース
   コードは一切変更不要 — 変更はレジストリという設定データだけで閉じる。
5. 実装(スクリプト本体・レジストリのスキーマ定義・detached worktree
   ビルドの具体手順)は #276 に分離する。

## Alternatives considered

- **マーカーファイル opt-in 方式(#208 の当初案)**: dotfiles 側がリポ名を
  知らずに済む点は満たすが、対象リポ側に「この仕組みを使っている」痕跡が
  残る。開発中の自作 OSS にとってこの露出自体が望ましくないと判明した
  ため棄却。
- **home-manager への早期組み込み**: `flake.lock` pin の更新頻度がリポの
  merge 頻度と合わず、Context で述べた通り不適合。
- **各リポ自身のリリース機構を待つ**: 開発中ツールを試す用途と矛盾する。
- **nix flake input として都度 follow**: `publish-guard`(ADR-0009)が
  gh の version-capped escape hatch で使っている手法だが、あちらは
  「stable が追いつくまでの一時しのぎ」であり pin の更新が要る点は
  home-manager と同じ問題を抱える。pre-release CLI の頻繁な更新には
  そぐわない。

## Consequences

- dotfiles のソースツリー・Issue・PR には、どの private リポがこの仕組み
  を使っているかの情報が一切現れない。publish-guard(ADR-0009)の脅威
  モデルと矛盾しない。
- レジストリファイルはホストごとに独立するため、複数ホストで同じツールを
  使うにはホストごとに登録が要る(home-manager 化するまでの一時的な
  コストとして許容する)。
- 実装(レジストリのスキーマ、スクリプト本体)は本 ADR のスコープ外。
  後続の実装 Issue でレジストリのファイル形式・エラーハンドリング・
  `--dry-run` 対応を詰める。

## Verification

- 本 ADR は docs のみの変更のため `nix flake check` への影響はない。
- #276(実装 Issue)側で、レジストリファイルが git 管理外であること
  (`.gitignore` 相当の確認は不要 — ホームディレクトリ配下でありリポジトリ
  外)と、対象リポのソースツリーに変更が生じないことを検証する。
