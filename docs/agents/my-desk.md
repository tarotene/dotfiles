# my-desk — その日にさばく作業を、朝の机 1 か所に集める入口

`/my-desk` は、本人が習慣として手で打つ入口の 1 つ(もう 1 つは片付けの `/my-cleanup`、[my-cleanup.md](my-cleanup.md))。
旧名 `daily-desk` はこのスキルへの転送口で、通知役の hook `daily-desk-reminder` は実行名のまま変えない。
名前の付け方は[ツールの命名と配置](tooling-conventions.md)の「入口と知識の 2 層」にある。

スキル: `config/agents/skills/my-desk/SKILL.md`
hook: `crates/daily-desk-reminder`(SessionStart、1 日 1 回の注入)
手順の正本: 機体ごとに別の private なリポジトリの `desk.md`。個人の機体は person-state リポジトリの
`state/reviews/daily/desk.md`、会社の機体は会社側のリポジトリ(下の「ハブは机専用の目印で解決する」)
(各リポジトリ側の ADR。出典は private リポジトリ側のため、ここでは決定の存在だけを参照する)

## 動機

その日にさばくべき作業が、別々の発火装置に散っていた。

- 返信が必要なメールの下書きと日程回答は、発火装置がなく、依頼文があるときだけ動く。
- 軽い人手タスクは、`issue-index` が SessionStart で索引として注入する(`docs/agents/issue-index.md`)。
- 未消費の PR の裁定は、夜のクラウドの通知に 1 件だけ載っていた。
- 受信箱は、端末のバッジが起点だった。
- 期限は、カレンダーの終日予定が起点だった。

さばく作業はどれも、手元の資格情報を必要とし、クラウドの定期実行からは届かない。
そこで、起点を「その日最初のセッション」に 1 つ置く。クラウドの定期実行は、提案だけを届ける。

## なぜ薄いスキルにしたか

手順には、実在の差出人、看板の番号、リポジトリ名が入る。これらは public な dotfiles に置けない
(ADR-0034)。そこで、手順の正本は person-state リポジトリに置き、スキルは「ハブを解決して
`desk.md` を読んで従う」だけにする。クラウドの定期実行で採った形
(`docs/adr/519-routines-declaration-in-repo.md` D2、薄いポインタと home リポジトリの正本)と同じ形を、
手元のスキルに適用した。手順をスキルと `desk.md` の両方に書くと、複写と同期の二重管理になる。

## ハブは机専用の目印で解決する

最初は、`performance-hub`(`crates/hub-resolve`)をそのまま流用し、同じリポジトリを指すマーカーを
増やさなかった(`docs/agents/venue-search.md` と同じ判断)。会社の機体にも机を通すとき、この判断を改めた。
`performance-hub` は演奏本番・会場探しのスキルも読む個人データの目印で、会社の機体に置くと、
個人の段が会社の機体で動く状態を作れてしまう。そこで、机専用の目印 `desk-hub` を追加し、機体ごとに
その机の正本を持つリポジトリを指す。

`desk-hub`(`crates/hub-resolve` の bin。`resolve_desk` が解決する)の規則:

1. 環境変数 `DESK_HUB`、なければ `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/desk-hub` の 1 行目が、
   机のハブ(person-state リポジトリの絶対パス)。
2. 机の正本の場所は、個人と会社で同じ `state/reviews/daily/desk.md`(ハブの中の固定のパス)。
   個人と会社は、Slack の有無と GitHub アカウントの共用を除いて、対称な対にする(同じレイアウトの
   person-state リポジトリを、側ごとに持つ)ので、場所を上書きする仕組みは持たない。正本がなければ失敗する。
3. `desk-hub` の目印がなく、`performance-hub` があれば、従来どおりそのハブが机を持つ(個人の機体は
   何も変えずに動く)。
4. どちらもなければ、明示のメッセージを出して非 0 で終わる。
5. 使い方: `desk-hub`(引数なし)は机のハブの絶対パスを、末尾改行なしで出す。

会社の機体は、`performance-hub` を置かず、`desk-hub` だけで会社側の person-state リポジトリを指す。
会社の机の中身(どの段をどの部品で行うか)は、会社側のリポジトリの ADR が決める。目印はどの機体でも
手で置く(home-manager は宣言しない。`docs/operations.md` の目印の表)。

