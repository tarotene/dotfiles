# personal-web-app — 入力と保存を持つ private な個人用 Web アプリを、Worker + D1 + Access で作る標準

実装: `.github/actions/deploy-worker/action.yml`(`d1-database` 入力と、Website 欄の読み戻し)
決定: `docs/adr/0000-personal-web-app-standard.md`
隣の標準: [`deploy-worker.md`](deploy-worker.md)(閲覧だけの静的サイト)
先行例: 団体向けの先行例のリポジトリ(団員が Discord でログインする Worker + D1。`docs/adr/28`〜`30`・`86`、`wrangler.jsonc`、`app/`)

この文書は、本人だけが使い、画面から値を入れて保存する Web アプリの標準である。認証は団体向けの先行例の Discord でなく、Cloudflare Access(インフラのリポジトリの ADR-16。アカウントの全 Worker を本人だけに閉じる)に任せる。

## どれを選ぶか

| 作るもの | 置き場 | 理由 |
|---|---|---|
| 閲覧だけの静的サイト(private) | `Cloudflare Workers` の静的アセット([`deploy-worker.md`](deploy-worker.md)) | 保存先と Worker のスクリプトが要らない |
| 画面から入力し、保存する(private、本人だけ) | `Cloudflare Workers` + `D1`(この文書) | 入力を受ける口(API)と保存先が要る。`GitHub Pages` は private 公開を `GitHub Enterprise Cloud` に限る(ADR-1553 の Context) |
| 誰でも見てよい | `GitHub Pages`(`repo-governance` の `astro-site` の雛形) | 門を外す宣言が要らない |

claude.ai の Artifact(db つき)は、この標準の対象でない。置き場が別(claude.ai)で、リポジトリの CI・配信・About の URL と一続きにならないため。

## 形

1 つの Worker が、静的アセットと、API のスクリプトと、D1 を持つ。

```jsonc
{
  "$schema": "node_modules/wrangler/config-schema.json",
  "name": "<Worker 名>",            // インフラのリポジトリの bootstrap/sites.tf の「Worker 名」と同じにする
  "main": "app/src/index.ts",
  "compatibility_date": "<UTC の日付>", // JST の「今日」を書かない(UTC では未来日になり、wrangler が拒否する)
  "workers_dev": true,
  "preview_urls": false,            // プレビューを公開する URL を増やさない
  "assets": {
    "directory": "./dist/assets",
    "binding": "ASSETS",
    "run_worker_first": true        // 静的アセットへの要求も、先に Worker を通す
  },
  "d1_databases": [
    { "binding": "DB", "database_name": "<名前>", "database_id": "<UUID>", "migrations_dir": "app/migrations" }
  ],
  "vars": { "TEAM_DOMAIN": "https://<チーム名>.cloudflareaccess.com", "POLICY_AUD": "<AUD タグ>", "OWNER_EMAIL": "<本人のメール>" },
  "observability": { "enabled": false } // 呼び出しのログに URL が入るので、既定で切る(団体向けの先行例の判断)
}
```

- `run_worker_first: true` にする理由: Access の例外(bypass)を誤って足しても、Worker が検証するので、アセットも API も開かない。Cloudflare の文書: 「`run_worker_first = true` will unconditionally invoke your Worker script」(Static Assets, Binding、取得 2026-11-20)。
- Free の注意: 「When using `run_worker_first`, requests matching the specified patterns will always invoke your Worker script.」「If you exceed your free tier request limits, these requests will receive a 429 (Too Many Requests) response」(Static Assets, Billing and limitations、取得 2026-11-20)。1 日 100,000 回は「Accounts on the Workers Free plan have a daily request limit of 100,000 requests」(Platform Limits、取得 2026-11-20)。個人用の量には収まる。
- `vars` の値は秘密でない(`POLICY_AUD` は Access のアプリごとの識別子、`OWNER_EMAIL` は private のリポジトリにだけ書く)。このリポジトリ(dotfiles)には、本人のメールを書かない。

## 認証: Access が門、Worker も検証する

Access が本人以外を門で止める(インフラのリポジトリの `account/access.tf`。新しい Worker は配信した瞬間から閉じている)。それに加え、Worker が `Cf-Access-Jwt-Assertion` を検証して、本人のメールだけを通す。

`ctx.access` には頼れない。Cloudflare の文書: 「Workers with Static Assets execute behind an internal router Worker.」「However, the router does not pass `ctx.access` to the user Worker.」(Cloudflare Access for Workers、取得 2026-11-20)。同じ文書は、Worker では「You do not need to parse or validate the JWT」(Validate JWTs の Workers の節)と書くが、それは `ctx.access` を使う前提である。アセット付きの Worker は、ヘッダの JWT を手で検証する。検査する項目は文書のとおり: 「confirm that `iss` matches your expected team domain」「Also confirm that `aud` contains your application's AUD tag, `exp` is in the future, and `nbf` is not in the future.」「A valid signature alone does not establish that the token is valid for your application.」(Validate JWTs、取得 2026-11-20)。

