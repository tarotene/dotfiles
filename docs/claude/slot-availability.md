# slot-availability — 候補日程 × Google Calendar の3コマ当たり判定

## なぜこれが必要か

調整さん等の候補日程一覧を Google Calendar の空き状況と突き合わせる作業は、
素朴に「時間指定予定と重なるか」だけを見ると欠陥がある。試験・研修のように
終日イベント(多くは Google Calendar 上 `transparent` = 予定なし扱い)として
登録された拘束時間を見落とし、実際には身動きが取れない日を ○ と誤判定する。
判定ルール(コマ境界・バッファ・終日イベントの扱い)をその場の会話で毎回
組み立てるとブレが出るため、判定は決定的なスクリプトに固定した。

## 器をスキルにした理由

候補日程一覧の取得(Web ページの読み取り)・カレンダーの読み書き(MCP)・
合意形成(判定表の提示と承認)は文脈判断を要し、機械的なフックでは代替
できない。判定ロジック自体は decidable なので `scripts/slot-hit.py` に
切り出し、skill はその前後の I/O 手順を統括する。

既存の `external-call-scheduling` は「Claude が代行できないハンドオフ作業」
が発火条件で、調整さんへの回答は Claude 自身が実行できる作業のため発火
条件が噛み合わない。両者は判断の型(営業時間 × 空き時間 の突合)は似て
いるが、別スキルとして独立させた。

## 個人設定を dotfiles に置かない理由

判定ルール自体(コマの時間境界・バッファ分数)は汎用的でリポジトリに置ける
が、実際の運用にはユーザー個人のカレンダー ID・所属団体名を含む
`[[calendars]]` allowlist が要る。これは `external-send-guard` の
`~/.config/external-send-guard/self.txt` と同種の個人識別情報であり、
public な dotfiles には置かない。今回は self.txt と異なり版管理したい
という運用裁定(grilling セッション、2026-09-25)があったため、private な
person-state リポジトリを正本にし、`~/.config/slot-availability/config.toml`
への手動シンボリックリンクで既定の読み込み先に接続する構成にした。
nix の `home.file` で symlink を宣言すると private リポジトリ名が public な
dotfiles に露出するため、この symlink 作成だけは nix 管理に含めず手動にする。

## 設定スキーマ

`SKILL.md` の説明のとおり TOML。`[slots]` は名前をキーにした区間の辞書、
`[[calendars]]` は判定元カレンダーの allowlist(`id` は `list_calendars` が
返す ID)、`[all_day]` の `ignore_prefixes`/`soft_day_prefixes` は終日
イベントのタイトル前方一致リスト。スキーマの正本はスクリプト本体の
docstring(`scripts/slot-hit.py` 冒頭)— ここでは重複させない。

## カレンダーの正規化を skill の手順に組み込んだ理由

ユーザーからのフィードバック(2026-09-25、grilling セッション):「調べもので
本来の拘束時間がわかったやつは Google Calendar 側に Write してほしい。
Google Calendar 側もある程度自明な事実に沿って正規化されていてほしい」。
これを受けて、設定ファイルに時間割などの事実データを持たせず、判明した
事実は Google Calendar 側に時間指定の予定として書き戻す運用にした。同じ
事実を設定ファイルとカレンダーの二重管理にすると、どちらかが古くなった
ときに判定が静かに誤るため、正本を Calendar 1 箇所に絞っている。

この原則は `external-call-scheduling`(電話等のハンドオフ作業)にも当てはまる
可能性があるが、このスキルの初版では slot-availability 側にのみ反映した。
横展開の要否は別 Issue で検討する。

## 運用

新しい失敗事例(判定ロジックの誤り、サニタイズ漏れ等)が出たら `SKILL.md`
の事例節に追記する。