## GitHub の通知に触れてよい側かの関門(`desk-github-gate`)

机の段 3a(GitHub 通知の消化)は、GitHub のアカウントが個人と会社で共用なので、どちらの機体で見ているかが
確かめられたときだけ、エージェントが通知に触れてよい(ADR-523)。この関門は、かつて個人側のリポジトリの
スクリプトにあった。中身は `dotfiles-identity` と通知ツールを呼ぶだけで個人側のデータに依らないので、
公開側の道具 `desk-github-gate`(`crates/desk-github-gate`)にして、個人と会社の両方の机が同じものを使う(#1575、ADR-1573)。
通知ツールは非公開のリポジトリの道具なので、名前を公開のソースにも文書にも書かない(ADR-0034)。

`desk-github-gate`(引数なし)は、次の 3 つがすべて成り立ったときだけ終了コード 0 で、`<通知ツール> scope --json` の報告を
そのまま標準出力に出す。1 つでも崩れたら終了コード 1 で、理由を標準エラーに出す(その場合、通知には 1 件も触れない)。

1. この機体の identity と、ログイン中の Claude アカウントが一致する(`dotfiles-identity verify`。`docs/dotfiles-identity.md`)。
2. 通知ツールの `[github]` の範囲が設定されていて、その側が 1. の identity と同じ。
3. 通知ツールのキャッシュが現在の範囲で作られている。ない、または古ければ `<通知ツール> sync` で作り直し、2. と 3. をもう一度確かめる。

報告(JSON)は `{"side","stale","synced","other_side":[…],"unassigned":[…]}`。`unassigned` は、会社側にも個人側にも表のない
持ち主で、机は、持ち主ごとにどちら側かを本人に聞く。`stale` がない報告は新しいとみなさない(`false` でなければ失敗)。
同期の経過は標準エラーに出し、報告(標準出力)に混ぜない。外のコマンドの呼び出しは `Runner` に分けてあり、単体テストは偽の
コマンドで、`tests/cli.rs` は偽の `dotfiles-identity` と偽の通知ツール(一時ディレクトリの PATH)で実バイナリを動かす。

**通知ツールの名前は、`desk-hub` と同じ間接参照で解く**(`resolve_tool`): 環境変数 `DESK_NOTIFY_TOOL`(セッション限定の上書き)、なければ
`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/desk-notify-tool` の 1 行目。値は空白を含まない 1 語(コマンド名か絶対パス)で、
どちらもなければ「通知ツールの名前が未設定」で止まる(通知には触れない)。目印は機体ごとに手で置く(`docs/operations.md` の目印の表)。
呼び出しの形は `<通知ツール> scope --json` と `<通知ツール> sync` の 2 つだけである。

個人側のリポジトリの机の手順は、このツールを呼ぶ形に直す(旧スクリプトとそのテストは、そのリポジトリの側で消す)。直す前の機体では、
旧スクリプトがそのまま動く(この道具の追加は、既存の呼び出しを変えない)。

## なぜ hook を `issue-index` に同居させなかったか

`issue-index` は GitHub の Search API に依存する。失敗すると机の注入も一緒に失敗する。机の判定は
日付ファイルだけでネットワークが不要なので、障害を独立させるために別の hook にした。

## daily-desk-reminder の発火条件

- イベントは SessionStart の `startup` だけ。`resume` と `compact` では出さない(印がない限り、新しい
  セッションごとに 1 回出る)。
- 机の目印(`desk-hub`、なければ `performance-hub`)が未設定の機体では何も出さない。会社の機体は
  `desk-hub` を置くと、会社側の正本を読んで注入が出る。ハブはあるが机の正本が読めないときは、
  stderr に 1 行だけ出す。
- 日付は JST で、`TZ` や `date(1)` に依存せず unix 秒から求める(`crates/daily-desk-reminder`)。
- 注入文は、本人が望むときにだけ `my-desk` を実行するよう促す。自動では走らせない。

## 実施済みの印

`~/.local/state/claude/desk/<JST の日付>` の空ファイル 1 つだけを新設する。机の完了を示す台帳は
作らない。完了は、メールの下書き、受信箱の操作ログ、PR のマージ状態、カレンダーから読み戻す。
印を作り忘れた場合の壊れ方は、同じ日にもう一度注入されるだけである。