```ts
import { createRemoteJWKSet, jwtVerify } from "jose";

let jwks: ReturnType<typeof createRemoteJWKSet> | undefined;

// 失敗は、理由を返さず false にする(経路の存在も教えない)。
export async function isOwner(request: Request, env: Env): Promise<boolean> {
  const token = request.headers.get("cf-access-jwt-assertion");
  if (!token) return false;
  try {
    jwks ??= createRemoteJWKSet(new URL(`${env.TEAM_DOMAIN}/cdn-cgi/access/certs`));
    const { payload } = await jwtVerify(token, jwks, {
      issuer: env.TEAM_DOMAIN,
      audience: env.POLICY_AUD,
      algorithms: ["RS256"],
    });
    return payload.email === env.OWNER_EMAIL;
  } catch {
    return false;
  }
}
```

- 鍵は、決め打ちで持たず、証明書の URL から取る。文書: 「Validate tokens using the external endpoint rather than saving the public key as a hard-coded value.」(同上)。署名鍵は定期的に回る(「By default, Access rotates the signing key every 6 weeks.」)。
- 本人の特定は `email` の claim。文書: 「The email address of the authenticated user, verified by the identity provider.」(Application token、取得 2026-11-20)。サービストークンの JWT には `email` が無いので、ここでは通らない(定時実行など、機械から呼ぶ口が要るときは別に設計する)。
- **未実測**: アセット付きの Worker が、アカウント全体の Access の下で、このヘッダを実際に受け取るか。Cloudflare の文書の Workers の例はヘッダを読むが、アセット付きの場合の記述は、取得した資料に無い。最初の app の初回配信で、検証の経路を 1 回通して確かめる(下の「完了条件」)。
- **未確認**: アカウント全体の Access の AUD タグが Worker ごとに違うか。違えば、`POLICY_AUD` を app ごとに持つ(上の雛形のとおり)。

## 経路と書き込み

経路は、誰が呼べるかを宣言する表で持つ。`access` は型で必須で、省略するとコンパイルが通らない。個別の経路に認証の検査を書かせない。

```ts
export type Access = "owner" | "public"; // public は、明示したときだけ。個人用では、ほぼ使わない
export interface Route {
  readonly method: "GET" | "PUT" | "POST" | "DELETE";
  readonly path: string;
  readonly access: Access; // 必須
  readonly handler: (ctx: Context) => Promise<Response>;
}
export const routes = [/* ... */] as const satisfies readonly Route[];
```

ルータの入口の 1 か所で、(1) `access` が `owner` なら `isOwner` を検査し、失敗は 401(本文なし)、(2) 状態を変える要求(PUT・POST・DELETE)は、`Origin` が Worker 自身の origin と完全に一致し、`Content-Type` が `application/json` のときだけ通す。認証の検査は書き込みの関門より先に行う。先行例: 団体向けの先行例 ADR-29 の D2・D3(OWASP の CSRF 対策の Origin 検証)。

## データ: 項目ごとに正本を 1 か所にする

| 正本 | 持つもの |
|---|---|
| git(人が書き、PR で直す) | 計画・一次情報の引用・語彙・スキーマ・コード。ビルドが、配信するアセットと「受ける鍵の一覧(manifest)」を作る |
| D1(アプリで入れる) | 画面で入れた状態(状態語・値・日付) |

アプリは、manifest にある鍵だけを受ける(未知の鍵は 400)。値は、鍵ごとの型(閉じた語彙・日付・長さの上限)で検査する。同じ項目の値を git と D1 の両方に持たない。個人情報(連絡先・実名)の入力欄は作らない。

スキーマは `app/migrations/*.sql`(`wrangler d1 migrations apply`)。D1 の Free の上限は、Cloudflare の文書(D1 Limits、取得 2026-11-20)で、データベース 1 つが 500 MB、Time Travel が 7 日(Paid は 10 GB と 30 日)。

## 配信

build(secret なし)と deploy(secret あり)を分ける。deploy は共通の action を呼び、`d1-database` を渡す。マイグレーションが先に走り、失敗したら配信しない(順序は action が持つので、呼び出し側が間違えられない)。

```yaml
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
      - uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1 # v7.1.0
        with:
          node-version-file: mise.toml
          package-manager-cache: false
      - run: npm ci --ignore-scripts
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: dist
          path: dist
      # `uses:` の参照は、deploy-worker.md の workflow の雛形と同じ(ここでは書かない)
      - uses: <deploy-worker.md の雛形の uses>
        with:
          mode: deploy
          api-token: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          account-id: ${{ secrets.CLOUDFLARE_ACCOUNT_ID }}
          wrangler: npx --no-install wrangler
          d1-database: <wrangler.jsonc の database_name>
```

