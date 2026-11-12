# deploy-worker — private リポジトリの静的サイトを Workers に置き、配信の手順を共通の action で呼ぶ

実装: `.github/actions/deploy-worker/action.yml`(composite action)
決定: `docs/adr/1553-deploy-worker-action.md`
走る場所: サイトのリポジトリの GitHub Actions(`uses: tarotene/dotfiles/.github/actions/deploy-worker@main`)

## どちらに置くか

| サイトのリポジトリ | 置き場 | 理由 |
|---|---|---|
| public | `GitHub Pages`(`repo-governance` の `astro-site` の雛形) | 誰でも見られてよい。Workers に寄せても得るものが無い |
| private で、閲覧を絞りたい | `Cloudflare Workers`(この文書) | `GitHub Pages` は、private 公開を組織の `GitHub Enterprise Cloud` に限っている(ADR の Context) |
| private で、誰でも見られてよい | どちらでもよい | 公開にするなら、Workers では「門を外す」宣言が 1 つ要るので、`GitHub Pages` が軽い |

Workers に置いたサイトの閲覧は、`Cloudflare Access` が GitHub のログインで絞る。既定は「アカウントの全 Worker を
本人だけに閉じる」で、新しい Worker は配信した瞬間から非公開である。門・ログイン・共同開発者の許可・deploy 用の token と
secret は、アカウントの `Cloudflare` を宣言するインフラのリポジトリ(`OpenTofu`、非公開)が持つ。サイトのリポジトリは
`wrangler.jsonc` と workflow だけを持つ。

## 新しい private サイトの手順

1. インフラのリポジトリで、サイトの一覧(`bootstrap/sites.tf` の `site_repos`)に「Worker 名 = リポジトリ名」を 1 行足し、
   人が `bootstrap/` を適用する。サイトのリポジトリに、deploy 用の token と、アカウント ID の secret が入る。
2. サイトのリポジトリに、下の `wrangler.jsonc` と workflow を置く。`name` は 1 で書いた Worker 名と同じにする
   (門の宣言が、この名前で Worker を引く)。
3. `main` へ入れる。最初の配信で、Worker が作られる。それまで Worker は存在しないので、最初の PR の
   `Preview` は必ず失敗する(下の「失敗したとき」)。`Preview` は参考用(`continue-on-error`)で、`CI passed` は通る。
4. 閲覧の確認: ログインなしで `https://<Worker 名>.<アカウントの workers.dev のサブドメイン>.workers.dev/` を開くと、
   ログイン画面へ転送される。GitHub でログインして、本文が出る。
5. 共同開発者に見せるときは、インフラのリポジトリで、その Worker の許可にメールか Org のチームを足す(PR 1 本)。

## `wrangler.jsonc`(静的アセットだけ)

`compatibility_date` は UTC の日付で書く。JST の「今日」を書くと、UTC では未来日になり、`wrangler` が拒否する。

```jsonc
{
  "$schema": "node_modules/wrangler/config-schema.json",
  "name": "<Worker 名>",
  "compatibility_date": "<UTC の日付>",
  "workers_dev": true,
  "preview_urls": true,
  "assets": {
    "directory": "./dist",
    "html_handling": "auto-trailing-slash",
    "not_found_handling": "404-page"
  }
}
```

## workflow の雛形

サイトの `ci.yml` に足す 3 つの job である。build は secret を持たず、deploy と preview だけが持つ。
`wrangler` はサイトの lockfile で固定し、token を持つ job では依存のスクリプトを走らせない。
`ci-passed` の `needs` に、`deploy` と `preview` を足す。`uses: tarotene/dotfiles/...@main` は、`repo-governance` が
播く `.github/zizmor.yml`(`tarotene/*` だけを ref の固定まで緩める)が無いと、`zizmor` が `unpinned-uses` で落とす。

```yaml
jobs:
  build:
    name: Build
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7
        with:
          persist-credentials: false
      # サイトのビルド(依存の導入と build)。成果物は ./dist に出す。
      - run: echo "replace with the site build"
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: dist
          path: dist
          if-no-files-found: error
          retention-days: 7

  deploy:
    name: Deploy
    needs: [build]
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    runs-on: ubuntu-latest
    permissions:
      contents: read
    concurrency:
      group: deploy-production
      cancel-in-progress: false
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7
        with:
          persist-credentials: false
      # wrangler を lockfile から入れる。install スクリプトは走らせない。
      - run: npm ci --ignore-scripts
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: dist
          path: dist
      - uses: tarotene/dotfiles/.github/actions/deploy-worker@main
        with:
          mode: deploy
          api-token: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          account-id: ${{ secrets.CLOUDFLARE_ACCOUNT_ID }}
          wrangler: npx --no-install wrangler

  preview:
    name: Preview
    needs: [build]
    if: github.event_name == 'pull_request' && github.actor != 'dependabot[bot]'
    runs-on: ubuntu-latest
    continue-on-error: true
    permissions:
      contents: read
      pull-requests: write
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7
        with:
          persist-credentials: false
      - run: npm ci --ignore-scripts
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: dist
          path: dist
      - uses: tarotene/dotfiles/.github/actions/deploy-worker@main
        with:
          mode: preview
          api-token: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          account-id: ${{ secrets.CLOUDFLARE_ACCOUNT_ID }}
          wrangler: npx --no-install wrangler
```

## 失敗したとき

| 症状 | 見るところ |
|---|---|
| deploy が警告だけで成功し、配信されない | `api-token` が空。インフラのリポジトリの `bootstrap/sites.tf` に、このサイトが載っていて適用済みかを見る |
| `wrangler` が認証で落ちる | token の期限切れ。インフラのリポジトリで `bootstrap/` を適用し直す(token と secret が更新される) |
| 配信できたが、ログインなしで本文が見える | 門の宣言が外れている。インフラのリポジトリで、全 Worker を守る宣言と、その Worker の公開(門を外す)宣言の有無を見る |
| プレビューの URL が PR に載らない | `preview` の job に `pull-requests: write` があるか。`dependabot` の PR は secret が無く、飛ばされる |
| 新しいサイトの最初の PR で `Preview` が「You cannot upload a new version of a Worker that does not yet exist」で落ちる | 想定どおり。Worker がまだ無いので、`wrangler` は version を上げられない。`main` への最初の配信で Worker ができれば、以後の PR では動く(実測: 2026-11-12、認証は通っていた) |
