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
| OAuth 同意画面 | User type = External / アプリ名 = `<tool>` / サポートメール・開発者連絡先 = 所有アカウントのメールアドレス / ロゴ・ドメインは空欄 |
| 公開ステータス | プロジェクト作成直後に「Publish app」で In production へ切り替える(未審査のままでよい。個人利用は Google 審査対象外)。**Testing のまま test user 登録で運用しない** — refresh token が 7 日で失効する |
| OAuth クライアント | 種別は用途に応じた最小権限(CLI/デスクトップツールなら Desktop app)/ クライアント名 = `<tool>`(既定の "Desktop client 1" 等を使わない) |
| 有効化 API | そのツールが実際に呼ぶ API のみ |

個々のツールはこの規則から値を機械的に導出するだけでよく、命名や設定を
都度考える必要はない。

## 適用例(第 1 適用例: 自作の Gmail/Calendar 連携 CLI)

| 項目 | 値 |
|---|---|
| ツール名 | `<tool>` = そのツールのバイナリ/コマンド名 |
| プロジェクト ID/名 | `<tool>-<github-username>` |
| 同意画面 | External / アプリ名 `<tool>` / 連絡先は所有アカウントのメールアドレス |
| 公開ステータス | 作成直後に Publish app → In production |
| OAuth クライアント | Desktop app / クライアント名 `<tool>` |
| 有効化 API | 実際に呼ぶ Google API のみ(例: Gmail API, Google Calendar API) |

反映先: 当該ツールの `auth` サブコマンドの GCP セットアップガイダンスに、
上記の導出規則(プレースホルダ込みの推奨命名)を既定値として埋め込む。

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
