---
name: gas-clasp-ops
description: Google Apps Script (GAS) プロジェクトを clasp CLI で操作するときの手順(初回セットアップ・ログイン・push・run-function・スクリプト側の規約)。GAS・Google Apps Script・clasp・claspでGASを操作・GASをCLIから叩く・FormApp・SpreadsheetApp・Apps Script API・スクリプトのURLを取得、といった文脈で使う。clasp login, clasp push, clasp run-function, Apps Script CLI, headless GAS execution、といった英語の文脈でも使う。
---

Google Apps Script は既定では script.google.com への手動コピペ実行が前提で、
実行結果(生成した Form/Spreadsheet の URL など)はブラウザの実行ログにしか
出ない。ログを閉じると結果を拾い直せず、スクリプトの再実行が必要になる。
また `FormApp.create` のように「毎回新規作成する」呼び出しを冪等化せずに
置いておくと、再実行のたびに Drive 上のオブジェクトが増える。

これを避けるため、GAS プロジェクトは Google 公式 CLI である `clasp`
(<https://github.com/google/clasp>)で操作する。`clasp push` でコードを
同期し、`clasp run-function` で関数をリモート実行して戻り値を端末で直接
受け取れる。

## 1. 正本の置き方(前提)

GAS のコード + `.clasp.json` は、それを使うプロジェクトのリポジトリに
同居させる(分散配置)。GAS 専用の集約 monorepo は作らない — スクリプトは
たいていそのリポジトリのドメイン知識(文面・設問など)と密結合しており、
コードだけを別リポに切り出すと二重正本の同期コストが増える。

```
<consuming-repo>/
  tools/               # または任意のディレクトリ名
    .clasp.json        # { "scriptId": "...", "rootDir": "." }
    appsscript.json     # manifest(timeZone・oauthScopes・executionApi 等)
    *.gs
```

`.clasp.json` は git 管理してよい(scriptId は秘密ではない)。実行結果として
生成される `.clasprc.json`(認証トークン)だけは対象リポジトリの
`.gitignore` に必ず加える(2節参照)。

## 2. 初回セットアップ(ホストごとに一度)

clasp v3 の `run-function` はスクリプトと同じ Google Cloud プロジェクトに
紐付いた OAuth クライアントを要求する(公式ドキュメント
<https://github.com/google/clasp/blob/master/docs/run.md> に明記)。個人で
繰り返し使う前提なので、Cloud プロジェクトと OAuth クライアントは
**GAS プロジェクトごとではなく個人で 1 つ**を使い回す。

命名・設定値は都度考えず、`docs/personal-cloud-projects.md`(個人ツールの
自前クラウドプロジェクト命名・設定ポリシー)の「適用例2: 個人用 GAS 自動化
スクリプト群(gas-clasp-ops)」から機械的に導出する。この場合の `<tool>` は
個々の GAS スクリプトではなく共有基盤である `gas-clasp-ops` 自身。

1. Google Cloud Console で個人共通のプロジェクトを新規作成する(既にあれば
   再利用する)。プロジェクト ID/名は `gas-clasp-ops-<github-username>`。
2. OAuth 同意画面を設定する。User type = External、アプリ名 =
   `gas-clasp-ops`、サポートメール・開発者連絡先は所有アカウントのメール
   アドレス。**Testing のまま、自分自身を test user に登録する。**
   `clasp run-function` は常に人間が都度実行するものであり無人・定期
   実行ではないため、Production 化(ドメイン所有証明・homepage・
   privacy policy の用意が必須。`docs/personal-cloud-projects.md`
   「Testing か Production か」節参照)の工数を払う理由がない。
   refresh token は 7 日で失効するが、次に使う時に手順3の
   `clasp login --user run` をやり直すだけで実害はない。
3. OAuth クライアント ID を作成する。種別は **Desktop app**。クライアント名は
   `gas-clasp-ops`(既定の "Desktop client 1" 等を使わない)。ダウンロードした
   JSON を `~/.config/clasp/client_secret.json` として保存する(git 管理外・
   home-manager 管理外。ホストローカルのファイルのまま扱う)。
4. その Cloud プロジェクトで Apps Script API(`script.googleapis.com`)を
   有効化する。加えて <https://script.google.com/home/usersettings> でも
   Apps Script API を ON にする(個人アカウント側のトグルで、プロジェクト側の
   有効化とは別)。
5. 操作したい既存/新規の Apps Script プロジェクトを開き、
   プロジェクトの設定(Project Settings)の「Google Cloud Platform (GCP) Project」で
   手順1のプロジェクト **番号**(Project Number。Project ID ではない)を設定する。
   これでそのスクリプトが個人共通の Cloud プロジェクトに紐付く。

## 3. ログイン(2段)

```sh
# 通常ログイン(push/pull 等。~/.clasprc.json に保存される)
clasp login

# run-function 用の名前付きプロファイル(手順2で作った Desktop app クライアントを使う)
clasp login --user run --use-project-scopes --creds ~/.config/clasp/client_secret.json
```

`--creds` を指定したログインは `.clasprc.json` を**カレントディレクトリ**に
書き出す(公式 README に明記)。GAS プロジェクトのディレクトリで実行した
場合は、そのリポジトリの `.gitignore` に `.clasprc.json` を必ず加える。

## 4. 日常操作

```sh
cd <gas-project-dir>        # .clasp.json のあるディレクトリ
clasp push                  # ローカルのコードを Apps Script プロジェクトへ反映
clasp run-function <fn> --user run   # 関数をリモート実行し、戻り値を端末で受け取る
clasp tail-logs              # console.log の出力を追う(Cloud Logging 経由)
```

## 5. スクリプト側の規約

- **実行結果は戻り値で返す**。`Logger.log` は `clasp run-function` の
  レスポンスに乗らない。ログとして端末に出したい場合は `console.log` を使う
  (`clasp tail-logs` が拾うのも console 系)。
- Drive 上にオブジェクトを作成する関数(`FormApp.create` /
  `SpreadsheetApp.create` 等)は、再実行で増殖しないよう
  `PropertiesService.getScriptProperties()` に作成済み ID を保存し、
  次回以降はそれを開くだけにする(冪等化)。
- `appsscript.json` の `oauthScopes` を明示すると、GAS はその範囲でしか
  権限を要求しない。スコープ不足は実行時の権限エラーで判明するので、
  エラーメッセージに従って追加していけばよい。

## 6. 複数 GAS プロジェクトを扱うとき

`.clasp.json` はディレクトリ基準で解決される(`rootDir` もそのディレクトリを
起点にする)。1 つのリポジトリに複数の GAS プロジェクトがある場合は、
プロジェクトごとにサブディレクトリを分け、それぞれに `.clasp.json` を置けば
競合しない。dev/prod のような複数デプロイ先を 1 ディレクトリで切り替えたい
場合は `--project <file>` で `.clasp.json` 以外の設定ファイルを指定できる
(公式 README 記載の想定用途)。
