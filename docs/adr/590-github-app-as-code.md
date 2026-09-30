# ADR-590 — GitHub App の登録・install 先・secret 配布を 3 層すべて宣言 → 検出の対象にする

- Status: Accepted
- Date: 2026-09-30
- Issue: #587(tracking)、#588、#506。`/grill-me` セッション(2026-09-29)で裁定。

## Context

GitHub App には「登録(name/permissions/events)」「install 先集合(どの repo
に付いているか)」「secret 配布」という 3 層があるが、`docs/adr/
436-single-releaser-github-app.md` は 3 層目(secret 配布、`scripts/
github-audit` の `releaser` ドメイン)しか宣言 → 検出の対象にしていない。
1 層目・2 層目は「GitHub API から読めない」という技術的制約(`/user/
installations` は user-to-server token が要る、App 自身の登録は JWT が
要る)を理由に、手書きの状態記述と実 App の乖離を許してきた。

その乖離が実際に起きた(#506 の発端): あるプライベートリポジトリで「Claude
の GitHub App が未グラントだから cloud routine が登録できない」という
手書きの状態記述をドキュメントに残していたところ、実際にはとっくにアクセス
が付与済みだったことが判明した。

**前提の訂正(2026-09-29)**: 「API から読めない」は `gh` の既定 OAuth token
に限った話だった。GitHub REST docs "REST API endpoints for GitHub App
installations" <https://docs.github.com/en/rest/apps/installations>
(取得 2026-09-29)は `PUT/DELETE /user/installations/{id}/repositories/
{repo}` が classic PAT(`repo` scope)専用と明記しており、
`integrations/terraform-provider-github#2103`(取得 2026-09-29)は個人
アカウントでも classic PAT で `GET /user/installations/{id}/repositories`
を読んでいる。所有 App(releaser)自身は JWT で `GET /app`・
`GET /app/installations` が読める(この訂正の影響を受けない)。第三者
App(Claude)側は classic PAT ではなく fine-grained PAT を使う設計にした
(D4 参照、理由は #506 の裁定)。

## Decision

### D1: 3 層すべてを宣言 → 検出の対象にする

登録・install 先集合・secret 配布のどの層でも、手書きの状態記述と実 App の
ずれを機械的に検出できるようにする。所有 App(releaser)と第三者 App
(Claude)で観測手段が異なるため(前者は JWT、後者は fine-grained PAT)、
実装単位も分ける(D3/D4)。

軸: 検出のみ — App の実体は GitHub 側にしかなく、宣言側から状態を固定
できない。できるのは実測との突合だけ。

### D2: 所有 App の登録正本は Manifest JSON に置く

所有 App(releaser)の登録(name/permissions/events)の正本を
`config/github-app-manifests/<name>.json` に置く。新規作成はこの
Manifest を使った GitHub の Manifest フローで行い、事後の drift 検出は
`GET /app` の実測値との突合(`scripts/github-app-registry-check`、段2)で
行う。Manifest の再送信による自動修復はできない — GitHub Docs,
"Modifying a GitHub App registration" <https://docs.github.com/en/apps/
maintaining-github-apps/modifying-a-github-app-registration>
(取得 2026-09-29)は permissions/events の変更が UI 専用で REST API が
無いと明記している。

先行例: GitHub Docs, "Registering a GitHub App from a manifest"
<https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest>
(取得 2026-09-29) — Manifest はブラウザへの POST → 一時 `code` →
`POST /app-manifests/{code}/conversions`(認証不要、1 時間で失効)で
id/pem/webhook_secret を得る、初回登録専用のフロー。本決定はこの同じ
JSON を登録の宣言としてリポジトリにコミットし、事後の drift 検出に転用
する点で公式ドキュメントの想定用途と異なる(差分: 異なる)。
`actions/create-github-app-token` README、`renovatebot/github-action`
README(いずれも取得 2026-09-29)を「manifest / drift / declaration」で
探したが、Manifest を drift 検出の宣言としてリポジトリに保存する事例は
見つからなかった。

軸: 表現不可能 — 宣言 > 手続き(UI クリック手順書より、意図した権限
セットを JSON で固定する)。

### D3: 所有 App の観測は `scripts/github-app-snapshot` が書き、`github-audit` は読むだけ

所有 App の登録実測値(`GET /app`)と install 先集合(`GET /app/
installations` + `GET /installation/repositories`)は、秘密(PEM)を持つ
専用スクリプト `scripts/github-app-snapshot` が JWT で取得し、
`$XDG_STATE_HOME/github-audit/app-snapshot.json` に書く。`scripts/
github-audit` と新設 `scripts/github-app-registry-check`(段2)はこの
ファイルを lazy に読むだけで、どちらも秘密を要求しない — `docs/adr/
436-single-releaser-github-app.md` D4「github-audit は秘密を要求しない」
という不変条件を維持し、reopen しない。

本命: なし — スナップショット生成側に秘密の取得点を 1 つに閉じる設計を
そのまま採る。
対抗馬: github-audit 自身が `bws run` で PEM を取る案(秘密の取得点は
1 つに減るが、上記の不変条件を壊す)— 不採用。
既存手段: `scripts/github-app-snapshot` — 自前 — 却下: `Link-/gh-token`
(installation token の発行のみで `GET /app` の登録情報が取れず、PEM を
ファイル/argv で渡す必要がある)、`integrations/terraform-provider-github`
(tfstate という第二の写しを持ち、所有 App の登録を読む data source が
無い)、`actions/create-github-app-token`(Actions 内専用)。

軸: 還元 — 秘密の取得点をスナップショット生成側の 1 箇所に閉じ、監査側は
既存の「ファイルが無ければ空」読み取り規律(`read_overrides()` 等)に
乗るだけで足りる。

### D4: 第三者 App(Claude)の到達は fine-grained PAT の probe で観測する

Claude の GitHub App 自体の install 先は(D3 と異なり)自分で発行した
JWT では読めない(サードパーティ App の秘密鍵を持たない)。代わりに、
selected repositories スコープの専用 fine-grained PAT(`CLAUDE_WEB_PAT`、
Bitwarden Secrets Manager 保管)を発行し、`GET /repos/{owner}/{repo}` の
`permissions.push` が true かどうかで到達可能な repo を実測する。

fine-grained PAT は public repo を常に読み取れる(`.permissions.push` は
false のまま)ため、単純な 200 応答では判定できない —
`GET /repos/{owner}/{repo}` の `permissions.push` フィールドを使う。

先行例: GitHub Docs, "Managing your personal access tokens"
<https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens>
(取得 2026-09-29) —「Tokens always include read-only access to all public
repositories on GitHub」。#506 の裁定(取得 2026-09-29)は `GET /user/
repos` での一覧化を挙げていたが、これは public repo を過剰計上する
(差分: 異なる)。

軸: 検出のみ — 第三者 App の install 状態は宣言側から固定できず、PAT の
選択範囲を実測するしかない。

### D5: PEM・PAT の保管は Bitwarden Secrets Manager

`docs/adr/436-single-releaser-github-app.md` D3 は「将来 apply を自動化
する時点で Secrets Manager(SM)へ昇格する」としていた。`github-app-
snapshot`(D3)がまさにその自動化にあたるため、この段で昇格を執行する:
所有 App の PEM(1 App あたり `GITHUB_APP_<NAME>_ID`/`_PEM`)と Claude 用
fine-grained PAT(`CLAUDE_WEB_PAT`)を、SM の project `github-apps` に
置く。読み取り専用の machine account を新設し(`obsidian-backup` の
machine account とは分離 — 最小権限、2026-09-30 ユーザー裁定)、その
access token だけを GNOME Keyring に保存する。旧来の「vault item
(Secure Note)」記述は本決定で置き換え、二重保管はしない。

先行例: `docs/operations.md` "One-time Backblaze and Bitwarden setup"、
`scripts/obsidian-backup` の `configure_token()`/`run_with_bitwarden()`
(取得 2026-09-29) — machine token は keyring、payload secret は SM、
実行時 `bws run` で注入し環境ファイルを作らない、という同型の配線を
そのまま踏襲する(差分: 一致)。

軸: 表現不可能 — 単一正本(Secure Note と SM の複写+同期をしない)。

### D6: `docs/adr/436-single-releaser-github-app.md` D5 は段2の Amendment で覆す

D5「App の install 有無は検出しない(loud failure に任せる)」は、D3 が
`GET /app/installations` を安価に提供するようになった以上、その前提
(「二重に検出する機構を持たない」の根拠だったコスト)が成立しなくなる。
覆す作業自体(`judge_releaser()` への `releaser-app-not-installed` 追加、
過剰付与の advisory 化)は段2 の PR で `docs/adr/
436-single-releaser-github-app.md` への `## Amendment` として行う —
この ADR 自身はその方針だけを記録する。

## 執行点

- `scripts/github-app-snapshot`
- `config/github-app-manifests/releaser.json`
- `config/github-app-snapshot/pat-probes.tsv`
- `home/modules/github-apps.nix`
- `home/modules/bitwarden.nix`
- `.github/workflows/ci.yml`

## スコープ外

- 所有 App の登録 ⇔ スナップショットの drift 突合そのもの
  (`scripts/github-app-registry-check`)と `github-audit` の
  `judge_releaser()`/`judge_routines()` 拡張は段2 で実装する(この ADR は
  方針のみを記録し、`## Amendment` を段2 の PR で追記する)。
- Renovate の自前ホスト化(#589)。#587 本文が明記したスコープ外。

## Alternatives considered

Decision 各節の「対抗馬」「外した候補」を参照。

## Amendment (2026-09-30 — PAT の到達判定を private リポジトリの HTTP 200/404 に改める)

D4 は fine-grained PAT の到達を `GET /repos/{owner}/{repo}` の `permissions.push` で判定するとしていた。実機で検証した結果、この判定は成立しない。

- 選択外の public リポジトリでも `permissions.push` は `true` になる。この値は PAT の範囲ではなく、アカウント所有者自身の権限を返す。全 repo が「到達」と判定され、`routines-cloud-access-unclaimed` が全 repo で誤発火する。
- fine-grained PAT は public リポジトリを常に読めるため、`GET /user/repos` にも選択外の public が含まれる。public リポジトリに対する PAT の書き込み範囲は、書き込まずに観測する手段が API に無い。
- 一方、private リポジトリは、選択外なら 404、選択内なら 200 になる(実測)。

このため判定を次のように改める。

- `github-app-snapshot` は private かつ非 fork のリポジトリだけを対象に、`GET /repos/{owner}/{repo}` が HTTP 200 を返すものを到達集合とする。リポジトリの列挙は REST(`user/repos`)で行い、失敗した場合は空の成功として扱わず、その probe を警告付きでスキップする(GraphQL の `gh repo list` はレート制限で黙って空になった)。
- `github-audit` の `routines` ドメインは、到達を `true` / `false` / `unknown` の3値で扱う。public リポジトリは `unknown` とし、`routines-cloud-access-missing` も `routines-cloud-access-unclaimed` も判定しない。`missing` は private で 404 が確定した場合だけ。
- 元案(#506)の `GET /user/repos` での一覧化も、public を含むため採らない。

出典: GitHub Docs, "Managing your personal access tokens"(「Tokens always include read-only access to all public repositories on GitHub」、取得 2026-09-29)と、2026-09-30 の実機実測(public の選択外リポジトリで `permissions.push` が `true`、private の選択外で 404)。

軸: 検出のみ — 到達の実体は GitHub 側にしか無く、観測できる範囲(private のみ)で検出する。

### 執行点

- `scripts/github-app-snapshot`
- `scripts/github-audit`
