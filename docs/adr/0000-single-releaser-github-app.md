# ADR-0000 — releaser GitHub App を 1 個に集約し、宣言を workflow ファイルの存在に還元する

- Status: Accepted
- Date: 2026-09-24
- Issue: #432(tracking)、#433。grill-me セッションで裁定。

## Context

`tarotene/publish-guard#26` の `## 要確認` が「`settings/apps/new` で
GitHub App を新規作成せよ」と指示していた。同じ手順を `tarotene/telepath`
でも既に実行済みで(repo secrets に `RELEASE_PLZ_APP_ID` /
`RELEASE_PLZ_APP_PRIVATE_KEY`、2026-05-30 登録)、grill-me セッション時点で
個人アカウントには releaser 用途の GitHub App が 4 個存在していた。

原因は `config/claude/skills/rust-repo-governance/reference/
manual-steps.md` の手順そのもの: App 名テンプレートを `<REPO>-release-plz`
と決め打ちし、"Install the app on your repository only" と明示していた。
放置すれば rust/astro 系リポジトリを seed するたびに App が 1 個増える。

セッション冒頭の思いつきは「GitHub App Personal Registry のようなものを
宣言的に持つ」だったが、調査の結果、「乱立」は 3 つの別物が混ざっていた:

1. **App 登録の個数** — 1 個を使い回せば消える。新しい仕組みは不要で、
   skill の手順を覆すだけで足りる。
2. **install 先の集合** — GitHub 側の install 状態が唯一の真実。dotfiles
   側に写しを持つ必要はない。
3. **secret のコピー** — 個人アカウントには account-level の Actions
   secret が存在しない(repo/environment/org のみで、org secret も Free
   プランでは private repo から読めない)。App を 1 個にしても
   `APP_ID`/`PRIVATE_KEY` は repo ごとにコピーが要り、ここだけが
   **還元不可能**。

したがって成果物は「repo を列挙する registry ファイル」ではなく、(a) 1
App 方針の決定と手順書の書き換え、(b) 3 のコピー漏れを検出する
`github-audit` の新ドメイン、の 2 つになる。

## Decision

### D1: releaser GitHub App を repo ごとでなく 1 個に集約する

bot 人格 = 権限セット単位で App を持ち、対象リポジトリすべてにその 1 個を
install する。App 名はツール・repo に依存しない汎用名にする。

出典: GitHub, "Best practices for creating a GitHub App"
<https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/best-practices-for-creating-a-github-app>
(取得 2026-09-24) — 個数の指針は無く「最小権限」「トークンの repo 絞り込
み」「必要以上の鍵を作るな」のみ。GitHub, "actions/create-github-app-token
README" <https://github.com/actions/create-github-app-token>(取得
2026-09-24)—"If `owner` and `repositories` are empty, access will be
scoped to only the current repository."。つまり実行時の blast radius は
1 App でも per-repo App でも同一で、per-repo App はコストだけが線形に
増える。実運用先行例は Renovate / Dependabot /
googleapis/repo-automation-bots(いずれも 1 App × 多 repo)。

対抗馬として repo ごとの App 維持を検討したが、上記の理由で採らない。
用途別に 2 個以上に分ける案も、現時点で releaser 以外の用途が無いため
YAGNI として外した。

### D2: 宣言の正本を「release workflow ファイルの存在」に置き、対象 repo を列挙するファイルは作らない

`.github/workflows/release-plz.yml` または `release-please.yml` が存在
する repo が「releaser App の対象」であり、これを超えて対象 repo を列挙
する dotfiles 側の registry ファイルは持たない。

出典: `docs/adr/0025-update-own-tools-local-registry.md` — 「本リポ
(public)のソース・Issue・PR に『この名前の private リポが存在する』と
いう情報を書くこと自体が、guard が防ごうとしている漏洩の一種になる」と
して静的列挙を棄却済み(同じ理由がここにも適用される)。実装先行例は
`scripts/github-audit` の `judge_renovate()`(`renovate.json` の存在で
判定)と `list_repos_meta()`(`gh repo list` による動的列挙)。

結果として、drift しうる「写し」がそもそも存在しない。

### D3: App の秘密鍵(PEM)は 1 本を Bitwarden の vault item に単一正本として保管する

App 統合後も秘密鍵は 1 本のまま Bitwarden(Secure Note)に保管し、repo に
撒く直前に取り出して手元の複製は破棄する。

出典: GitHub, "Managing private keys for GitHub Apps"
<https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps>
(取得 2026-09-24)—「最大 25 本」「鍵は失効せず手動 revoke」「生成・削除は
UI のみ(REST API 無し)」。組織内先行例は `docs/operations.md` の
Backblaze/Bitwarden セットアップ節(Bitwarden Secrets Manager + `bws run`
+ keyring の machine token で B2 の秘密を注入する既存運用)と
`docs/adr/0003-secrets-and-identity.md`。

当面 apply スクリプトを作らない(D4)ため、machine account が要る
Secrets Manager ではなく通常の Bitwarden vault item(Secure Note に App
ID + PEM)に置く。SM は machine-to-machine 用で、人間しか読まない秘密には
過剰 — 将来 apply を自動化する時点で SM へ昇格する。

