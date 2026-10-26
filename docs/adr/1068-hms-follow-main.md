# ADR-0000 — main の更新に沿って hms を自動で当てる。ただし hms が main から作った世代だけに追従する

- Status: Accepted
- Date: 2026-10-27
- Issue: No-Issue(セッション中に本人から出た依頼「リポジトリの更新に沿ってそのまま hms 打つのをフックしたい」。関連: #1025 並行適用で世代が戻る問題、#1060 許可ポリシーの拡大を配備すると全ツールが止まる問題)

## Context

main が進むたびに `hms` を手で打っている。自動で当てる仕組みは無く、いま当たっている世代が main より遅れているかを比べる処理も無い(origin/main 96204fa の時点で、`pipeline-map.toml` の `hms` は `auto = false`)。

自動で当てるには、手で打つときには本人が暗黙に守っていた条件を、機構が守る必要がある。

- 検証用の世代を上書きしない。`hms .` や `hms github:…/<branch>` で当てた世代は、push 前の検証のためにわざと main と違う。自動で main に戻すと、検証が黙って消える。
- 同時に当てない。別の worktree や別のセッションから `hms` を同時に当てると、後の switch が先の世代を戻した(#1025、2026-10-26 に実際に発生)。自動で当てる経路が増えると、重なる機会が増える。
- native 権限設定の変更を、本人の確認より先に自動配備しない。Claude と Codex の設定差分を確認してから、本人が手動で `hms` を実行する。共有ポリシーの reviewed migrate は前提にしない(ADR-457 の 2026-10-27 Amendment、`docs/agents/agent-permissions.md`)。
- 失敗を黙らせない。timer で動く機構の失敗は、journal を見るまで誰も気づかない。

本人が承知している制約が 3 つある。herdr 本体の更新は、今と同じく手動での再起動が要る(`hms` は警告するだけ)。Claude Code の hook は、作業中のセッションの下で入れ替わる。linger は `assertNoLinger` が禁止しているので、ログイン中にしか動かない。

## Decision

### D1: 自動適用は `hms` 自身に `--follow` を足して行い、素の `home-manager switch` は回さない

`hms --follow` は、適用の本体(`apply`)を引数なしの `hms` と共有する。wrapper の解決、wrapper の lock の上書き、bleep の事前ビルド、private-hub の降格の拒否(#567)、fcitx5 の再起動の検証が、そのまま通る。home-manager 本体の `services.home-manager.autoUpgrade` は、flakeDir で `home-manager switch --flake .` を回すだけで、これらを素通りする。検証用の世代を見分けず、Linux 限定の assertion も持つ。

### D2: どの経路が世代を作ったかは、適用した時点で記録する

switch が成功した直後に `~/.local/state/hms/last-apply.json`(`mode`・`rev`・`generation`・`applied_at`)を書く。`mode` は、引数なしの `hms` なら `default`、ref を渡したか `--public-only` を付けたなら `explicit`。ローカルパスかどうかでは決めない。記録は後処理(Tailscale・daemon-reload・fcitx5)の前に書く。後処理が失敗しても世代は適用済みで、記録が無いと hms が作った世代を「hms 以外が切り替えた」と誤認するため。

追従するのは、記録の世代がいまの profile と一致し、かつ `mode = default` のときだけ。rev から推測しない。squash-merge では、ブランチのコミットが main に無いことからしか検証用と分からない。

### D3: 適用は同時に 1 つだけ。hms 全体で 1 つのファイルロックを取る

`~/.local/state/hms/lock` を `File::lock`(flock)で取る。手動の `hms` は、先の適用が終わるまで待つ。`--follow` は待たず、取れなければ見送る(timer が詰まらない)。ロックを取れないときは、ロックなしで進まずエラーで止まる。ロックの fd は子(nix・home-manager)に継承されない。

置き場は状態ディレクトリの 1 本にした。darwin には `XDG_RUNTIME_DIR` が無く、Linux と分岐が増える。#1025 の完了条件(競合の検出と、利用者への提示)は、この決定の範囲外で、#1025 に残す。

### D4: 止めた理由と失敗は、herdr の通知で 1 回だけ知らせる

通知は herdr(`herdr notification show`)で出す。`notify-send` ではない(`home/modules/due-remind.nix` の理由)。同じ原因では 1 回だけ知らせる。原因が変われば(世代が変わる・main が進む)別のキーになり、改めて知らせる。表示されたと分かったときだけ「知らせた」と覚える。herdr にクライアントが居ない時刻の通知は捨てられるので、表示を確かめずに覚えると、本人が一度も見ないまま黙る。

失敗した main のコミットは、main が次に進むまで再試行しない。unit は、適用しなかったときも失敗したときも終了コード 0 で終える(失敗した unit が居座らないように — `home/modules/drift.nix` の教訓)。非 0 は、ロックを取れないなど環境の異常だけ。

### D5: Linux は systemd の user timer、darwin は launchd agent。unit / plist の中身は世代によらず同じにする

10 分おきに `hms --follow` を呼ぶ。`X-SwitchMethod = "keep-old"` で、switch が実行中の unit を再起動して追従を打ち切るのを防ぐ。launchd は plist が変わると agent を bootout するので、PATH に store パスを埋めず、固定のパスだけにする(home-manager の `modules/launchd` は plist が同一なら bootout しない)。`Persistent=` は `OnCalendar=` の timer にだけ効くので、書かない。

### D6: 両 CLI の native 権限設定が完全に読め、配備済みのものと一致するときだけ自動で当てる

switch の直前に、新しい世代を `nix build` で組む。`home-files/.config/agent-native/claude-permissions.json` と `home-files/.config/agent-native/codex-permissions.json` を、それぞれ配備済みの設定ディレクトリ(`$XDG_CONFIG_HOME`、未指定なら `~/.config`)の `agent-native/` 内の同名ファイルと全バイト比較する。両方のファイルが新世代と配備先の両側に存在し、読み取れ、一致するときだけ適用する。差分・欠如・読み取りエラーがあれば適用せず、本人へ通知し、native 設定の差分確認と手動の `hms` を案内する。両側とも無い場合も一致とはしない。拡大か縮小かは見分けず、権限の意味や所有判定を `hms` に再実装しない。共有ポリシーの移行は要求しない。

## Alternatives considered

- **home-manager の `services.home-manager.autoUpgrade`**: D1 のとおり。正規の適用手順を通らず、検証用の世代を守れない。読んだ版は pin 済みの 24f8f8e。
- **comin(git を見張って適用する GitOps の道具)**: 感触で外した。NixOS のシステム層向けと記憶しているが、このセッションでは読んでいない。分析ではない。
- **rev の祖先関係から、検証用の世代かを推測する**: 採らない。squash-merge では main に無いコミットは「検証用」と「マージ前のブランチ」を区別できず、リモートのブランチ ref も取り違える。
- **timer を使わず、git の `post-merge` hook や `git-checkout-sync` の成功に連動して当てる**: `git-checkout-sync` は親 checkout の fast-forward だけを担い、`hms`(引数なし)は checkout を読まず GitHub の main を直接当てるので、checkout の更新を引き金にする意味が無い。感触で外した部分はなく、読んだ実装(`home/modules/worktree.nix`、`crates/hms`)から決めた。
- **自動適用せず、遅れを通知するだけにする**: 本人は選ばなかった(AskUserQuestion で「安全なときだけ自動適用し、それ以外は通知する」を選択)。

## Consequences

- main が進んだら、10 分以内に自動で追従する。ログイン中の間だけ(linger は禁止)。
- `hms .` で検証用の世代を当てている間は、追従が止まり、1 回だけ通知される。戻すには引数なしの `hms`。
- 新しい `hms` を入れる適用そのものは、古い `hms` が行うので記録が無い。そのため、配備後の最初の追従は「一度 `hms` を当ててください」の通知になる(main に追いついていれば、通知もしない)。
- Claude または Codex の native 権限設定に差分・欠如・読み取りエラーがある場合、自動では当てず通知する。本人が設定を確認して手動で `hms` を実行する。
- 自動で当てる経路が増えても、同時に 1 つしか走らない。手動の `hms` は待つ。
- herdr 本体の更新の再起動は、今と同じく手動(`hms` が警告するだけ)。
- timer は `hms-follow`。`dotfiles.hmsFollow.enable = false` か、`systemctl --user stop hms-follow.timer` で止める。

## 先行例

- nix-community/home-manager, `modules/services/home-manager-auto-upgrade.nix`(pin 済みの 24f8f8e、取得 2026-10-26)。oneshot service と user timer の形、`X-SwitchMethod = "keep-old"` は踏襲した。適用本体は hms を通し、適用しない条件を足した点が異なる。
- nix-community/home-manager, `modules/launchd/default.nix`(同上)。plist が同一なら bootout しない挙動が D5 の根拠。
- このリポジトリの `home/modules/worktree.nix` の `git-checkout-sync`(timer と launchd の同居)と `home/modules/drift.nix`(unit を失敗で居座らせない)。
- このリポジトリの `crates/gpg-subkey/src/lib.rs` と `crates/due-remind`(herdr の通知、表示されたかを確かめる)。
- このリポジトリの ADR-457 の 2026-10-27 Amendment と `docs/agents/agent-permissions.md`(共有所有管理を撤去し、両 CLI の native 設定を独立して宣言する。取得 2026-10-27)。D6 は両 spec 全体のバイト一致を自動適用の条件とし、差分の意味は本人の確認に残す。

## 執行点

- `crates/hms/src/lib.rs`
- `home/modules/hms-follow.nix`
- `home/common.nix`
- `pipeline-map.toml`

## Verification

- `cargo test -p hms`: `follow_decision` の全分岐、ロックの直列化、記録の書き込み、通知の重複抑止、許可ポリシーの差で止まること、失敗したコミットを再試行しないこと。
- 変異で、テストが落ちることを確かめた: ロックを常に取れたことにする、許可ポリシーの確認を外す、検証用の世代に追従する、通知の重複抑止を外す、失敗したコミットを覚えない、記録と別の世代を検出しない。
- `cargo test -p pipeline-map` と、図の再生成(`docs/assets/pipeline-map/`)。
- 実機での確認(ロックの待ち、macOS の flock、timer の起動、許可ポリシーの変更で止まること)は、本人の端末でしか確かめられないので、#1066 に払い出した。
