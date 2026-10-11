# herdr-protocol-check — herdr の protocol 不一致を SessionStart で先に伝える

herdr のクライアントとサーバの protocol が食い違うと、`herdr worktree create`
は `protocol_mismatch` で失敗する。直すにはサーバを再起動するが、再起動は
ペインのプロセスを終了させる副作用がある。エージェントが失敗の後で気付くと、
再起動の可否を本人に聞く前に作業が止まる。この hook(`crates/herdr-protocol-check`、
SessionStart、matcher `startup|resume`、timeout 10)は、セッション開始時に
状態を読み、食い違いがあれば先に伝える。

## 判定

`herdr status --json` を 3 秒の timeout で実行する(`hook_io::proc::output_with_timeout`)。
スキーマは実機(herdr 0.9.3、2026-10-11)で `herdr status --json` を実行して確かめた。
使うフィールドは次のとおり。

| フィールド | 意味 |
|---|---|
| `client.protocol` / `server.protocol` | 警告文に出す protocol 番号 |
| `server.running` | false なら無出力 |
| `server.compatible` | false なら警告 |
| `server.restart_needed` | true なら警告 |
| `server.server_binary_stale` | true なら警告 |

`server.running` が true で、残り 3 つのいずれかが該当するときだけ、
`additionalContext` に次の趣旨の文を出す: client protocol X / server protocol Y。
`herdr worktree create` は protocol_mismatch で失敗する。サーバの再起動は
ペインのプロセスを終了させる副作用がある。再起動の可否を本人に先に伝える。
出力形式は `worktree-fresh-base` と同じ `hookSpecificOutput`。

実機は互換の状態だったため、不一致のときの実出力は未採取。不一致・
`restart_needed` のケースは、実機のフィールド名に合わせた合成入力で検証した。

## fail-open

`herdr` が PATH に無い、起動に失敗する、timeout する、JSON が読めない、
フィールドが欠けている — いずれも無出力 exit 0。この hook はセッション開始を
止めない。サーバの再起動もしない(伝えるだけ)。

## 登録

`home/modules/claude.nix` の SessionStart 宣言リストと `home.file`
(`~/.claude/hooks/herdr-protocol-check`)に追加した。Codex には、同じ
バイナリを `registerCodexSessionStartHooks` で登録する(入力を読まず
agent 非依存のため、worktree-fresh-base と同じ扱い)。

## テスト

- `crates/herdr-protocol-check/src/lib.rs`: `decide` の単体(互換・不一致・
  `restart_needed`/stale・未起動/不正入力)。
- `crates/herdr-protocol-check/tests/cli.rs`: PATH 上の偽 `herdr` で
  互換 / 不一致 / `restart_needed` / herdr 不在 の 4 ケースを実バイナリ越しに検証。
