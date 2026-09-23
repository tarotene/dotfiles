# 個人ツールの自前クラウドプロジェクト命名・設定ポリシー

個人開発中の CLI ツールが Google API 等に OAuth でアクセスするとき、
gmailctl や rclone と同じ「ユーザー自前クラウドプロジェクト」モデルを
採る場合がある。このモデルではツールを認可するたびにプロジェクト名・
OAuth 同意画面のアプリ名・公開ステータス・OAuth クライアント名を
ユーザー自身が決めなければならない。これらの値は実害の大きさに対して
決定コストが割に合わない命名・設定判断であり、都度考えるのは無駄である。
かつ判断を誤ると refresh token が 7 日で失効する罠(下記出典)にはまる。
今後も個人ツールが自前クラウドプロジェクトを要求する場面は繰り返し
発生するため、都度の判断ではなく決定論的な導出規則としてここに定める
(#197)。

## 汎用規則

| 決定事項 | 規則 |
|---|---|
| プロジェクト戦略 | 1 ツール = 1 GCP プロジェクト(同意画面の設定粒度がプロジェクト単位のため、共有すると外部連携ツールが増えるほど同意画面の意味が薄まる) |
| プロジェクト ID/名 | `<tool>-<github-username>`。Google 公式制約(6–30 文字・小文字/数字/ハイフン・先頭英字・グローバル一意・作成後変更不可)に適合。衝突時は `-2` 等の連番を後置 |
| 所有アカウント | そのツールが読み書きする対象アカウント自身(認可するアカウントと同一) |
| OAuth 同意画面 | User type = External / アプリ名 = `<tool>` / サポートメール・開発者連絡先 = 所有アカウントのメールアドレス / ロゴは空欄 |
| 公開ステータス | 「Testing か Production か」節の基準で分岐する |
| OAuth クライアント | 種別は用途に応じた最小権限(CLI/デスクトップツールなら Desktop app)/ クライアント名 = `<tool>`(既定の "Desktop client 1" 等を使わない) |
| 有効化 API | そのツールが実際に呼ぶ API のみ |

個々のツールはこの規則から値を機械的に導出するだけでよく、命名や設定を
都度考える必要はない。

### Testing か Production か

**そのツールが無人・定期実行(cron/systemd timer 等)されるかどうかで
分岐する。**

- **無人・定期実行される**(例: 未読メールを毎日自動削除するジョブ):
  Production(In production)にする。Testing のままだと refresh token が
  7 日で失効し、ジョブが気づかれないまま止まる(サイレント障害)。
  トークン失効そのものは無害でも、検知が遅れることが実害になる。
- **人間が都度手動で実行するだけ**(例: `clasp run-function` を必要な時に
  叩くだけの CLI): Testing のままでよい。External / Testing で
  自分自身を test user に登録すれば十分。7 日で失効しても、次に使う時に
  再ログインし直すだけで実害がない(無人ジョブのような「気づかれない
  停止」が起きないため)。

**Production への切り替え自体にコストがある点に注意**: Google は User
type = External の Production アプリに対し、Application home page・
Privacy policy へのリンク、および Search Console で所有証明した
Authorized domain を要求する(未審査でも必須。下記出典)。ドメインを
持たないツールでは、この要件を満たすためだけにドメイン取得・Search
Console 検証という工数が発生する。**「無人実行だから Production が要る」
という実害が無い限り、この工数を払う理由はない** — 上記の分岐基準は
このコストとのトレードオフである。

「1 ツール」の粒度は、そのツール自身が下流の複数スクリプトを束ねる共有
基盤である場合、下流スクリプトごとではなく共有基盤自体を 1 つの `<tool>`
として扱う(適用例2参照)。

## 適用例(第 1 適用例: 自作の Gmail/Calendar 連携 CLI)

| 項目 | 値 |
|---|---|
| ツール名 | `<tool>` = そのツールのバイナリ/コマンド名 |
| プロジェクト ID/名 | `<tool>-<github-username>` |
| 同意画面 | External / アプリ名 `<tool>` / 連絡先は所有アカウントのメールアドレス |
| 公開ステータス | 無人・定期実行するサブコマンドがある場合のみ Production。「Testing か Production か」節参照 |
| OAuth クライアント | Desktop app / クライアント名 `<tool>` |
| 有効化 API | 実際に呼ぶ Google API のみ(例: Gmail API, Google Calendar API) |

反映先: 当該ツールの `auth` サブコマンドの GCP セットアップガイダンスに、
上記の導出規則(プレースホルダ込みの推奨命名)を既定値として埋め込む。

## 適用例2: 個人用 GAS 自動化スクリプト群(gas-clasp-ops)

`gas-clasp-ops` スキル(`config/claude/skills/gas-clasp-ops/SKILL.md`、
ADR-0030)は、個々の GAS スクリプトごとに GCP プロジェクトを作るのではなく、
**個人で 1 つの Cloud プロジェクト + OAuth クライアントを複数の GAS
スクリプトで使い回す**設計を採る(スクリプトごとに同意画面・クライアントを
作る往復コストが釣り合わないため)。この場合の `<tool>` は個々の GAS
スクリプト(例: 特定リポジトリの `tools/*.gs`)ではなく、それらを束ねる
共有基盤である `gas-clasp-ops` 自身とする。

| 項目 | 値 |
|---|---|
| ツール名 | `<tool>` = `gas-clasp-ops` (下流の個々の GAS スクリプト名ではない) |
| プロジェクト ID/名 | `gas-clasp-ops-<github-username>` |
| 同意画面 | External / アプリ名 `gas-clasp-ops` / 連絡先は所有アカウントのメールアドレス |
| 公開ステータス | **Testing のまま**、自分自身を test user に登録する。`clasp run-function` は常に人間が都度実行するものであり無人・定期実行ではないため、「Testing か Production か」節の基準で Production 化の工数(ドメイン所有証明等)を払う理由がない。7 日で refresh token が失効したら `clasp login --user run` をやり直す |
| OAuth クライアント | Desktop app / クライアント名 `gas-clasp-ops` |
| 有効化 API | Apps Script API (`script.googleapis.com`) のみ |

反映先: `gas-clasp-ops` スキルの初回セットアップ手順に、上記の導出値を
既定値として埋め込む(2026-09-23、ADR-0030 Amendment 参照)。

## gcloud で自動化できる範囲

同意画面の設定・OAuth クライアント作成はコンソール専用(公開 API が
存在しない)だが、プロジェクト作成と API 有効化は CLI 化できる:

```bash
gcloud projects create <tool>-<github-username>
gcloud services enable <api-1> <api-2> --project=<tool>-<github-username>
```

## 出典

- Google 公式「Using OAuth 2.0 to Access Google APIs」§Refresh token
  expiration: https://developers.google.com/identity/protocols/oauth2#expiration
  (取得 2026-09-19) — External + Testing は refresh token が 7 日で失効
  すると明記(name/email/profile のみのスコープの場合を除く)。
- Google 公式「Creating and managing projects」:
  https://docs.cloud.google.com/resource-manager/docs/creating-managing-projects
  (取得 2026-09-19) — プロジェクト ID の文字数・文字種・一意性・不変性の
  制約。
- gmailctl README: https://github.com/mbrt/gmailctl (取得 2026-09-19) —
  個人利用ではユーザー自前プロジェクト・即 Publish を推奨。
- rclone 公式ドキュメント "Making your own client_id":
  https://rclone.org/drive/#making-your-own-client-id (取得 2026-09-19) —
  同様に自前プロジェクト・Testing 状態の週次失効への言及。
- Google 公式「Manage OAuth App Branding」:
  https://support.google.com/cloud/answer/15549049?hl=en (取得 2026-09-23) —
  「These links are required for all external production apps」と明記。
  External + Production への切り替えには homepage・privacy policy の
  リンクと、それが乗る Authorized domain(所有証明必須)が要ることの根拠。
  未審査(verification 未申請)でもこの要件は免除されない。
