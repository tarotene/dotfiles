# Codex の調査用ネットワーク例外

`home/modules/codex.nix` の `dotfiles.codexPermissions.domains` は、共有の宛先に
`raw.githubusercontent.com` と `api.business.githubcopilot.com` を加える。固定した版の upstream ソースを、Plan モードの
調査でも取得できるようにするためである。許可するのは通信先であり、書き込みや公開の
許可ではない。秘密パスの拒否、資格情報の masking、通信 proxy は維持する。

OpenAI, *Codex*, `rust-v0.164.0` の
[`NetworkPermissionsToml`](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/permissions_toml.rs#L329-L342)
が profile ごとの `network.domains` を定義する。公開日未確認、取得日 2026-10-27。
この宣言は既存の `agent-settings-reconcile codex-permissions` で配備する。

配備後は新しいセッションで次を実行し、ファイルの先頭が Rust のソースとして読めることを
確認する。設定の読み戻しだけで、通信できたとは扱わない。

```sh
curl -fsSL https://raw.githubusercontent.com/openai/codex/rust-v0.164.0/codex-rs/config/src/permissions_toml.rs -o /tmp/codex-permissions-source.rs
sed -n '1,10p' /tmp/codex-permissions-source.rs
```

この確認は調査用の読み取りであり、Plan モードの実装禁止とは独立している。

## herdr の Unix socket

同じ profile の `network.dangerously_allow_all_unix_sockets` は false とする。
herdr や署名付き Git 操作で socket が必要なときは native の自動審査に渡す。
適用後の新しいセッションでの動作は未確認である。

#1064(Codex の sandbox 内でローカル GPG 署名できる経路)は、この判断で決定済みとして閉じる。
socket のパスだけを許可しても拒否が続く(下の `rust-v0.164.0` のソースとテスト)ので、
gpg-agent の socket だけを開く形は採らず、署名は native の自動審査に渡す。
`rust-v0.165.0` でも同じかは未確認。自動審査を通して署名付きコミットが作れることの実測は、
まだ行っていない(#1159 の実測と同じ Codex の使い捨てセッションで行う)。
署名が通らないときの別経路は、GitHub のコミット API(`config/agents/AGENTS.md` の該当節)。

OpenAI, *Codex*, `rust-v0.164.0` の
[`seccomp.rs` L254–269](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/linux-sandbox/src/seccomp.rs#L254-L269)
では、この boolean が真の場合だけ proxy 経由の Linux sandbox 内で standalone な
`socket(AF_UNIX, ...)` を許可する。同版の
[`managed_proxy_unix_sockets_tests.rs` L127–141](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/linux-sandbox/tests/suite/managed_proxy_unix_sockets_tests.rs#L127-L141)
は、socket のパスだけを許可した場合には拒否を維持することを試験している。
公開日未確認、取得日 2026-10-27。効かないパス単位の許可を重ねない。

配備後は新しいセッションで、対象リポジトリの中から次を実行する。作業場所は
`git rev-parse` で決め、既存のディレクトリと衝突しない一時パスに worktree を作る。

```sh
herdr status server
probe_repo="$(git rev-parse --show-toplevel)"
probe_parent="$(mktemp -d /tmp/codex-herdr-check.XXXXXX)"
herdr worktree create --cwd "$probe_repo" --branch chore/codex-permission-probe \
  --path "$probe_parent/worktree" --no-focus
git -C "$probe_parent/worktree" rev-parse --git-dir --git-common-dir
```

最後のコマンドの2行が異なることを確かめて完了とする。検証用ブランチがすでに
存在する場合は、既存 worktree を確認してその結果を使い、既存ブランチを削除しない。
現在のセッションが持つ sandbox の制約は、設定の変更だけでは差し替わらない。
未配備や sandbox の入れ子で確認できない場合は未確認と記録する。
