# gdrive-upload — ローカルのファイルを Google Drive の専用フォルダへ上げる(rclone、`drive.file`)。既存のファイルを取る読み取り側も持つ(`drive.readonly`)

エージェント(Claude Code)が、ローカルのバイナリ(PDF・画像、数 MB)を Google Drive へパス指定で上げる手段。
Drive にある既存の大きなファイルを取る読み取り側(`gdrive-read`、`drive.readonly`)も、同じ文書の「読み取り側のセットアップ」に置く。
手順書は `drive-upload` スキル(`config/agents/skills/drive-upload/SKILL.md`)、この文書は設計と初回セットアップ。

## なぜ必要か(#769)

写真付きの印刷用 PDF(約 3 MB)を Drive の既存フォルダへ上げられなかった。

- claude.ai の Google Drive コネクタ(`create_file`)は、ファイルの中身を `base64Content` / `textContent` の文字列で受け取る。
  ローカルのファイルパスを渡す口が無く、3 MB の PDF は base64 で約 4 MB の文字列になる。エージェントが一字も違えずに
  出力するのは現実的でなく、1 文字の誤りで PDF が壊れ、気づくのは Drive で開いたとき。
- Playwright でのブラウザ操作は、Google に未ログインで止まった。
- このホストにあったローカルの手段は、どちらも読み取り専用だった(既存の `rclone.conf` のリモートは `scope = drive.readonly`、
  ローカルの gdrive MCP サーバは検索と読み取りだけ)。

## 読み取り側がなぜ要るか(#969)

Drive にある 10 MB を超える既存のファイル(13 MB のバイナリ)を、エージェントが取れなかった。

- claude.ai の Drive コネクタの `download_file_content` は、10 MB を超えるファイルを拒否する。
- ブラウザ操作(claude-in-chrome)は、組織のポリシーで `drive.google.com` を開けなかった(回避しない)。
- 書き込み用の `gdrive-agent`(`drive.file`)は、rclone が作ったファイルしか読めず、既存のファイルには使えない。

本人が `drive.readonly` のリモートを作り、`rclone backend copyid` で ID を指定すると取れた(取れたサイズは、Drive のメタデータの
`fileSize` と一致した)。このとき、リモートを作る `rclone config create` の出力に、access token と refresh token が表示された。
読み取り側の手順は、下の「読み取り側のセットアップ」に、token が出ない形で書く。

## 何をするか

`rclone`(`home/modules/packages.nix`、nixpkgs の stable の 1.75.0)を入れ、`drive.file` の scope で、rclone が作る
専用フォルダにだけ書く。

- **scope は `drive.file`**: rclone が作ったファイル・フォルダだけを読み書きできる。誤って Drive の他のものを壊す経路を、
  許可の範囲で表現不可能にする(rclone docs の「With this scope rclone can read/view/modify only those files and
  folders it creates」、取得 2026-10-21)。
- **書き込み先は rclone が作る専用フォルダ**: `drive.file` では、Drive の画面で作った既存のフォルダの ID へ直接は書けない
  (rclone が作ったものしか触れない。実機での確認は未確認)。専用のフォルダ `rclone/uploads/<purpose>`
  (`docs/cloud-artifacts-and-runbooks.md` §1.2 の `<tool>/<purpose>` 規則、`<tool>` = `rclone`)に上げ、必要なら本人が Drive の
  画面で移す運用にする。Issue が求めた「既存のフォルダへ」は、この運用の変更で置き換える(本人の裁定、2026-10-21)。
- **`rclone.conf` は手書きのまま**: `programs.rclone`(home-manager の宣言的な生成)は `~/.config/rclone/rclone.conf` を生成し、
  既存の手書きのリモート(`drive.readonly`)を上書きし、rclone 自身の token の更新とも衝突する。そのため使わない。
  OAuth クライアントと token は git・home-manager の管理外で、ホストローカルに置く(`gas-clasp-ops` の
  `~/.config/clasp/client_secret.json` と同じ扱い)。
- **読み戻してから報告する**: 外部システムへの書き込みは、読み戻した状態だけを報告する(共有 `AGENTS.md`)。上げたあと
  `rclone lsf --format pi` と `rclone md5sum` で、サイズとハッシュを読み戻す。