対抗馬として「repo ごとに鍵を発行し、投入後に手元から破棄する(保管先
ゼロ)」を検討したが、採らない — 唯一の売りである被害の局所化が成立し
ない(漏れた鍵は App 全体のトークンを発行でき、全 install 先に届く。局所
化できるのは事後の失効だけ)。その対価として、GitHub が鍵にラベルを付け
られない(fingerprint + 作成日のみ)ため 25 本の天井付近で「鍵↔repo の
対応台帳」が必要になり、それを dotfiles に置けば D2 と同じ理由で棄却さ
れ、host-local に置いても D2 の「写しを持たない」方針と矛盾する。
fine-grained PAT への置き換えも外した(最大 1 年で失効し手動更新が要る、
PR・commit の作者が bot でなく本人になる、workflow 内のトークンが最初
から全 repo 権限を持つ)。sops ランタイム復号の復活(ADR-0010 が退役
済み)や on-disk PEM(ADR-0010 の「ホストに秘密ファイルを置かない」方針
に反する)も外した。

### D4: 執行は `github-audit` の新ドメイン(検出)に限り、apply スクリプトは作らない

secret のコピー漏れは `scripts/github-audit` の `releaser` ドメインが
検出するところまでとし、install や secret を自動で撒くスクリプトは今回
作らない。

出典: `docs/adr/0015-unified-github-audit-and-triage-loop.md` と
`scripts/github-audit` 冒頭コメント「Every domain is judged
deterministically — this script never calls an LLM」(取得 2026-09-24)。
実装先行例は `judge_titles()` の presence-detection 方針コメント。

対抗馬として `scripts/apply-releaser-app.sh` の新設(install API
`PUT /user/installations/{id}/repositories/{repo_id}` — classic PAT +
`repo` scope が必須 — と `gh secret set` の自動化)を検討したが、採らな
い。PEM の実行時入手経路が要り、`github-audit` が初めて秘密を要求する
依存を背負う。repo 数が 10 前後の間は手順書で足りる。skill の手順書修正
だけで済ませる案も外した — publish-guard の「secret 未設定のまま放置」が
再発し続ける。

### D5: App の install 有無は検出しない

install が無ければ `actions/create-github-app-token` が次の release
push で即座に失敗する(loud failure)ため、同じ失敗を 2 箇所で検出しない。
対して secret 未設定は静かに放置される(publish-guard で現に発生してい
た)ので、そこだけ `releaser` ドメインが拾う。

先行例なし: 「別機構が loud に失敗するから検出しない」と明記した既存
ドメインの例は無い(探した範囲: `scripts/github-audit` 全体、
`docs/github-audit.md`、ADR-0015/0020/0023/0031 を一次情報として通読)。

### D6: secret 名をツール中立な `RELEASER_APP_ID` / `RELEASER_APP_PRIVATE_KEY` に統一する

現状 rust 系は `RELEASE_PLZ_APP_*`、astro 系は `RELEASE_PLEASE_APP_*` と
ツール名入りで割れていたが、App がツール非依存になる以上、secret 名も
単一の閉じた組にする。

出典: 組織内先行例として `config/github-audit/*.tsv` の閉語彙
(`docs/adr/0020-generative-repo-governance-rules.md`)と
`scripts/github-audit` の `REQUIRED_PR_TITLE_CONTEXT` 単一定数化
(`docs/adr/0031-pr-title-as-commit-message-contract.md`)。

### D7: releaser App の手順書を `repo-governance-common` に 1 本化する

手順書(App 作成でなく install する旨、secret の配布手順、鍵ローテーション
手順)を `config/claude/skills/repo-governance-common/reference/
releaser-app.md` に置き、`rust-repo-governance` / `astro-site-governance`
の両 skill からこれを参照させる。

出典: commit `01f0858`「refactor(skills): governance skill の
scripts/rulesets 三重化を還元する」tarotene/dotfiles#424(2026-09-24
マージ、取得 2026-09-24)。#424 は実行ファイル(scripts/rulesets)を各
skill 配下へ `claude.nix` で注入したが、今回は参照ドキュメントなので
注入は不要 — `repo-governance-common` ディレクトリ自体が
`home/modules/claude.nix` で `~/.claude/skills/` と `~/.agents/skills/`
の直下に丸ごと配備済みのため、各 skill から相対リンクで届く。

## 執行点

- `scripts/github-audit` — `releaser` ドメイン(`ALL_DOMAINS` への追加、
  `fetch_repo_secrets()`、`judge_releaser()`、audit() への配線、selftest)
  を本 PR で新規追加。
- `docs/github-audit.md` — `### releaser` 節を本 PR で新規追加。

secret 名統一(D6)・手順書の一本化(D7)・4 App の統合(D1 の実運用)は
別段(#434 / Stage 3)で執行する。

## スコープ外

- App の install 有無の検出(D5)。
- install/secret を自動で撒く apply スクリプト(D4)。
- `typst-repo-governance`。現状 release App を使わない(Renovate のみ)。
  必要になった時点で共通 reference を参照させる。
- `scripts/github-audit` の Rust 移行。同ファイルは `rust-migration.toml`
  の Stage 4e target(#414)であり、既存 target ファイルへの追記なので
  `[limits].max_remaining` は動かさない。

## Alternatives considered

Decision 各節の「対抗馬」「外した候補」を参照。
