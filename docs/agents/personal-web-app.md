# personal-web-app — 入力と保存を持つ private な個人用 Web アプリを、Worker + D1 + Access で作る標準

実装: `.github/actions/deploy-worker/action.yml`(`d1-database`・`vars`・`access-login-check` の入力、初回配信の `10007` の再試行、Website 欄の読み戻し)
決定: `docs/adr/1660-personal-web-app-standard.md`
隣の標準: [`deploy-worker.md`](deploy-worker.md)(閲覧専用の静的サイト)
先行例: 団体向けのリポジトリ(団員が Discord でログインする Worker + D1。`docs/adr/28`〜`30`・`86`、`wrangler.jsonc`、`app/`)

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
    // database_id は書かない。D1 はインフラのリポジトリが `<Worker 名>-db` の名前で宣言して作り、wrangler が名前で引く
    { "binding": "DB", "database_name": "<Worker 名>-db", "migrations_dir": "app/migrations" }
  ],
  // vars は書かない(TEAM_DOMAIN・POLICY_AUD・OWNER_EMAIL は、配信の action が Actions 変数から --var で渡す。下の「配信」)
  "observability": { "enabled": false } // 呼び出しのログに URL が入るので、既定で切る(団体向けの先行例の判断)
}
```

- `run_worker_first: true` にする理由: Access の例外(bypass)を誤って足しても、Worker が検証するので、アセットも API も開かない。Cloudflare の文書: 「`run_worker_first = true` will unconditionally invoke your Worker script」(Static Assets, Binding、取得 2026-11-20)。
- Free の注意: 「When using `run_worker_first`, requests matching the specified patterns will always invoke your Worker script.」「If you exceed your free tier request limits, these requests will receive a 429 (Too Many Requests) response」(Static Assets, Billing and limitations、取得 2026-11-20)。1 日 100,000 回は「Accounts on the Workers Free plan have a daily request limit of 100,000 requests」(Platform Limits、取得 2026-11-20)。個人用の量には収まる。
- `TEAM_DOMAIN`・`POLICY_AUD`・`OWNER_EMAIL` は秘密でないが、サイトのリポジトリには複写しない。インフラのリポジトリが、チーム名・AUD・本人のメールを宣言し(`account/` の出力を `bootstrap/` が読む)、`app` の印のあるサイトのリポジトリの Actions 変数として書く(インフラのリポジトリの ADR-16 Amendment 2026-11-21)。サイトの `wrangler.jsonc` に値を書くと、AUD が変わったときにずれる(最初の app では、AUD をダッシュボードで読んで手で複写した)。このリポジトリ(dotfiles)には、本人のメールを書かない。
- `database_id` を書かない理由: `wrangler` は、設定に `database_id` が無くても、`database_name` で D1 を名前から引く(`d1 migrations apply --remote` も同じ。workers-sdk `packages/wrangler/src/d1/utils.ts`、`wrangler@4.102.0` の変更履歴 #14275、タグ `wrangler@4.166.0`、取得 2026-11-21)。D1 が無いと、マイグレーションの段で `Couldn't find a D1 DB named …` で落ちるので、D1 は最初の配信より前に、インフラのリポジトリが宣言して作る。

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
- **実測(2026-11-20)**: 最初の app の初回配信で、アセット付きの Worker が、アカウント全体の Access の下でこのヘッダを受け取り、検証が通った。`POLICY_AUD` を壊した状態では 401 になった(インフラのリポジトリの Issue #32 の記述。画面と端末での確認は本人が行った。2 回目以降の app では、壊した AUD の確認を、配信でなく app の単体試験(JWT の異常系)で代える。本人の裁定、2026-11-21)。
- **AUD はアプリごと**: Cloudflare の文書は「Cloudflare Access assigns a unique AUD tag to each application.」と書く(Validate JWTs、取得 2026-11-21)。アカウント全体のアプリ(全 Worker を本人だけに閉じる)の AUD は 1 つで、最初の app はそれで通った。共有の Worker(共同開発者に開く)は、自前のアプリを持つので、AUD が別になる。どちらの AUD を `POLICY_AUD` に渡すかは、インフラのリポジトリがサイトごとに選んで宣言する(上のとおり、サイトは値を複写しない)。

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

build(secret なし)と deploy(secret あり)を分ける。deploy は共通の action を呼び、`d1-database` と `vars` を渡す。マイグレーションが先に走り、失敗したら配信しない(順序は action が持つので、呼び出し側が間違えられない)。`vars` の値は、インフラのリポジトリが宣言したこのリポジトリの Actions 変数で、値が空なら、action がアップロードの前に落とす。

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
          access-login-check: 'true'   # 配信のあと、ログインなしで 302 のログイン転送になることを確かめる
          vars: |
            TEAM_DOMAIN=${{ vars.TEAM_DOMAIN }}
            POLICY_AUD=${{ vars.POLICY_AUD }}
            OWNER_EMAIL=${{ vars.OWNER_EMAIL }}
```

- プレビューの job は置かない(`mode: preview` と `d1-database`・`vars`・`access-login-check` の併用は action が拒否する)。プレビューの version が本番の D1 を指すかは未確認で、指す見込みで扱う。
- token と D1 と変数: サイトの deploy token・D1・Actions 変数は、インフラのリポジトリの `bootstrap/sites.tf` が、サイト一覧の印(`d1`・`app`)から宣言する。`d1` のサイトの token には `D1 Write` が付く。`bootstrap/` は人が適用する(インフラのリポジトリの ADR-16 Amendment 2026-11-20 と 2026-11-21)。`wrangler d1 create` を手で実行して、`wrangler.jsonc` に UUID を書く手順は、もう要らない。
- 初回配信の `10007`: 新しい Worker の最初の配信が、アップロードとマイグレーションの成功のあとに `10007`(Worker does not exist)で落ちることがある(cloudflare/workers-sdk#17069)。action は、`10007` のときだけ `wrangler triggers deploy` を間隔を空けて最大 6 回(合計約 10 分)再試行する。それでも続くなら、action は落ちる。人が `wrangler triggers deploy` を実行する前に、Worker が作られているか(ダッシュボードの Workers の一覧)を見る。

## 配信の最後: About の Website 欄

配信できたら、リポジトリの About の Website 欄に URL を載せる。private は workers.dev の URL、public は Pages の URL。

```bash
gh repo edit OWNER/REPO --homepage https://<Worker 名>.<アカウントのサブドメイン>.workers.dev/
```

`gh repo edit` は、リポジトリを引数で取る。`-R` は取らない(最初の app で、この文書が `-R` を書いていて誤りだった)。管理者権限が要る。

実態: Cloudflare に置いた学習サイトは workers.dev の URL、Pages に置いた 2 つのサイトは `github.io` の URL(2026-11-20、`gh repo list` で読んだ)。この手順を決めた文面は、dotfiles の `docs/adr`・Issue・個人用の記録のリポジトリには見つからなかった。deploy の action は、配信のあとで欄を読み戻し、空なら警告する。設定そのものは、action が行わない。`GITHUB_TOKEN` は `administration` の権限を持てない(workflow の `permissions:` のキーに無い)うえ、GitHub App にその権限を足すと、ブランチ保護・ruleset・リポジトリの削除・移管にも届く(GitHub Docs, Permissions required for GitHub Apps、取得 2026-11-21)。`homepage` だけを書き換えるより狭い権限は、文書に記述が無い。だから、人かエージェントが、配信のあとに上の 1 行を打つ。

## 完了条件(最初の app の初回配信で確かめる)

1. ログインなしで URL を開くと、Access のログインへ転送される(門が閉じている)。**action が配信のたびに確かめる**(`access-login-check: 'true'`。302 で `cloudflareaccess.com` のログインへ転送されなければ、配信の job が落ちる)。
2. 本人でログインして、画面が出る(本人のブラウザで 1 回)。
3. **Worker が JWT を受け取って検証に通っている**(2026-11-20 に最初の app で確かめた。上の「認証」)。検証を壊した状態(`POLICY_AUD` を間違える)で 401 になることは、本番の Worker を壊すので、配信では確かめず、app の単体試験(JWT の異常系)で確かめる。
4. 未知の鍵への書き込みが 400 になる(app の単体試験と、本人のブラウザの 1 回)。
5. About の Website 欄に URL が載っている(action の警告が出ていない)。

## バックアップ

app の README に、段を宣言する。

| 段 | 条件 | 対応 |
|---|---|---|
| 再入力できる | 入れ直しが数分で済む状態だけ | D1 の Time Travel(Free で 7 日)だけ |
| 戻せない | 失うと戻せない記録を含む | 団体向けの先行例の ADR-86 の型(`wrangler d1 export --remote` を定時に実行し、`age` で暗号化した `*.age` だけを artifact に置く) |

`wrangler d1 export` の制約: 「A running export will block other database requests.」「Export is not supported for virtual tables」(D1 の Import and export、取得 2026-11-20)。

## 画面の作り方(ADR-1677・ADR-1685、dotfiles#1664)

この標準で作る app の画面の既定。閲覧だけの静的サイトの面も、1〜5 は同じである。**面は共通のテーマを持たない**(ADR-0000)。見た目(色・書体・尺度・部品の見た目)は、面の目的から面ごとに決め、面の `PRODUCT.md`・`DESIGN.md` に書く。共通に残すのは、床(`config/web/page-types.toml` の要件と `behavior.js`)・状態の色の意味・通信の床(第三者の配信元から読み込まない)と、工程(下の 10)である。
出発点として使える値は `config/web/variables.css`(ADR-1685)で、採る面は複写して pin する。採らない面は pin を持たない。役割と部品の見た目は、見本ページ `config/web/specimen.html` で確かめる。明暗を解いた値の一覧は、生成した `config/web/variables.json`(手で書かない)。
出発点を採る、CSS を HTML の中に持つ 1 枚だけの面(Artifact の雛形)は、HTML の中の印を付けた区間に複写し、`design-pin.toml` の `begin`・`end` でその区間を固定する(ADR-1700)。`behavior.js` は CSS の役割名に依存しない床なので、出発点を採らない面も複写して使える。

1. **CSP `style-src 'self'` の下の書き方。** インラインの `style` を使わない。位置と大きさは、CSS のクラスと SVG の属性で与える。MDN は、`element.style.<名前> =` は止められないが、`setAttribute("style", …)` と `cssText` は止められると書く(MDN, "Content-Security-Policy: style-src directive"、最終更新 2025-12-15。dotfiles#1664 の記載による)。この境目に頼らないよう、静的試験で `.style`・`setAttribute("style"`・`cssText` を禁じる。
2. **書体とスクリプト。** 第三者の配信元からは読み込まない(ADR-1677 D6。スクリプトを含める方針は ADR-1703 D4 で明確にしており、`web_assets` が検査する)。書体の選択は面ごとで、機体の書体を既定にする(出発点の `--font-sans`・`--font-mono`。明朝は `serif` に落ちる)。自前のサイトから配信する Web フォントを使うときは、`font-src 'self'` にして、第三者の配信元を開けない。外部スクリプトが必要なときは、同梱して自サイトから配信し、`sha256` を pin する。
3. **色と尺度。** 出発点を採る面は、役割(`--bg`・`--surface`・`--text`・`--text-muted`・`--border`・`--accent`・`--on-accent`・`--ok`・`--ng`・`--warn` など)だけを使い、16 進を直書きしない。出発点を採る面の主色は、`--accent`・`--on-accent`・`--accent-text`・`--focus` の値だけを上書きする。採らない面は、自分の語彙で色を決めてよいが、状態の色(成功・誤り・注意)の意味は変えず、コントラストの床(文字 4.5:1・図形 3:1)を明暗の両方で満たす。暗色は OS の設定に追従するだけで、切り替えを置かない。accent の上の文字は `--on-accent` を使い、固定の `#fff` を書かない(暗色で、淡い accent の上の白文字が 2.0〜2.5:1 になる型が、実機の審査で複数の面から見つかった)。正本のコントラストは、明暗の両方で `web_variables` が検査する。面の上書きの値は、この検査の外なので、同じ組(文字 4.5:1・図形 3:1)を自分で計算する。
4. **狭い画面の表。** 表の要素のまま、CSS で 1 行 1 枚に組み替える。各欄の名前は `data-label` 属性から `::before` で出す。ARIA の role は足さない(Adrian Roselli, "Tables, CSS Display Properties, and ARIA"、更新 2024-01-29。dotfiles#1664 の記載による)。幅の狭い画面で、列を隠さない(隠した列の情報が、スマートフォンで消える)。
5. **値の導出は build の 1 か所。** 状態語の意味づけ(要対応・済み・対象外など)、表の列の位置、時刻を分にした値は、build が `model.json` に載せる。ページは語や時刻の書式を知らない。CSS に語を複写しない。
6. **操作。** 次の 3 つは、手書きせず、共有の部品 `config/web/behavior.js`(`window.pageBehavior` の `saver`・`keyguard`・`viewstate`。ADR-1703 D2)を、複写して pin して使う(複数ファイルの面は `design-pin.toml` の `[[file]]`、1 枚だけの HTML は `begin`・`end` の区間 pin)。使い方は、動く見本 `config/web/specimen.js` が示す。
   - 保存は「保存中… → 保存済み」と出し、**書き込みの成功を確かめてから**「保存済み」にする。失敗は、その項目の横に、再保存の操作と一緒に出す(`role="status"`)。成功を先に出さない。失敗を握りつぶさない(`.catch(() => {})` を書かない)。→ `saver`
   - ショートカットのキーは、ボタン・リンク・`summary`・`select` にフォーカスがあるとき、その要素の標準のキー操作を奪わない。同じ操作を、タップできるボタンとしても置く(幅 390px で、タップだけで主なタスクが完結する)。→ `keyguard`
   - 日常の操作に確認ダイアログを使わず、選び直しで戻せる。
   - 読み込み中・失敗・空を、同じ表示にしない。失敗には再読み込みの導線を出す。エラーの出し先を、DOM に常に置く(初回の読み込みの失敗で、出し先が無く、「読み込み中…」のまま止まる型が見つかった)。→ `viewstate`
   - 操作部は 24×24 CSS px 以上(基準は 44px)。リンクを縦に並べるときは、行の高さを 24px 以上にする。
7. **画面の確認の仕方。**
   - Playwright の `page.route` と `route.fulfill({ path })` で、本番と同じ CSP のヘッダを付けて返す。
   - コンソールの CSP 違反が自分のページのものかは、`document.querySelectorAll('style, [style]')` で見分ける(ブラウザ拡張が差し込む `<style>` が混じる)。
   - 明暗・幅 390px と 1280px で、コントラスト・横スクロール・操作部の大きさを測る。**初期状態だけでなく、押下・エラー・解答後など、状態ごとに測る。** 初期状態だけの実測は、上の 3 の型を拾えなかった。
   - 撮り方は、`pr-description` スキル §5b の固定の手順(`scripts/visual_diff.py`。Chromium で撮り、Before は基準の ref から取り出し、Before | After | Diff を本人に見せる)に従う。
   - 環境の制約(2026-11-24 に実測した、Playwright MCP の Firefox): `emulateMedia` の明指定が効かず(常に暗)、`setViewportSize` は要求した幅の 0.75 倍が `innerWidth` になる(要求 520 → 390。`await` せずに投げる)。`file://` は拒否され、sandbox の内側で立てたサーバーには届かない。`visual_diff.py` は Chromium を自前で起動し、`color_scheme` と `viewport` を指定して、`route` で配信するので、この制約に当たらない(同日、sandbox の中で、明暗の描き分け・幅の一致・2 回の撮影の画素の一致を確かめた)。
8. **踏んだ落とし穴(dotfiles#1664)。** 節への移動を画面の上に留めると、狭い画面で横スクロールのバーが出た(余白を詰め、バーを隠した)。保存失敗の文が出ると、状態の欄が広がって品目名を押しつぶした(欄の幅に上限を付けた)。
9. **部品・振る舞いは、手書きする前に対応表を参照する(ADR-1703)。** `config/web/selection.toml` が、作業の種類ごとに「ブラウザ標準 → 共有部品(`config/web/`) → 外部ライブラリ」のどれを選ぶかを管理している。ダイアログは `<dialog>`、折りたたみは `<details>`、選択肢はネイティブのラジオを使用する。保存の表示・ボタン・警告・空の状態・狭い画面の表は共有部品を使う。トースト・テーマ切替・確認ダイアログは使わない(要件 F3・C8・F4)。表にない作業は、行を追加して裁定を残す。`popover` 属性は Baseline が newly なので、標準の行にはできない(2026-11-24)。

10. **ページの構造は、課題の性質から選び、宣言する(ADR-1711)。** 縦に並べるか、1 件ずつ進むか、は、好みでなく、課題の性質(軸の値)で決める。進め方は次のとおり。
    1. **課題の性質を、軸の値で書く。** 項目間の独立性・比較の要否・反復頻度・1 件あたりの情報量・取り消しの可否・件数・順序の自由度。語彙は `config/web/structures.toml` の `[[axis]]`。
    2. **構造パターンを選び、退けた案と理由を書く。** 軸の値から、`[[pattern]]` の `fits` で候補が出る。順次の裁定・質問に答える面の既定は `step`(1 件 1 画面 + 全件の概観 + 選ぶと次へ + 確認画面)。既定は強制でなく、外れるときは理由を書く(`page-structure-deviation`)。反復する本人の面では、利用者を閉じ込めない手当て(概観の常時表示・自動で次へ・キーだけで完結・確認画面から任意の件へ戻れる)を入れる。
    3. **「もの」が複数あり、一覧 → 詳細 → 操作になる面だけ、〈もの・関係・操作〉の表**を、1 面あたり 1 枚、先に書く。手順が本質の面では要らない。
    4. **構造の候補が複数あるときは、同じ架空のデータで並べた練習用の画面を、本人が触って選ぶ。** 選んだ結果と理由を、決定文書に残す。
    5. **検査する。** 宣言は `<meta name="page-structure">`・`<meta name="page-axes">`(HTML の `<head>`)に書く。リポジトリ内のすべての HTML に、`web_structures`(`repo-structure-check`)が宣言を要求する。`visual_diff.py audit` の F10 が、描画したページを宣言に突き合わせる(`step` はページ全体が縦にスクロールしない)。画面を作る・変える計画は、`## 先行例との対比` の中に `構造:` の行を書く(ADR-1711 の続きの規則。`plan-precedent-gate`)。

画面を作る・直すときの、見た目の進め方は、第三者のスキル impeccable(https://github.com/pbakaus/impeccable 、Apache-2.0。配備と使い方: [`impeccable.md`](impeccable.md)、決定: ADR-1719)の手順に従う: `shape`(計画のみ。上の構造の選択が先) → 実装 → `critique`(読み取りだけの評価) → `polish`(磨き上げ) → 撮って比べる(`visual_diff.py`)。次の 4 つは、impeccable の助言より優先する: 床(`page-types.toml` の要件と `behavior.js`)、状態の色の意味、第三者の書体・スクリプトの禁止、構造の宣言。出発点を採った面では、複写して固定した区間(`design-pin`)も優先する。面の見た目は、面ごとの `PRODUCT.md`・`DESIGN.md` が決め、`new-work` で世界を選ぶ(1 つの root に世界は 1 つなので、面ごとに root を分ける)。

`audit` の検査は、impeccable に置き換えない。最小の違反例と旧版の裁定ページを、`audit` と impeccable の検出器の両方にかけた(2026-11-25)。`keyboard`・`hover`・`input-size`・`targets`・`third-party`・`document` は、impeccable が同じ違反を落とさなかった。`overflow` は、旧版の裁定ページでは当たったが、最小の違反例では当たらなかった。`contrast` は、暗い配色の違反例を impeccable が落とさず、placeholder と `:hover` の状態は impeccable だけが測るので、併用する。`structure` ほかは、上流の 62 ルールに該当するものが見つかっていない(読んだ範囲)。逆に、impeccable の「AI の癖」の 31 ルール(`side-tab`・`nested-cards`・`gradient-text` など)には、手元に該当する検査が無いので、手元の検査との重なりでなく、足りない検査として使う。

未確認: 明るい配色の実際の画面と、スマートフォンの実機での見え方(dotfiles#1664 の記載)。上の 6 の操作の規則を、出力した HTML の検査にするかは、後続の裁定(未実装、意図あり)。

## 適用例: 登山の計画を管理する個人用リポジトリ(ダッシュボード)

そのリポジトリの README は、計画・装備・下調べの正本を markdown に置く。この標準に当てはめると次のとおり(app 本体は、そのリポジトリの Issue で追う)。

- git(markdown)が持つ: 必携品の役割と一次情報の記述、判断表の項目と基準、行程表、規制と運行の出典(題名・URL・取得日)。ビルドが `pandoc` で表を JSON にし、アセットと manifest を作る。
- D1 が持つ(manifest の鍵): `gear:<役割>`(状態語は `持っている|買う|借りる|不要|確認` の 5 語)、`go:<項目>:<前夜|当日>`(判断表の値。短い文)、`yamap:<項目>`(地図のダウンロード・登山計画の保存・登山届の提出・みまもりの通知先の**済んだ日付**)。みまもりの通知先の連絡先は、入力欄を作らない。
- 経路: `GET /`(ダッシュボード)、`GET /api/state`(D1 の値)、`PUT /api/state/<鍵>`(manifest にある鍵だけ)。いずれも `access: "owner"`。
- 導出: 山行までの残り日数、日の入りと目標時刻、取得日の古さの警告は、今日の日付から導く(宣言の欄を作らない)。
- 山の中は圏外が多いので、入力は自宅と登山口までで使う前提にする。

## 開発中の確認(sandbox の中で `wrangler dev` を使うとき)

2026-11-20 の最初の app の実装で分かったこと(インフラのリポジトリの Issue #32 の記述):

- `wrangler dev --local` は、sandbox の中では `~/.config/.wrangler` に書けず `EROFS` で落ちる。`WRANGLER_REGISTRY_PATH` と `WRANGLER_LOG_PATH` を、書ける場所(scratchpad など)に変える。
- sandbox は、Bash の呼び出しごとにネットワークを分ける。別の呼び出しで起動したサーバーには、次の呼び出しから届かない。サーバーの起動と要求を、1 本のシェルにまとめる。

## 未実測・未確認(2026-11-21)

- 実測済み(2026-11-20): アセット付きの Worker が、アカウント全体の Access の下で JWT のヘッダを受け取り、検証が通る。`POLICY_AUD` を壊した状態で 401 になる(上の「認証」)。
- 共有の Worker(自前のアプリを持つ)で、`POLICY_AUD` に自前のアプリの AUD を渡して検証が通るか(未実測。最初の共有の app で確かめる)。
- `jose` の保守表示と最終 commit(未確認。採用する app の実装前に読む)。
- プレビューの version が本番の D1 を指すか(未確認)。
- action の Cloudflare に対する実行のうち、`10007` の再試行で通る間隔(未実測。2026-11-20 に人が手で `wrangler triggers deploy` を実行して通ったのは数分後)と、`access-login-check` と `vars` の実際の配信(未実測。次に新しい Worker を足す、または最初の app の次の配信で確かめる)。偽の `wrangler`・`curl` で、呼び出しの順序・失敗時の停止・再試行の上限・警告の出力は確かめた(ADR-1660 Amendment 2026-11-21)。
- `database_id` を書かない `wrangler.jsonc` で、`wrangler deploy` が D1 の binding を名前で解決するか(`wrangler` のソースは読んだが、実行は未実測。deploy token の権限で、存在の確認が 403 になると、自動のプロビジョニングを飛ばす。`provision-bindings.ts`)。最初の app が `database_id` を消した最初の配信で確かめる。
