# ADR-0010 — SOPS ランタイム復号チャネルの退役

- Status: Accepted
- Date: 2026-09-13
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

`config/zsh/modules/35-secrets-sops.zsh:274` は、インタラクティブな zsh
起動のたびに `load_sops_secrets` を無条件実行する。これは
`scripts/sops-secrets-env.sh` 経由で `~/.sops/.env` を `sops --decrypt`
する — 復号先は YubiKey 上の GPG `[E]`(暗号)サブ鍵で、カードが刺さって
いれば scdaemon がカード PIN を要求し、pinentry-gnome3 の GUI プロンプトが
出る。これはターミナルを開くたびに発火し、鬱陶しいという指摘があった。

この設計自体は ADR-0003 の意図した挙動である: sops-nix(activation 時復号)
を避け、PIN プロンプトを `home-manager switch` からインタラクティブシェル
起動側へ意図的に移した(ADR-0003 Decision/Consequences)。この設計判断
自体は誤りではないが、**復号対象のシークレットに消費者が残っているか**
は ADR-0003 制定時から時間が経ち再検証されていなかった。

`~/.sops/.env` の全キーを棚卸しした結果:

| キー | 消費者(調査時点) | 状態 |
|---|---|---|
| `GDRIVE_CLIENT_ID` / `GDRIVE_CLIENT_SECRET` | `~/.claude.json` の `gdrive` MCP サーバー(`@isaacphi/mcp-gdrive`) | claude.ai の Google Drive コネクタで代替可能 |
| `BRAVE_API_KEY` | `~/.claude.json` の `brave-search` MCP サーバー | Claude Code 組み込み WebSearch で代替可能 |
| `CONTEXT7_SECRET_KEY` | 見つからず(context7 は plugin 経由でキー無し運用が可能) | 消費者なし |
| `GITHUB_MCP_TOKEN` | 見つからず(`github` MCP は `gh mcp` = `gh auth` 済みの認証に移行済み) | 消費者なし |
| `RENOVATE_APP_ID` / `RENOVATE_APP_PRIVATE_KEY` | 見つからず(ユーザー確認: 使用終了) | 消費者なし |
| `FALCON_CID`(company-pop-new のみ) | `scripts/install-falcon-sensor.sh` が `sops-secrets-env` 経由で読む | 唯一の実消費者 — 別経路への移行が必要 |

`FALCON_CID` を除く全キーの消費者が既に消滅していた。`FALCON_CID` も、
インストーラーの実行頻度(導入・更新・復旧時のみ)を踏まえると、ホスト
ローカルに秘匿保存する必要はなく、実行時の対話入力で足りる。

つまり、この機構が現在「サポートしている」機能は実質ゼロで、シェル起動
ごとの PIN プロンプトはコストだけを払って便益を得ていない状態だった。

なお `sops` **バイナリ自体**は、このリポジトリ管理下にない複数の個人プロジェクトで
`direnv`(`.envrc`)経由のファイル単位暗号化(`sops --decrypt`)に現役で使われて
おり、退役対象の `~/.sops/.env` シェル起動時ローダーとは独立した依存である。
今回の全撤去は `home/modules/secrets.nix` が担っていた**ローダー配線**が対象で、
`sops` パッケージそのものの提供は止めない(Decision 参照)。

## Decision

1. **SOPS ランタイム復号チャネルを全撤去する。** `config/zsh/modules/
   35-secrets-sops.zsh`(自動ロード本体 + `reload_sops_secrets` /
   `sops_status` 手動コマンド)、`scripts/sops-secrets-env.sh`(wrapper)、
   `scripts/setup-sops-secrets.sh`(ホストローカル setup)、
   `home/modules/secrets.nix`(home-manager 配線)を削除する。**ただし
   `sops` パッケージ自体は `home/modules/packages.nix` の一般 CLI 群へ
   移し、`home.packages` からは外さない**(Context 参照 — 退役対象の
   ローダーとは独立に現役で使われているため)。
2. **シークレットは各ツール固有の認証チャネルに委ねる。** MCP-gdrive /
   brave-search の env 供給という間接経路をやめ、claude.ai の Google Drive
   コネクタと Claude Code 組み込み WebSearch という直接の代替に置き換える。
   `GITHUB_MCP_TOKEN` は既に `gh auth` ベースの `gh mcp` に置き換わって
   おり、削除は現状追認に過ぎない。
3. **`FALCON_CID` はインストーラー実行時の対話入力に切り替える。**
   `scripts/install-falcon-sensor.sh` は `FALCON_CID` が環境変数に無ければ
   `read -rs` でプロンプトする(エコーなし・シェル履歴に残らない)。
   ホストに秘密ファイルを置かない代わりに、導入・更新・復旧のたびに
   手入力が必要になる。
4. **ADR-0003 の鍵モデル・trust model は変更しない。** YubiKey に
   `[A]`/`[E]` を置き `[S]` を on-disk にする構成、二アイデンティティ、
   `.sops.yaml` がホストローカルという Amendment の内容は今回の対象外
   — `[E]` サブ鍵自体は残り、単に日常的な復号呼び出しが無くなるだけ。
   本 ADR は ADR-0003 の「secrets stay runtime-decrypted via SOPS in the
   interactive shell」という Decision 項目のみを **supersede** する。

## Alternatives considered

- **自動ロードだけ止め、手動コマンド(`reload_sops_secrets`)と復号基盤は
  温存する** — 将来別のシークレットが必要になったときの受け皿になるが、
  消費者ゼロの状態で機構(zsh 関数・wrapper・setup スクリプト・
  home-manager モジュール)を維持するコストの方が、必要になった時点で
  作り直すコストより高いと判断し、採らない。
- **`FALCON_CID` のためだけに company-pop-new に SOPS を残す** — 機構の
  大半を残すことになり、今回の目的(機構の全撤去)と矛盾するため採らない。
- **`FALCON_CID` をホストローカル平文ファイルに保存する** — at-rest の
  保護が現行(GPG 暗号化)より弱くなる上、実行頻度(導入・更新・復旧のみ)
  を考えると対話入力で十分であり、恒久保存の必要性が薄い。採らない。

## Consequences

- YubiKey `[E]` サブ鍵はこのリポジトリ管理下の消費者を失うが、上記の
  外部プロジェクトでの `direnv` 経由の利用は継続するため、`sops` パッケージ
  自体の home-manager 提供は止めない。鍵構成・ローテーション方針
  (ADR-0003 Amendment)自体は変更なし。
- 新しいシークレットが将来必要になった場合、この ADR の存在を踏まえた
  上で供給チャネルを都度選び直す(自動ロードを安易に復活させない)。
- `docs/nixification-roadmap.md` の「Secrets loader」行、
  `CONTEXT.md`/`README.md`/`SETUP.md` の SOPS 起動手順の記述は削除・改稿
  対象になる(実装は本 ADR のスタック上位段で行う)。
- ホスト側の後始末(`~/.sops/` の削除、`~/.claude.json` の
  `gdrive`/`brave-search` MCP 定義削除、各サービス側でのキー無効化)は
  home-manager の管轄外であり、実装段の PR 本文にチェックリストとして
  記載する(ADR 自体には時間で腐る手順を埋め込まない — ADR-0008)。

## Verification

- `nix flake check` — `home/modules/secrets.nix` の import 除去後も
  3 ホストとも green であること
- `hms .` 適用後、新しいターミナルで `sops-secrets-env` が PATH に無い
  こと、YubiKey を挿していても起動時に PIN プロンプトが出ないこと
