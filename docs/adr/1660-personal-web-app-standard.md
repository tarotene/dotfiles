# ADR-1660 — 入力と保存を持つ private な個人用 Web アプリは、Worker + D1 + Access を標準にし、マイグレーションと About の URL を共通 action に載せる

- Status: Accepted
- Date: 2026-11-20
- Issue: No-Issue(登山の計画を管理する個人用リポジトリのダッシュボードを作りたいという依頼から始まり、入力は Artifact でなく Web アプリとして設計し、標準設計を中央に置きたい、という本人の指示で決まった。今回は設計までで、その app 本体は作らない)

## Context

private リポジトリの閲覧専用の静的サイトの標準は、ADR-1553 が決めた(Worker の静的アセットを Cloudflare Access で本人だけに閉じる。`docs/agents/deploy-worker.md`)。画面から入力して保存する個人用のアプリには、標準が無かった。先行例は団体向けの先行例のリポジトリ(Worker + D1。団員が Discord でログインする)で、ここでは認証が違う。

2026-11-20 に調べて分かったこと。

- 静的アセットを持つ Worker は、内部のルータの後ろで動き、そのルータは `ctx.access` をスクリプトに渡さない。Cloudflare の文書: 「Workers with Static Assets execute behind an internal router Worker.」「However, the router does not pass `ctx.access` to the user Worker.」(Cloudflare Access for Workers、https://developers.cloudflare.com/workers/configuration/cloudflare-access/ 、取得 2026-11-20)。
- Worker の JWT の検証は、文書では `ctx.access` を使えば要らないと書かれている(「You do not need to parse or validate the JWT.」、Validate JWTs、https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/authorization-cookie/validating-json/ 、取得 2026-11-20)。アセット付きではその経路が使えない。
- About の Website 欄は、`gh repo list` で、Cloudflare に置いた学習サイト(workers.dev)、Pages に置いた 2 つのサイト(`github.io`)、公開の crate(crates.io)に入っている。その手順を決めた文面は、この ADR を書く時点で、dotfiles の `docs/adr`・Issue・個人用の記録のリポジトリに見つからなかった。インフラのリポジトリの Issue #20 の手順 6 に `gh repo edit --homepage` がある。

## Decision

標準の本体は `docs/agents/personal-web-app.md`。この ADR は、その決定と、先行例との対比を残す。

### D1: 標準は dotfiles の ADR と `docs/agents/personal-web-app.md` に置き、閲覧専用の標準の表から指す

先行例: ADR-1553 D2 とその分岐表 docs/adr/1553-deploy-worker-action.md (取得 2026-11-20)
差分: 一致 — 置き場の分岐の正本を 1 つの表に集める型を、入力を持つ app の行へ広げる。
軸: 表現不可能 — 単一正本 > 複写+同期。

### D2: 形は 1 Worker に静的アセットとスクリプトと D1 を載せ、`run_worker_first: true`、`preview_urls: false` にする

先行例: 団体向けの先行例 ADR-29 D1 と `wrangler.jsonc`(団体向けの先行例のリポジトリ、取得 2026-11-20)。Cloudflare, Static Assets の binding と billing(https://developers.cloudflare.com/workers/static-assets/binding/ 、取得 2026-11-20)
差分: 一致 — Free では、`run_worker_first` で通した要求が Worker 呼び出しに数えられ、超過で 429(billing 文書の「Important note for free tier users」)。個人用の量には収まる。
軸: 表現不可能 — Access の例外を誤って足しても、Worker が検証するので、アセットと API が開かない。

### D3: Access の門に加え、Worker が JWT を `jose` で検証し、本人のメールだけを通す

本命: なし — Cloudflare の文書の標準の例が `jose` を使う。文書の例から出発した。
対抗馬: WebCrypto だけの自前検証(同じ評価軸「署名・`iss`・`aud`・`exp` の検査を、検証済みの部品に任せるか」。依存は増えないが、手書きの検査が残る)。
外した候補: `ctx.access` に頼る案 — 分析で外した(上の Context。アセット付きでは渡されない)。
既存手段: docs/agents/personal-web-app.md — 採用: jose (https://github.com/panva/jose)。呼び方は Cloudflare の文書の例と同じ。
実測: 未実測 — アセット付きの Worker が、アカウント全体の Access から JWT のヘッダを受け取るか。確かめるには配信した Worker にログインして触れる必要があり、今回は app を配信しない。最初の app の初回配信の完了条件に書いた。
未確認: `jose` の保守表示と最終 commit。アカウント全体の Access の AUD タグが Worker ごとに違うか。
先行例: Cloudflare, Validate JWTs(上の URL、取得 2026-11-20)
差分: 異なる — 文書は Worker なら検証が要らないと書くが、アセット付きでは `ctx.access` が使えないので、手動の検証を標準にする。
軸: 表現不可能 — 宣言(Access)と実体(Worker)の二重で、片方の宣言の誤りが公開に直結しない。

### D4: 経路は `access` を型で必須にした表で宣言し、書き込みの関門を入口の 1 か所に置く

先行例: 団体向けの先行例 ADR-29 D2・D3(取得 2026-11-20)
差分: 一致 — 区分は `owner` と、明示した `public` の 2 つにする(個人用)。
軸: 表現不可能 — 型・コンパイル時 > 実行時検査。

### D5: 項目ごとに正本を 1 か所にし、アプリが受ける鍵は、ビルドが git から導く閉集合だけにする

先行例: 団体向けの先行例 ADR-28 D1(取得 2026-11-20)
差分: 異なる — 団体向けの先行例は全部を D1 へ移した。ここは、人が読む計画と一次情報の引用を git の文書の正本に残し、D1 にはアプリで入れる状態だけを持つ。
軸: 表現不可能 — 単一正本(項目単位)+ 閉語彙。

### D6: 実装言語は TypeScript にし、テストは Node 組み込みの SQLite で D1 の代用品を作る。依存は完全一致で pin する

本命: なし — 団体向けの先行例の決定が既にあり、白紙の評価として比べ直した。
対抗馬: Rust(workers-rs)(同じ評価軸「保存状態の不変条件をどこで担保するか」。保存状態は SQL のスキーマが担うので、言語の型の強さは差を生まない。D1 対応と UI の言語数が差を決めた、と団体向けの先行例 ADR-28 D3 が記す。原文の確認は未)。
外した候補: Python Workers — 感触で外した(別に JavaScript の UI が要る点が気になった)。分析ではない。
既存手段: docs/agents/personal-web-app.md — 拡張: 団体向けの先行例の `app/` の構成を骨子として文書に載せる。共有の部品にはしない。使う app が 1 つのあいだは、複写で足りる。2 つ目の app が出たら裁定する。
先行例: 団体向けの先行例 ADR-28 D3 と ADR-29 D6(取得 2026-11-20)
差分: 一致
軸: 検出のみ — 骨子の複写は、ずれを検出できるだけ。

### D7: マイグレーションと配信の順序を、共通 action `deploy-worker` の入力 `d1-database` に持たせる

`mode: deploy` に `d1-database` を渡すと、配信の前に `wrangler d1 migrations apply <名前> --remote` を実行し、失敗したら配信しない。`mode: preview` との併用は拒否する(プレビューの version が本番の D1 を指さないことを、確かめていない)。

先行例: 団体向けの先行例 ADR-29 D5 と ADR-1553 D1 docs/adr/1553-deploy-worker-action.md (取得 2026-11-20)
差分: 異なる — 団体向けの先行例は workflow の step の並びで順序を保つ。ここは action の 1 呼び出しにして、順序を間違えられない形にする。seed は標準に入れない。
軸: 表現不可能 — 手続き(step の並び)より宣言(1 呼び出し)。

### D8: バックアップは、再入力できるかで段を分ける

先行例: 団体向けの先行例 ADR-86(取得 2026-11-20)。Cloudflare, D1 limits(Time Travel は Free で 7 日、https://developers.cloudflare.com/d1/platform/limits/ 、取得 2026-11-20)
差分: 異なる — ADR-86 は団員の出欠が戻せないので必須。個人の状態は再入力できる場合があるので、app が README に段を宣言する。
軸: 検出のみ — 宣言を読んで判断するだけで、強制はしない。

### D9: 配信の最後に About の Website 欄を埋め、action は配信のあとで欄を読み戻して、空なら警告する

先行例: Cloudflare に置いた学習サイト（workers.dev）、Pages に置いた 2 つのサイト（`github.io`）、公開の crate（crates.io）の実態と、インフラのリポジトリの Issue #20 の手順 6（非公開のため URL は書かない、取得 2026-11-20）
差分: 一致 — ただし、決めた文面は見つからなかった。設定そのものは管理者権限が要り、`GITHUB_TOKEN` では行えない見込み（未確認）なので、設定は人かエージェントが行い、action は読み戻しの警告だけを持つ。
軸: 検出のみ — 設定の欠落を表現不可能にはできず、警告で見える形にする。

## Alternatives considered

D3 の対抗馬と外した候補、D6 の対抗馬と外した候補を参照。入力を claude.ai の Artifact(db つき)で受ける案は、置き場がリポジトリの CI・配信・About の URL と一続きにならないので、この標準の対象から外した(本人の指示)。

## Consequences

- D7 と D9 は、`deploy-worker` の action が執行する。D2〜D6 と D8 は、この標準を使う最初の app が、型・テスト・CI で執行する(その app の PR で執行点が付く)。この ADR の執行点は、action だけである。
- D1 を持つ app の deploy token には、D1 の書き込み権限が要る。インフラのリポジトリの `bootstrap/sites.tf` の token は `Workers Scripts Write` だけで、サイト一覧に D1 の印を足す宣言が要る(`bootstrap/` は人が適用する。インフラのリポジトリの Issue #20 に追記する)。
- 既存の呼び出し(`d1-database` を渡さない静的サイト)の挙動は変わらない。配信のあとに、Website 欄が空なら警告が出る(失敗にはしない)。

## 執行点

- `.github/actions/deploy-worker/action.yml`

## Verification

- `zizmor` 1.32.0(`--offline`)を `action.yml` に当てて、指摘は 0 件(2026-11-20)。取り出した `run:` の 4 つのブロックに `shellcheck` を当て、指摘は既存の SC2153(info、`WRANGLER` は step の env)だけ。
- `yq` で `run:` のブロックを取り出し、偽の `wrangler` と偽の `gh` で動かした(2026-11-20)。(1) `d1-database` あり: `d1 migrations apply <名前> --remote` のあとに `deploy`。(2) マイグレーションが失敗: 配信の step は走らない。(3) `d1-database` なし: `deploy` だけ(従来どおり)。(4) token が空: 何もせず警告。(5) `mode: preview` と `d1-database` の併用、`-` で始まる名前: エラー。(6) Website 欄が空: 警告、入っている: notice、読めない: 警告。`if:` の条件は、この試験では手で再現した(Actions の評価器では動かしていない)。
- 実在のリポジトリで、読み戻しの式 `gh api repos/<repo> --jq '.homepage // ""'` を確かめた: Cloudflare に置いた学習サイトは workers.dev の URL、欄が空の個人用リポジトリは空。
- 未実施: Cloudflare に対する実行(`d1-database` の実行と、アセット付きの Worker への JWT のヘッダの到達)。最初の app の初回配信で確かめる。`actionlint` は、この機体に入っておらず、当てていない。
