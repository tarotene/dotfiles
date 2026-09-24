# ADR-457 — Claude Code 本体は native installer を正本にする

- Status: Accepted
- Date: 2026-09-24
- Issue: No-Issue(grill-me セッションで裁定)
- Amends: ADR-0001 の「home-manager が user environment の source of truth
  である」という決定を*変更しない*。その決定への scoped exception を新設
  する(ADR-0025 と同型)。

## Context

`home/modules/packages.nix` は unfree パッケージとして nixpkgs の
`claude-code` を宣言していた一方、`docs/cutover-runbook.md` の
「Removing an ad-hoc native Claude Code install」節は「native install
(`~/.local/bin/claude`)を見つけたら消せ」と書いていた。ところが
`config/shell/profile` の PATH 順序コメントと `docs/operations.md`(#313 /
PR #317 の裁定)は「`.local/bin` の `claude` は意図的な shadow で、消して
はいけない」と正反対のことを言っていた。同じリポジトリの中で 2 つの文書が
矛盾したまま共存していた。

矛盾の発端は #313 だった。`scripts/claude-plan-model` は
`command -v claude` で解決した*インストール済みバイナリ*に焼き込まれた
`latest_per_family` カタログを読んで Opus Plan Mode の具体モデル ID を
引き直す。`~/.local/bin` が `~/.nix-profile/bin` より PATH 上で先に来る
(ADR-0029)ため、native installer の自己更新シムが常に勝つ。#313 は
この事実だけを受けて「native 側が正」と裁定し、`config/shell/profile` /
`docs/operations.md` にその理由を書いた(PR #317)。しかし当時
`packages.nix` の `claude-code` 宣言と `cutover-runbook.md` の「消せ」節は
反転させず、そのまま残した。

矛盾が実害を生んだのが今回のセッションである。実測:

```
23:08  hms(activation)実行 → claude-plan-model sync 発火(旧カタログで pin)
23:13  native claude が自己更新: 2.1.220 → 2.1.281
       (新カタログ: opus = claude-opus-5-5、旧: claude-opus-5)
23:17  ユーザーが手で claude-plan-model を叩くまで、opusplan の Plan 側は
       旧世代の Opus モデルのままだった
```

`claude-plan-model sync` の発火点は `hms` とトグル実行時の 2 つだけ
(`docs/claude/opusplan-model-aliases.md`)。native の自己更新は `hms` を
経由しないため、根本原因は「claude 本体の更新経路が sync の発火点と
一致していない」ことだった。加えて `~/.claude/settings.json` の
`env.DISABLE_AUTOUPDATER=1` は手書きで宣言のどこにも無く、次に同じホストを
`hms` した際に何が起きるか(nixpkgs の `claude-code` 2.1.223 — 58 パッチ
遅れ — が `~/.nix-profile/bin/claude` に現れても、PATH 順序により実害は
無いはずだが、宣言と実体が一致していない状態そのものが ADR-0029 の趣旨に
反する)が誰にも分かる形で書かれていなかった。

## Decision

1. **`claude` バイナリは全ホストで native installer
   (`curl -fsSL https://claude.ai/install.sh | bash`)を正本とする。**
   `home/modules/packages.nix` から nixpkgs の `claude-code` を削除する。
   nixpkgs pin は upstream リリースに数十パッチ遅れており(実測: 2.1.223
   vs 2.1.281)、「新モデルが出た当日に使う」という運用と構造的に噛み
   合わない。`claude-plan-model` が読む「インストール済みバイナリ」は
   1 つでなければならず、native と nix の二重管理は #313 が指摘した
   「宣言側が勝つと実害が起きる」パターンをそのまま再現する。
2. **自動更新は宣言で OFF に固定する。** `home.sessionVariables` に
   `DISABLE_AUTOUPDATER = "1"` を追加する(`home/modules/claude.nix`)。
   公式ドキュメントによれば、これは background check のみを止め、
   `claude update` 自体は動く。更新経路を「手動 `claude update` の 1 つ」
   に絞ることで、次の項目が確実にそれを捕まえられるようにする。
3. **更新直後に `claude-plan-model sync` を実行する。** 対話 zsh の関数
   `claude()`(`config/zsh/modules/53-tools-claude.zsh`、新規)が
   `update`/`upgrade` サブコマンドを横取りし、実行後に
   `claude-plan-model sync` を呼ぶ。`hms`・トグル実行時と並ぶ 3 つ目の
   発火点になる。sync は steady state で無言・mtime 不変(`apply_mode` の
   rc=2 経路)なので、up to date のときの見た目は変わらない。
4. **exit 条件**: Claude Code 自身が更新完了を検知できる hook を提供する
   ようになったら、zsh 関数での横取りをその hook に置き換える。nixpkgs の
   `claude-code` がリリース追従を数日以内にするようになったら、この ADR
   全体を見直す。

## Alternatives considered

- **`nixpkgs-unstable` overlay で追従する**(herdr の前例、ADR-0001
  Amendment #42)。更新が必ず `hms` を通るため sync の発火点を増やさずに
  済むが、unstable でも数日〜数週遅れており、「新モデルが出た日に使う」
  用途には合わない。herdr は `exec` されるだけで `dlopen` されないため
  overlay が安全という判断だったが、claude はそもそも上流が高頻度で
  自己更新するツールであり、overlay を挟んでも native の自己更新シムとの
  競合(#313 の実測どおり `.local/bin` が勝つ)は解消しない。
- **SessionStart hook で毎起動 sync する**。更新経路を問わずに追従できるが、
  settings.json の `env` は起動時に process.env へ焼き込まれるため、
  SessionStart で sync しても効くのは次のセッションから
  (`docs/claude/opusplan-model-aliases.md` が同じ理由で既に棄却済み)。
  毎起動でバイナリを読むコストに対して得るものが薄い。
- **両方(zsh 関数 + SessionStart)**: 冗長性は増すが、同じ仕事を 2 機構が
  担うことになり還元性に反する。

## Consequences

- `home/modules/packages.nix` から `claude-code` が消え、`nix flake check`
  の評価対象が 1 パッケージ減る。全ホストが native installer に一本化
  される(vega・arcturus・altair、いずれも既に native で稼働中と確認済み)。
- `docs/cutover-runbook.md` の該当節が「消す」から「入れる」に反転する。
  greenfield ホストのセットアップ手順に native installer の実行が
  明示的に加わる(従来は nix パッケージが暗黙に入れていた)。
- `claude update` を打つ習慣さえあれば、モデル pin の追従を忘れることが
  構造的に無くなる(対話シェルでは wrapper が必ず割り込むため)。非対話
  経路(cron 等)での更新は自動更新 OFF により起きないため、この ADR の
  射程外のまま安全。
- ADR-0029 の「`.local/bin` は nix より前」という PATH 順序自体には手を
  加えない。この ADR はその順序が生む shadow の意味(「意図的か drift か」)
  を明文化するだけである。

## 執行点

- `home/modules/packages.nix` — nixpkgs `claude-code` の宣言を削除
- `home/modules/claude.nix` — `home.sessionVariables.DISABLE_AUTOUPDATER`
- `config/zsh/modules/53-tools-claude.zsh` — 新規。`claude update` 直後に
  `claude-plan-model sync` を実行する関数 `claude()`
- `docs/cutover-runbook.md` — 「Removing an ad-hoc native Claude Code
  install」を「Installing Claude Code (native installer)」に反転
