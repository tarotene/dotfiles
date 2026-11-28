# deploy-worker — private リポジトリの静的サイトを Workers に配置し、配信手順を共通の action にまとめる

実装: `.github/actions/deploy-worker/action.yml`(composite action)
決定: `docs/adr/1553-deploy-worker-action.md`
走る場所: サイトのリポジトリの GitHub Actions(`uses: tarotene/dotfiles/.github/actions/deploy-worker@main`)

入力と保存の機能を持つアプリ(Worker + D1)は [`personal-web-app.md`](personal-web-app.md)(決定は `docs/adr/1660-personal-web-app-standard.md`)を参照。
そのアプリが使う入力 `d1-database`(配信前にマイグレーションを適用する)・`vars`(`wrangler deploy --var` で値を渡す)・`access-login-check`(配信後にログインなしで 302 のログイン転送になることを確かめる)は、この action が持つ。
配信後にリポジトリの About の Website 欄を読み戻し、空なら警告する(静的サイトも同じ。設定は人が `gh repo edit OWNER/REPO --homepage URL` で行う)。新しい Worker の最初の配信が `10007` で失敗したときは、action が `wrangler triggers deploy` を間隔を空けて再試行する(下の「失敗したとき」)。

## どちらに置くか

| サイトのリポジトリ | 置き場 | 理由 |
|---|---|---|
| public | `GitHub Pages`(`repo-governance` の `astro-site` の雛形) | 誰でも見られてよい。Workers に寄せても得るものがない |
| private で閲覧を絞りたい | `Cloudflare Workers`(この文書) | `GitHub Pages` は private 公開を組織の `GitHub Enterprise Cloud` に限っている(ADR の Context) |
| private で画面から入力して保存する(本人のみ) | `Cloudflare Workers` + `D1`([`personal-web-app.md`](personal-web-app.md)) | 入力を受け付ける口と保存先が必要。閲覧の制限はこの文書と同じ `Cloudflare Access` |
| private で誰でも見られてよい | どちらでもよい | 公開にするなら Workers ではアクセス制限を外す宣言が 1 つ必要になるため、`GitHub Pages` が手軽 |

Workers に置いたサイトの閲覧は、`Cloudflare Access` が GitHub のログインで絞る。既定は「アカウントの全 Worker を本人だけに閉じる」であり、新しい Worker は配信した瞬間から非公開になる。アクセス制限・ログイン・共同開発者の許可・deploy 用の token と secret は、アカウントの `Cloudflare` を宣言するインフラのリポジトリ(`OpenTofu`、非公開)が持つ。サイトのリポジトリは `wrangler.jsonc` と workflow だけを持つ。

## 新しい private サイトの手順

1. インフラのリポジトリでサイト一覧(`bootstrap/sites.tf` の `site_repos`)に「Worker 名 = リポジトリ名」を 1 行追加し、人が `bootstrap/` を適用する。サイトのリポジトリに deploy 用の token とアカウント ID の secret が入る。D1 を持つアプリは `d1 = true`(D1 とその書き込み権限の宣言)、JWT をサイトで検証するアプリは `app = true`(Access の値が Actions 変数として入る)の印を付ける([`personal-web-app.md`](personal-web-app.md))。静的サイトはどちらも `false` にする。
2. サイトのリポジトリに下の `wrangler.jsonc` と workflow を配置する。`name` は 1 で書いた Worker 名と同じにする(アクセス制限の宣言がこの名前で Worker を参照する)。
3. `main` へ入れる。最初の配信で Worker が作成される。それまで Worker は存在しないため、最初の PR の `Preview` は必ず失敗する(下の「失敗したとき」)。`Preview` は参考用(`continue-on-error`)のため、`CI passed` は通る。
4. 閲覧の確認: ログインなしで `https://<Worker 名>.<アカウントの workers.dev のサブドメイン>.workers.dev/` を開くとログイン画面へ転送される。GitHub でログインして本文が表示されることを確認する。
5. 共同開発者に見せるときは、インフラのリポジトリでその Worker の許可にメールか Org のチームを追加する(PR 1 本)。

## `wrangler.jsonc`(静的アセットだけ)

`compatibility_date` は UTC の日付で書く。JST の「今日」を書くと UTC では未来日になり、`wrangler` が拒否する。

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

サイトの `ci.yml` に追加する 3 つの job である。build は secret を持たず、deploy と preview だけが持つ。
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

| 症状 | 確認箇所 |
|---|---|
| deploy が警告だけで成功し、配信されない | `api-token` が空。インフラのリポジトリの `bootstrap/sites.tf` に、このサイトが載っていて適用済みかを確認する |
| `wrangler` が認証で失敗する | token の期限切れ。インフラのリポジトリで `bootstrap/` を適用し直す(token と secret が更新される) |
| 配信できたが、ログインなしで本文が見える | アクセス制限の宣言が外れている。インフラのリポジトリで全 Worker を守る宣言と、その Worker の公開設定の有無を見る |
| プレビューの URL が PR に載らない | `preview` の job に `pull-requests: write` があるか確認する。`dependabot` の PR は secret がないためスキップされる |
| 新しいサイトの最初の PR で `Preview` が「You cannot upload a new version of a Worker that does not yet exist」で失敗する | 想定どおり。Worker がまだないため `wrangler` は version を上げられない。`main` への最初の配信で Worker が作成されれば、以後の PR では動く(実測: 2026-11-12、認証は通っていた) |
| 新しいサイトの最初の配信が、アップロード後に `10007`(This Worker does not exist on your account)で失敗する | action が `wrangler triggers deploy` を最大 6 回(約 10 分)再試行する。続くなら Worker が作られているか(ダッシュボードの Workers 一覧)を確認し、`wrangler triggers deploy` を手で 1 回実行する。原因は cloudflare/workers-sdk#17069 の報告(Worker 作成直後の数分間、エンドポイントが 404 を返す)とみられるが、確かめた事実ではない(実測: 2026-11-20、本人の `wrangler triggers deploy` が数分後に通った) |
| `access-login-check` で配信 job が「302 で転送されません」と失敗する | アクセス制限が開いている。インフラのリポジトリの全 Worker を守る宣言と、その Worker の公開設定の有無を見る。公開 Worker にはこの入力を渡さない |
| `vars` の「値が空です」でアップロード前に失敗する | そのサイトのリポジトリに Actions 変数が届いていない。インフラのリポジトリの `bootstrap/` を適用し、サイト一覧の `app` の印を確認する |