## 初回セットアップ(本人の作業。1 手順ずつ、完了確認を満たしてから次へ)

前提: `hms` で `rclone` が入っている。`rclone version` が `rclone v1.75.0` を返す。

導出値(`docs/personal-cloud-projects.md` の汎用規則、適用例3):

| 項目 | 値 |
|---|---|
| ツール名 `<tool>` | `rclone` |
| プロジェクト ID/名 | `rclone-<github-username>` |
| 同意画面 | External / アプリ名 `rclone` / 連絡先は所有アカウントのメールアドレス |
| 公開ステータス | Testing のまま、自分自身を test user に登録する(人が都度実行するだけで、無人・定期実行ではない) |
| OAuth クライアント | Desktop app / クライアント名 `rclone` |
| 有効化 API | Google Drive API(`drive.googleapis.com`)のみ |
| rclone のリモート名 | `gdrive-agent` |

### 1. プロジェクトを作り、API を有効化する(端末)

```
gcloud projects create rclone-<github-username>
gcloud services enable drive.googleapis.com --project=rclone-<github-username>
```

- 完了確認: `gcloud services list --enabled --project=rclone-<github-username>` に `drive.googleapis.com` が出る。
- 根拠: `docs/personal-cloud-projects.md` の「gcloud で自動化できる範囲」。
- `gcloud` が未認証なら、先に `gcloud auth login`(ブラウザ)を行う。`gcloud` が無いホストでは、下の 2 以降と同じコンソール
  (https://console.developers.google.com/)で、プロジェクトの作成と API の有効化を行う(画面の項目名は、rclone 公式の手順の
  「Select a project or create a new project」「ENABLE APIS AND SERVICES」)。

### 2. OAuth 同意画面を設定する(ブラウザ、コンソール専用)

https://console.developers.google.com/ で、手順 1 のプロジェクトを選ぶ。

- 左のパネルの「Credentials」を開く(「Create credentials」ではない。ウィザードが開く)。「CONFIGURE CONSENT SCREEN」が出たら押す。
- 同意画面の種類(audience)は「External」、アプリ名は `rclone`、サポートメールと開発者連絡先は所有アカウントのメールアドレス。ロゴは空欄。
- 追加するスコープ: `https://www.googleapis.com/auth/drive.file`。**rclone 公式の手順は、同意画面に `docs`・`drive`・
  `drive.metadata.readonly` を足す形で、`drive.file` を足す画面の項目名は公式文書に無いので未確認**。見つからない場合は、
  スコープを画面から足さずに先へ進み、手順 5 の認可で `drive.file` を要求する(同意画面に載っていないスコープの扱いは未確認)。
- 「Audience」を開き、下へスクロールして「+Add users」で、自分のアカウントを test user に追加して保存する。
  公開ステータスは **Testing のまま**(「PUBLISH APP」は押さない)。refresh token は 7 日で失効するので、失効したら手順 5 をやり直す。
- 完了確認: 「Audience」の画面に、自分のアカウントが test user として出る。
- 根拠: rclone, "Making your own client ID" https://rclone.org/drive/#making-your-own-client-id(取得 2026-10-21)。

### 3. OAuth クライアントを作る(ブラウザ)

同じコンソールで、左のパネルの「Overview」を開き、「Create OAuth client」を押す。

- アプリケーションの種類は「Desktop app」、名前は `rclone`(既定の「Desktop client 1」にしない)。
- 作成後に出るクライアント ID とクライアントシークレットを控える。**クライアントシークレットはチャットや Issue に貼らない**
  (手順 4 で端末の対話入力にだけ使う)。
- 完了確認: 「Credentials」の一覧に、名前 `rclone`・種類 Desktop のクライアントが出る。

### 4. rclone のリモートを作る(端末、対話)

```
rclone config
```

- `n`(New remote)→ 名前 `gdrive-agent` → ストレージの種類は `drive`(Google Drive)。
- `client_id` と `client_secret` に、手順 3 の値を入れる(端末の対話入力にだけ使う)。
- `scope` は **`drive.file`**(`drive.file` を含む選択肢を、画面に出る表記で選ぶ。選択肢の文言と番号は版で違うはずで、
  1.75.0 での表記は未確認。`drive`(フルスコープ)や `drive.readonly` を選ばない)。
