# ADR-0000 — 静的サイトの配信を composite action `deploy-worker` に集め、private リポジトリの静的サイトは Workers に置く

- Status: Accepted
- Date: 2026-11-12
- Issue: No-Issue(`/grill-me` セッションで、private リポジトリの静的サイトを、GitHub のアカウントで認証して自分と共同開発者にだけ見せる標準を持ちたい、という依頼から始まった)

## Context

private リポジトリの静的サイトを `GitHub Pages` で出す標準は、閲覧を絞れない。GitHub の文書は
「To publish a GitHub Pages site privately, your organization must use GitHub Enterprise Cloud.」と述べる
(GitHub Docs, "Changing the visibility of your GitHub Pages site",
<https://docs.github.com/en/pages/getting-started-with-github-pages/changing-the-visibility-of-your-github-pages-site>、取得 2026-11-12)。
個人のアカウントは組織ではないので、リポジトリを private にしても、サイトを非公開にする手段が無い。

一方、静的アセットだけの `Cloudflare Worker` は、`Cloudflare Access` を Worker 自体に結び付けられる。
`workers.dev`・独自ドメイン・プレビュー URL のすべてが同じ門で守られ、アカウントの全 Worker をまとめて守って、
公開したい Worker だけ外すこともできる(Cloudflare, "Cloudflare Access" の Workers 版の文書、
<https://developers.cloudflare.com/workers/configuration/cloudflare-access/>、取得 2026-11-12)。
このリポジトリの利用者のサイトは、すでに「GitHub Actions でビルドし、`wrangler` で Workers へ配信する」形で動いており、
配信の手順(token が無いときの扱い、PR のプレビュー)が、サイトのリポジトリごとの複写になっていた。

## Decision

### D1: 配信の手順は composite action `deploy-worker` に 1 つだけ置く

`.github/actions/deploy-worker/action.yml` が、`mode: deploy`(本番)と `mode: preview`(PR のプレビュー)の 2 つを担う。
token が空なら、落とさずに警告して成功で終わる(token を渡す前に `main` を赤くしない)。wrangler は呼び出し側の
lockfile で固定し、`wrangler` 入力で呼ぶ。依存の導入とビルド成果物の取得は呼び出し側が済ませる(token を持つ job で、
この action が依存のスクリプトを走らせない)。

先行例: tarotene/dotfiles `.github/actions/pr-title/action.yml`(ADR-591 D3。共通の検査を composite action に置き、呼び出し側を数行にする) (取得 2026-11-12)
差分: 一致 — reusable workflow でなく composite action を選ぶ理由(呼び出し側 job の名前だけが check の名前になる)も同じ。
本命: なし — 雛形の複写だけの案と比べて決めた。
対抗馬: 雛形(workflow)を各リポジトリに複写する(手順の修正が全リポジトリへの同期になる)。`cloudflare/wrangler-action` を中に包む(wrangler の導入と版の扱いが action の内側に入り、呼び出し側の lockfile と二重になる)。
外した候補: なし
既存手段: 既存のサイトの CI の deploy・preview の手順 — 拡張: 同じ手順を action に持ち上げる。
軸: 表現不可能 — 配信の手順の正本が 1 つになり、サイトごとの複写がずれる状態を作れない。

### D2: private リポジトリの静的サイトは Workers に置き、public の静的サイトは `GitHub Pages` のままにする

private のサイトだけが、閲覧を絞る必要を持つ。public のサイトは、Workers に寄せても得るものが無い(門を外す宣言が
増えるだけ)。分岐の判断と、新しいサイトの手順は `docs/agents/deploy-worker.md` に置く。

先行例: GitHub Docs, "Changing the visibility of your GitHub Pages site"(上の Context) (取得 2026-11-12)
差分: 異なる — GitHub が private 公開を組織の Enterprise Cloud に限っているので、個人のアカウントの private リポジトリは別の置き場を要する。
本命: なし — public も Workers に寄せる案と比べて決めた。
対抗馬: public の静的サイトも Workers に寄せる(配信の型は 1 つになるが、公開したい Worker ごとに門を外す宣言が要る)。
外した候補: 組織を Enterprise Cloud にして private Pages を使う — 感触で外した(プランの変更を伴う。比較は行っていない)。
既存手段: `GitHub Pages` — 採用(public のサイト)。
軸: 還元 — 閲覧を絞る仕事をしないサイトの配信の型を、増やさない。

## Alternatives considered

D1 と D2 の「対抗馬」と「外した候補」を参照。ほかには検討していない。

## Consequences

- 呼び出し側は、`wrangler.jsonc`(静的アセットだけ)と、build(secret なし)と deploy(secret あり)を分けた workflow を持つ。
  雛形は `docs/agents/deploy-worker.md` にある。
- token と `Cloudflare Access` の宣言は、この action の外(アカウントのインフラを宣言するリポジトリ)にある。
  この action は、それらが宣言済みであることを前提にする。
- action は `@main` で呼ばれる。入力を変えるときは、呼び出し側を壊さない追加にする。

## 執行点

- `.github/actions/deploy-worker/action.yml`

## Verification

- `zizmor` 1.31.1(`--offline`)を `action.yml` に当てて、指摘は 0 件(2026-11-12)。
- `docs/agents/deploy-worker.md` の workflow の雛形を実ファイルにして、`actionlint` 1.7.12 は指摘 0 件。`zizmor` は、`deploy-worker@main`
  への `unpinned-uses` 2 件だけを返した。`repo-governance` が播く `zizmor.yml` の `tarotene/*: ref-pin` が、この型を許可する
  (既存の `pr-title@main` と同じ。雛形に `zizmor.yml` を当てた実行はしていない)。
- 未実施: action 自体の実行。最初の利用者のサイトの CI で確かめる。
