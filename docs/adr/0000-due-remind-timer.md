# ADR-0000 — a generic reader for another repository's local due-index, on an hourly Herdr-notified timer

- Status: Accepted
- Date: 2026-09-28
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

## Context

別の private リポジトリ(社用の個人サンドボックス)は複数の SOPS-encrypted
record が持つ期限(出張報告書・旅費精算など)を、`${XDG_STATE_HOME:-~/.local/
state}/claude/<domain>/<repo-slug>/due.jsonl` という平文 index に一方向で射影
する契約を持つ(その repo 自身の `docs/adr/333-due-index-contract.md`)。この
index は Claude Code の SessionStart hook から読まれるが、セッションを開かない
日には誰も気づけない。実際に出張報告書の提出期限を本人が見落としていた
(2026-09-28、grill-me セッション)。

この dotfiles リポジトリは、Claude Code の外側で動く恒常的なリマインダー経路
(systemd/launchd timer + Herdr notification)を既に持っている
(`gpg-subkey-remind`、`git-audit-worktrees` — `home/modules/gpg.nix`,
`home/modules/worktree.nix`)。件の index はどの domain の record かに関わらず
`slug`/`due` の2キーしか持たないので、この dotfiles 側は行の形だけを知る汎用の
reader として実装でき、あちら側に新しい domain が増えてもこのリポジトリ側は
変更不要になる。

## Decision

### D1: reader は行の二キー契約だけを知り、どの domain のものかは一切知らない

`crates/due-remind` は `${XDG_STATE_HOME:-~/.local/state}/claude/*/*/due.jsonl` を
glob するだけで、`slug`・`due`・(推奨)`id` 以外のキーを一切解釈しない。会社名・
出張先・組織名などのドメイン語彙はこのリポジトリのソースに一切現れない
(ADR-0034 — private な識別子をこのリポジトリに書けない、という制約とも両立する)。

### D2: 実装は Rust(ADR-0024 の既定)

新規の hook/CLI は `docs/adr/0024-hook-cli-scripts-target-rust.md` 以降 Rust を
既定とする。本命は「既定に従う」で、対抗馬は bash(`gpg-subkey`/
`git-audit-worktrees` と同じ形——`HERDR_BIN` 差し替え・`--selftest`・状態ファイル
書き込み)だったが、これは `rust-migration.toml` への `[[target]]` 追記と
`max_remaining` 加算という ADR-0024 への例外を1件積む必要があり、その理由が
「書くのが楽」以外に無かった。`crates/herdr-issue-counts`(timer から herdr を叩く
同型の既存クレート)をそのまま型にした — `lib.rs` に純粋関数(パース・日付計算・
文字列組み立て)、`main.rs` にファイル探索・プロセス起動・exit code。日付計算は
`YYYY-MM-DD` の civil-day への変換だけなので、`chrono`/`time` を新規依存に加えず
Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms"
<http://howardhinnant.github.io/date_algorithms.html>(取得 2026-09-28)の
`days_from_civil` を実装した(この用途に日付ライブラリ1つを増やす理由がない)。

### D3: 時間毎再試行 + 日次 state で「1日1回、必ず見る」を作る

`gpg-subkey-remind` は daily 00:00 発火で、herdr が前面に無いときの
`shown:false` を握り潰す設計 — GPG 鍵の失効警告なら翌日また鳴るので実害が薄いが、
提出期限の見落としは実際に起きた実害である。`09..19:00:00` の時間毎 timer と、
`shown:true` を得た日だけ更新する `last-shown` state ファイルで、「オフィス時間に
1回でも herdr を見れば気づける」を作る。`shown:true` 以外(前面クライアント無し・
busy・rate_limited・herdr 未起動)は state を更新せず次の時刻に再試行し、
`reason:"disabled"`(config.toml で通知配信自体を切っている)だけを
`systemctl --user --failed` に出す恒久的な失敗として扱う
(`crates/herdr-issue-counts` の exit-code 方針、#442、を継承)。

### D4: 通知経路は Herdr のみ(desktop 通知は追加しない)

対抗馬として notify-send(COSMIC のデスクトップ通知、dotfiles issue #462 で
libnotify が profile に入り既に動作可能)を検討したが、`docs/operations.md` の
既定方針(「通知は herdr 経由、デスクトップ通知ツールは宣言しない」)を崩す
理由が無く、2経路の state 管理が増えるだけだったので採らなかった。

## Alternatives considered

- bash 実装(ADR-0024 への例外) — D2 で不採用。
- notify-send へのフォールバック — D4 で不採用。
- Google Calendar への終日イベント射影 — 感触で外した(record の期限を Calendar に
  射影する経路はあちら側の `/trip` gate ③ の拡張になり、精算済みで消す運用が
  増えそうだという感触。分析はしていない)。

## Consequences

- あちら側の `docs/adr/333-due-index-contract.md` が変われば、このリポジトリの
  `crates/due-remind` も追従が必要になる — 契約の正本は向こう側に1つだけ置き、
  ここでは複写しない。
- `crates/due-remind` は Cargo workspace の `members = ["crates/*"]` に自動で
  含まれ、`pkgs.dotfiles-tools` の一部として配備される。新しい bin を追加した
  ことによる `nix.yml` の重いジョブへの影響は他のクレート追加と同型
  (ADR-468 が既に規定する skip 条件がそのまま適用される)。
- `date +%F` を外部コマンドとして呼ぶため、`due-remind.service`/`launchd.agents.
  due-remind` の PATH には `coreutils` を渡す — herdr のみを渡していた設計から
  1つ増える。

## 執行点

- `crates/due-remind/` — D1・D2・D3 の執行点(reader 本体・純粋ロジックと
  `#[test]`)。
- `home/modules/due-remind.nix` — D3・D4 の執行点(hourly timer・launchd 双子・
  Herdr 経由の配線)。
- `home/common.nix` — 上記モジュールの import 登録。

## Verification

- `cargo test -p due-remind`(23 テスト: `lib.rs` の単体テスト17件 + `tests/cli.rs`
  の結合テスト6件 — shown / 同日2回目スキップ / no_foreground_client 等の一過性
  理由 / disabled / 該当0件 / claude ディレクトリ不在、の6分岐)。
- `cargo clippy -p due-remind --all-targets -- --deny warnings`。
- `cargo fmt -p due-remind -- --check`。
- `nix flake check --all-systems --no-build`(全 host の `homeConfigurations`
  評価、`checks.*.dotfiles-tools`/`dotfiles-tools-clippy`/`dotfiles-tools-fmt`
  が実際に `cargo test`/`clippy`/`fmt` をサンドボックス内で実行して合格)。