- プレビューの job は置かない(`mode: preview` と `d1-database` の併用は action が拒否する)。プレビューの version が本番の D1 を指すかは未確認で、指す見込みで扱う。
- token: サイトの deploy token(インフラのリポジトリの `bootstrap/sites.tf`)は `Workers Scripts Write` だけで、マイグレーションと D1 の作成には足りない。D1 を持つ app は、同じ一覧に「D1 の書き込み権限が要る」印を足す宣言が要る(インフラのリポジトリの Issue #20 に追記した。宣言の変更は `bootstrap/` なので、人が適用する)。
- D1 の作成は `wrangler d1 create <名前>`(出た UUID を `wrangler.jsonc` に書く。秘密の値ではない)。

## 配信の最後: About の Website 欄

配信できたら、リポジトリの About の Website 欄に URL を載せる。private は workers.dev の URL、public は Pages の URL。

```bash
gh repo edit -R OWNER/REPO --homepage https://<Worker 名>.<アカウントのサブドメイン>.workers.dev/
```

実態: Cloudflare に置いた学習サイトは workers.dev の URL、Pages に置いた 2 つのサイトは `github.io` の URL(2026-11-20、`gh repo list` で読んだ)。この手順を決めた文面は、dotfiles の `docs/adr`・Issue・個人用の記録のリポジトリには見つからなかった。deploy の action は、配信のあとで欄を読み戻し、空なら警告する(設定そのものは管理者権限が要るので、action は行わない)。

## 完了条件(最初の app の初回配信で確かめる)

1. ログインなしで URL を開くと、Access のログインへ転送される(門が閉じている)。
2. 本人でログインして、画面が出る。
3. **Worker が JWT を受け取って検証に通っている**(未実測の確認)。検証を壊した状態(`POLICY_AUD` を間違える)で 401 になることも、1 回確かめる。
4. 未知の鍵への書き込みが 400 になる。
5. About の Website 欄に URL が載っている。

## バックアップ

app の README に、段を宣言する。

| 段 | 条件 | 対応 |
|---|---|---|
| 再入力できる | 入れ直しが数分で済む状態だけ | D1 の Time Travel(Free で 7 日)だけ |
| 戻せない | 失うと戻せない記録を含む | 団体向けの先行例の ADR-86 の型(`wrangler d1 export --remote` を定時に実行し、`age` で暗号化した `*.age` だけを artifact に置く) |

`wrangler d1 export` の制約: 「A running export will block other database requests.」「Export is not supported for virtual tables」(D1 の Import and export、取得 2026-11-20)。

## 適用例: 登山の計画を管理する個人用リポジトリ(ダッシュボード)

そのリポジトリの README は、計画・装備・下調べの正本を markdown に置く。この標準に当てはめると次のとおり(app 本体は、そのリポジトリの Issue で追う)。

- git(markdown)が持つ: 必携品の役割と一次情報の記述、判断表の項目と基準、行程表、規制と運行の出典(題名・URL・取得日)。ビルドが `pandoc` で表を JSON にし、アセットと manifest を作る。
- D1 が持つ(manifest の鍵): `gear:<役割>`(状態語は `持っている|買う|借りる|不要|確認` の 5 語)、`go:<項目>:<前夜|当日>`(判断表の値。短い文)、`yamap:<項目>`(地図のダウンロード・登山計画の保存・登山届の提出・みまもりの通知先の**済んだ日付**)。みまもりの通知先の連絡先は、入力欄を作らない。
- 経路: `GET /`(ダッシュボード)、`GET /api/state`(D1 の値)、`PUT /api/state/<鍵>`(manifest にある鍵だけ)。いずれも `access: "owner"`。
- 導出: 山行までの残り日数、日の入りと目標時刻、取得日の古さの警告は、今日の日付から導く(宣言の欄を作らない)。
- 山の中は圏外が多いので、入力は自宅と登山口までで使う前提にする。

## 未実測・未確認(2026-11-20)

- アセット付きの Worker が、Access の JWT のヘッダを受け取るか(未実測。完了条件 3)。
- アカウント全体の Access の AUD タグが Worker ごとに違うか(未確認)。
- `jose` の保守表示と最終 commit(未確認。採用する app の実装前に読む)。
- プレビューの version が本番の D1 を指すか(未確認)。
- action の実際の実行(`d1-database` と Website 欄の読み戻し)。偽の `wrangler` と `gh` で、呼び出しの順序・失敗時の停止・警告の出力を確かめただけで、Cloudflare に対する実行は、最初の app の CI で確かめる。
