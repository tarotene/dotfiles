# ADR-0034 — 私的な machine-state 実値は、外側の private wrapper flake に置く

- Status: Accepted
- Date: 2026-09-23
- Issue: なし。grill-me セッションで裁定
- Amends: なし。ADR-0001(home-manager as source of truth)は破らない —
  宣言が正本であることは変えず、私的実値の宣言先を dotfiles の外に置く。

## Context

著者が持つ別の private リポジトリ(person-state hub)の README は既に
三正本モデルを宣言している — `dotfiles`(machine-state, **public**)/
person-state hub(person-state, private)/ さらに別の private リポジトリ
(account-state, private)。つまり「machine-state hub が無い」のではなく、
**machine-state 正本が public 単体で、その私的な半分を受けられない**の
が正確な問題定義である。

実態調査の結果:

- 現存する私的 machine-state は 19 行・1.6 KB・3 ファイルだけ
  (`~/.config/github-audit/{codename-registry.local.tsv,
  site-domains.local.tsv,overrides.tsv}`、ADR-0020)。バージョン管理も
  バックアップも無い。
- 計画済みで未存在のものが多数ある: `~/.config/external-send-guard/
  self.txt`、`~/.config/dotfiles/style-hub`、`~/.config/update-own-tools/
  registry.toml`(ADR-0025、#276)、obsidian-backup の設定一式、#359 の
  restic allowlist・実 B2 bucket 名・Healthchecks ping URL・backup identity
  UUID。将来像ではこれらが相当量になる。
- 懸念は実証済み: あるコミットがエージェント由来で著者の別の private
  リポジトリ名を public main のコメントに持ち込み、別のコミット
  (#245)がそれを除去した。ただし年表を検算すると、この漏洩は
  pre-push の publish-guard `scan-push` 統合(#196)の**3 分 46 秒前**に
  起きている — ゲートが素通ししたのではなく、まだ存在しなかった。
- 公開側は既に「規則は public・実値はプレースホルダ」を de facto で
  守っている(`docs/personal-cloud-projects.md` の `<tool>` /
  `<github-username>`、`docs/operations.md` の
  `s3:<B2 endpoint>/<bucket>/<prefix>`)。実値の行き先だけが無い。

## Decision

1. **私的な machine-state の器は、外側の private wrapper flake**にする。
   `inputs.dotfiles = github:tarotene/dotfiles;` を取り、この flake が
   `dotfiles.lib.mkHome <system> <hostModule> <extraModules>` を呼んで
   `homeConfigurations` を再定義する。私的実値はその flake 側の nix
   モジュール(`extraModules`)で宣言され、home-manager が配備する。
2. **依存の向きはこの1本のみ**: dotfiles は private リポジトリを flake
   input に取らない。取れば `flake.lock` がその input の `original` に
   `{owner, repo}` を平文で記録し、コミット済みファイルに private リポ名
   が載る — `docs/claude/writing-style.md` が明文で禁じている形と同じで、
   物理的に不可能である。よって依存方向を反転し、private wrapper flake
   が dotfiles を input に取る側にする。
3. dotfiles 側は `lib.mkHome` を export するだけで、private wrapper flake
   の**名前・パスを一切知らない**。所在の解決は `scripts/hms.sh` の
   `~/.config/dotfiles/private-hub` マーカー1個を介した間接参照とする —
   ADR-0019 の host マーカー、`docs/claude/writing-style.md` の style-hub
   マーカーと同じ型。home-manager では宣言しない(宣言すると絶対パスが
   ソースに写り間接参照の意味が消えるため、style-hub と同じ理由)。
4. `homeConfigurations` は dotfiles 側にも据え置く。`nix flake check` の
   4 ホスト build マトリクスが公開 CI の唯一の回帰検出であり、ADR-0004の
   「public OSS として成立する」憲章と `bootstrap.sh` の greenfield 経路
   もこれに依存しているため、削ってはならない。private wrapper flake は
   同名の `homeConfigurations` を自分の側で再定義する。
5. 境界述語(何が public に残り、何が private に行くか): **規則・
   スキーマ・導出手順は public、それを実例化した具体値は private**。
   ADR-0025 の「存在すら書かない」対象(pre-release ツール名等)は
   無条件でこの述語より優先する。既存の公開露出(`keys/*.pub`、
   `home/identities/company.nix` の業務メール、3 ホストの実ホスト名、
   `config/herdr/oshi-marks.tsv`)はADR-0007 の「遡及的な一括修正はしない」
   に従い grandfather し、移動しない。
6. 書き込み時の抑止は、private wrapper flake が
   `~/.config/publish-guard/repos.txt` を自分の宣言から生成する形に
   委ねる。`repos.txt` の `/` を含まない行は publish-guard の
   `PLAIN_PATTERNS` に入り部分文字列一致で deny される(既存の
   upstream 仕様)ため、実値を private wrapper flake に登録する行為
   そのものが、その値の machine-wide (push・Bash・MCP)な deny 登録になる。
   denylist の生成自体は private wrapper flake 側の責務であり、本 ADR は
   決定のみを記録する — publish-guard 本体の改修は不要。
   > **[本 ADR の Amendment(2026-09-24)により生成先を変更]** publish-guard は
   > bleep に改名され、設定ディレクトリは `~/.config/bleep/` に移った。生成先は
   > `~/.config/bleep/` とし、`orgs.txt` も同じ経路で生成する。
7. publish-guard の `PreToolUse` matcher(現在 `"Bash|mcp__.*"`)を
   `Edit|Write` へ拡張することは、今回は見送る。唯一確認できた漏洩は
   push 時ゲート統合の前に起きており、ゲート不在下の事例であって
   ゲートが素通ししたのではない。matcher 拡張を正当化する実測が無い。

## Alternatives considered

- **dotfiles が private リポを flake input に取る**: `flake.lock` に
  private リポ名が平文で残るため不可能(Decision 2 参照)。
- **peer deployer**(private リポが自分の `just apply` 相当で
  `~/.config/**` に直接書く、home-manager を経由しない): その継ぎ目で
  宣言性が切れ、ADR-0001 への scoped exception が必要になる。dotfiles の
  宣言的モデルと二重管理になるため棄却。
- **著者の person-state hub の責務を広げて machine-state も受ける**: その
  CONTRIBUTING.md は「machine-state は dotfiles の責務」を Rejected 例として
  明示しており、tag 駆動の CV リリースパイプラインと同居させる
  motivation が無い。
- **器を作らず #359 の restic backup で済ませる**: バージョン管理と
  復旧は解決するが、ホスト間同期・レビュー・エージェントの書き込み先
  ルーティングが得られない。

## Consequences

- dotfiles のソースツリー・Issue・PR には、private wrapper flake の
  名前・パスが一切現れない。publish-guard(ADR-0009)・ADR-0025 の脅威
  モデルと矛盾しない。
- `hms .`(pre-push 検証)は、private wrapper flake が登録されたホストでは
  public worktree 単体を適用するため、私的実値を含む検証にはならない
  (`scripts/hms.sh` にこの劣化を明記する)。private実値込みで worktree を
  検証したい場合は `home-manager switch --flake <private-hub-ref>#$(hostname)
  --override-input dotfiles path:$PWD` を直接叩く。
- private wrapper flake 自体の作成(リポジトリ、`values/` のスキーマ、
  `repos.txt` 生成の実装、その flake 自身の ADR-0001)は本 ADR のスコープ
  外。別リポジトリでの作業になるため dotfiles の stacked PR には積めない。
- 会社機(`company-pop-old` / `company-pop-new`)はマーカー未登録のまま
  現状動作を維持する(ADR-0022 の `esa.nix` が personal identity 層限定で
  import される先例と同じ形 — 会社の YubiKey は個人の master fingerprint
  を復号できない)。

## Verification

- `nix flake check` — `lib.mkHome` への `extraModules` 引数追加後も、
  4 ホストすべての `activationPackage` が従来どおり評価されること。
- `nix eval .#lib.mkHome --apply builtins.isFunction` — export が
  外部から引けること。
- `scripts/hms.sh --help` — マーカー不在では既定 ref
  (`github:tarotene/dotfiles`)を表示し、`~/.config/dotfiles/private-hub`
  にマーカーを置いた場合はその値を表示すること。
- `shellcheck -S error scripts/hms.sh` — CI の `ci.yml` と同条件。

## Amendment (2026-09-24 — publish-guard の改名に追従し、生成先を ~/.config/bleep/ に改める)

upstream の publish-guard は bleep に改名され(#439 で追従済み)、設定
ディレクトリが `~/.config/publish-guard/` から `~/.config/bleep/` に移った。
upstream は互換 shim を持たず、旧ディレクトリを読まない。Decision 6 が
指定した生成先 `~/.config/publish-guard/repos.txt` は、改名後の bleep には
届かない。

- Decision 6 の生成先を `~/.config/bleep/repos.txt` に読み替える。
- private wrapper flake は `~/.config/bleep/orgs.txt`(org 名)も同じ経路で
  生成する。bleep は `orgs.txt` が存在しないと publish 系の検査を ask に
  エスカレートする(upstream tarotene/bleep#30)。手置きをホスト移行の唯一の
  経路にすると、改名のような設定パスの変更で黙って効かなくなるため、宣言から
  生成する経路に寄せる。登録する org が無いホストは空ファイルを置く。
- private wrapper flake 側の生成実装は、Consequences にあるとおり別リポジトリ
  での作業で、本 ADR のスコープ外に留める。

背景: 改名の直後、移行未了のホストで Claude Code セッションが private
リポジトリ名を本文に含む Issue をこのリポジトリに起票した(削除済み)。
当時の bleep は `orgs.txt` 不在を無言で pass させており、上記の fail-loud 化は
その再発防止として upstream に入れたもの。

### 執行点

- `flake.nix` — `inputs.bleep` の pin を、`orgs.txt` 不在で ask を返す rev に上げる