- `root_folder_id` は空のまま(手順 6 で固定する)。「Use auto config?」は `y`(rclone がローカルのウェブサーバを立てて認可を受ける)。
- 完了確認: 手順 5 の認可のあと、`rclone listremotes` に `gdrive-agent:` が出る。

### 5. ブラウザで認可する

手順 4 の「Use auto config?」に `y` と答えると、ブラウザが開く。手順 2 で test user にしたアカウントを選び、`rclone` が
Drive のファイルを扱うことを許可する。

- 完了確認: 端末に `Success!` が出て、`rclone config` が先へ進む。`rclone lsd gdrive-agent:` が(空でも)エラーなく終わる。
- 失敗の典型: test user に入っていないアカウントを選ぶと、アクセスがブロックされる。

### 6. 専用フォルダを作り、書き込み先を固定する

```
rclone mkdir gdrive-agent:rclone/uploads
rclone lsf --dirs-only gdrive-agent:rclone
```

- 完了確認: `uploads/` が出る。
- `drive.file` では、rclone が作ったフォルダだけが見える。`root_folder_id` を `rclone/uploads` の ID に固定すると、リモートの根がそこになり、
  どのパスで呼んでも、そのフォルダの外へ書けなくなる。ID は `rclone lsf --format pi --dirs-only gdrive-agent:rclone`(`p` = パス、`i` = ID)
  で読み、`rclone config` の編集で `root_folder_id` に入れる。固定したあとは、フォルダ名を含めずに `gdrive-agent:<purpose>/` と書く。
- 完了確認(固定後): `rclone lsd gdrive-agent:` が、`uploads` の中身(空でもよい)を返す。

## 日常の使い方

`drive-upload` スキルに従う。要点:

```
rclone copy <ローカルのファイル> gdrive-agent:<purpose>/
rclone lsf --format pi gdrive-agent:<purpose>/       # p=パス i=ID を読み戻す
rclone md5sum gdrive-agent:<purpose>/                # ローカルの md5sum と突き合わせる
```

既存のファイルを取るとき(読み取り側):

```
rclone backend copyid gdrive-read: <ファイルの ID> <保存先のディレクトリ>/
```

## 読み取り側のセットアップ(本人の作業。1 手順ずつ、完了確認を満たしてから次へ)

前提: 上の手順 1〜3 が済んでいる(プロジェクトと OAuth クライアント `rclone`)。書き込み用の `gdrive-agent` は、そのまま残す
(`drive.file` のままで、読み取り用とは別のリモート)。

| 項目 | 値 |
|---|---|
| rclone のリモート名 | `gdrive-read` |
| scope | `drive.readonly`(既存のファイルの読み取り専用。書き込めない) |
| OAuth クライアント | 上の手順 3 と同じもの(新しく作らない) |

### R1. `gdrive-read` を作る(端末、対話。エージェントのセッションの外のターミナルで)

目的: Drive にある既存のファイルを、ID を指定して取れるようにする。

遷移経路: 本人の端末で、エージェントのセッションを開いていないターミナルを使う(`!` 付きでセッションの中から実行すると、
画面の出力が会話に入る。最後の確認画面に token が出るかは未確認だが、`config create` の出力には出た)。

```
rclone config
```

- `n`(New remote)→ 名前 `gdrive-read` → ストレージの種類は `drive`(Google Drive)。
- `client_id` と `client_secret` に、手順 3 の値を入れる(端末の対話入力にだけ使う。チャットや Issue に貼らない)。共有の client_id
  を使うと `This remote uses rclone's shared Google Drive client_id, which is being retired` の警告が出る(2026 年中に止まる)ので、
  自分のクライアントを使う。
- `scope` は **`drive.readonly`**(`drive.readonly` を含む選択肢を、画面に出る表記で選ぶ。選択肢の文言と番号は版で違うはずで、
  1.75.0 での表記は未確認。`drive.file` や `drive`(フルスコープ)を選ばない)。
