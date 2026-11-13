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

- 返信が要るメールの下書きと日程回答は、発火装置が無く、依頼文があるときだけ動く。
- 軽い人手タスクは、`issue-index` が SessionStart で索引として注入する(`docs/agents/issue-index.md`)。
- 未消費の PR の裁定は、夜のクラウドの通知に 1 件だけ載っていた。
- 受信箱は、端末のバッジが起点だった。
- 期限は、カレンダーの終日予定が起点だった。

さばく作業はどれも、手元の資格情報を要し、クラウドの定期実行からは届かない。
そこで、起点を「その日最初のセッション」に 1 つ置く。クラウドの定期実行は、提案だけを届ける。

## なぜ薄いスキルにしたか

手順には、実在の差出人、看板の番号、リポジトリ名が入る。これらは public な dotfiles に置けない
(ADR-0034)。そこで、手順の正本は person-state リポジトリに置き、スキルは「ハブを解決して
`desk.md` を読んで従う」だけにする。クラウドの定期実行で採った形
(`docs/adr/519-routines-declaration-in-repo.md` D2、薄いポインタと home repo の正本)と同じ形を、
手元のスキルに適用した。手順をスキルと `desk.md` の両方に書くと、複写と同期の二重管理になる。

## ハブは机専用の目印で解決する

最初は、`performance-hub`(`crates/hub-resolve`)をそのまま流用し、同じリポジトリを指すマーカーを
増やさなかった(`docs/agents/venue-search.md` と同じ判断)。会社の機体にも机を通すとき、この判断を改めた。
`performance-hub` は演奏本番・会場探しのスキルも読む個人データの目印で、会社の機体に置くと、
個人の段が会社の機体で動く状態を作れてしまう。そこで、机専用の目印 `desk-hub` を足し、機体ごとに
その机の正本を持つリポジトリを指す。

`desk-hub`(`crates/hub-resolve` の bin。`resolve_desk` が解決する)の規則:

1. 環境変数 `DESK_HUB`、無ければ `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/desk-hub` の 1 行目が、
   机のハブ(person-state リポジトリの絶対パス)。
2. 机の正本の場所は、個人と会社で同じ `state/reviews/daily/desk.md`(ハブの中の固定のパス)。
   個人と会社は、Slack の有無と GitHub アカウントの共用を除いて、対称な対にする(同じレイアウトの
   person-state リポジトリを、側ごとに持つ)ので、場所を上書きする仕組みは持たない。正本が無ければ失敗する。
3. `desk-hub` の目印が無く、`performance-hub` があれば、従来どおりそのハブが机を持つ(個人の機体は
   何も変えずに動く)。
4. どちらも無ければ、明示のメッセージを出して非 0 で終わる。
5. 使い方: `desk-hub`(引数なし)は机のハブの絶対パスを、末尾改行なしで出す。

会社の機体は、`performance-hub` を置かず、`desk-hub` だけで会社側の person-state リポジトリを指す。
会社の机の中身(どの段をどの部品で行うか)は、会社側のリポジトリの ADR が決める。目印はどの機体でも
手で置く(home-manager は宣言しない。`docs/operations.md` の目印の表)。

## なぜ hook を `issue-index` に同居させなかったか

`issue-index` は GitHub の Search API に依存する。失敗すると机の注入も一緒に落ちる。机の判定は
日付ファイルだけでネットワークが要らないので、障害を独立させるために別の hook にした。

## daily-desk-reminder の発火条件

- イベントは SessionStart の `startup` だけ。`resume` と `compact` では出さない(印が無い限り、新しい
  セッションごとに 1 回出る)。
- 机の目印(`desk-hub`、無ければ `performance-hub`)が未設定の機体では何も出さない。会社の機体は
  `desk-hub` を置くと、会社側の正本を読んで注入が出る。ハブはあるが机の正本が読めないときは、
  stderr に 1 行だけ出す。
- 日付は JST で、`TZ` や `date(1)` に依存せず unix 秒から求める(`crates/daily-desk-reminder`)。
- 注入文は、本人が望むときにだけ `my-desk` を実行するよう促す。自動では走らせない。

## 実施済みの印

`~/.local/state/claude/desk/<JST の日付>` の空ファイル 1 つだけを新設する。机の完了を示す台帳は
作らない。完了は、メールの下書き、受信箱の操作ログ、PR のマージ状態、カレンダーから読み戻す。
印を作り忘れた場合の壊れ方は、同じ日にもう一度注入されるだけである。