- `root_folder_id` は空のまま。「Use auto config?」は `y`(rclone がローカルのウェブサーバを立てて認可を受ける)。
- ブラウザが開いたら、test user に入れたアカウント(手順 2)を選び、`rclone` が Drive のファイルを読むことを許可する。
  同意画面に `drive.readonly` を載せる必要があるかは未確認(「access blocked」が出たら、止まって項目名を確かめる)。
- 最後の「Keep this "gdrive-read" remote?」の画面には、設定が出る(token を含むかは未確認)。**画面の写しをチャットに貼らない**。
  `y` で確定する。
- 完了確認: `rclone listremotes` に `gdrive-read:` が出る。
- 根拠: rclone, "Google drive" https://rclone.org/drive/(取得 2026-10-21)の scope の選択肢と認可の手順。画面の項目名は、手順 4・5 と同じ。

自分のクライアントをまだ作っていないなど、当面の簡便形として、共有の client_id で非対話に作ることもできる(上の警告が出る):

```
rclone config create gdrive-read drive scope=drive.readonly --no-output
```

`--no-output`(`rclone config create --help` の「Don't provide any output」、手元の 1.75.2 で、出力が出ないことを `alias` のリモートで
確かめた。Drive の OAuth 認可と合わせた挙動は未確認)は、作ったリモートの設定の表示を止める。付けないと、token が標準出力へ出る(#969 で起きた)。
ブラウザの認可は同じく本人が行う。

### R2. 取れることを確かめる(端末)

目的: ID を指定して、既存のファイルを取れることと、サイズが合うことを確かめる。

```
rclone backend copyid gdrive-read: <ファイルの ID> <保存先のディレクトリ>/
```

- ファイルの ID は、Drive の URL `https://drive.google.com/file/d/<ID>/view` の `<ID>` を使う。
- 保存先は `/` で終える(終えないと、最後の要素がファイル名になる)。
- 完了確認: 保存先のファイルのサイズが、Drive のメタデータの `fileSize`(Drive のファイルの詳細、またはコネクタの `get_file_metadata`)と一致する。
- 根拠: `rclone backend help drive` の `copyid`(rclone 1.75.2、取得 2026-10-27)、および #969 で実際に取れた記録。

## 既存手段と代替

- 既存手段: 採用 — rclone(https://rclone.org/drive/、nixpkgs の stable で 1.75.0)。
- 対抗馬: Drive API を直接呼ぶ小スクリプト、`upload(localPath, parentFolderId)` を持つ MCP サーバ(dylancaponi の fork)。
  後者は `drive` のフルスコープへ昇格し、保守の実績が少ない。どちらも書き込み範囲の表現不可能性で rclone の `drive.file` + 専用フォルダに及ばない。
- 外した候補: `google-drive-ocamlfuse` と `gdrive3` — 感触で外した(nixpkgs の版遅れと upstream の保守停止を一次情報で見ただけで、動かして比べてはいない)。
- 使えない候補: サービスアカウント。2025-04-15 以降に作る新規のサービスアカウントは Drive のアイテムを所有できず、個人の Gmail では代理(impersonation)もできない。
- 軸: 表現不可能 — `drive.file` と専用フォルダで、書き先の外への書き込みを表現不可能にする。
- 戻せないもの(人手): GCP プロジェクトの作成、同意画面、OAuth クライアントの作成、rclone のブラウザ認可。

## 出典

- rclone, "Google drive" https://rclone.org/drive/(取得 2026-10-21): 自前の client ID の作り方、`drive.file` の意味、`root_folder_id`、
  共有の client ID が 2026 年中に止まること、Testing の grant が 1 週間で失効すること。
- `docs/personal-cloud-projects.md`: プロジェクト・同意画面・クライアントの導出規則(適用例3)。
- `docs/cloud-artifacts-and-runbooks.md` §1.2: Drive のフォルダの命名(`<tool>/<purpose>`)。
- rclone, `rclone backend help drive`(1.75.2、取得 2026-10-27): `copyid` の使い方(ID とパスの組、パスが `/` で終わるときの扱い)。
- rclone, `rclone config create --help`(1.75.2、取得 2026-10-27): `--no-output`。
