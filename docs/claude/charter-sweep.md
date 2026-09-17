# charter-sweep: 監査駆動の README charter 一括整地

## 何が問題だったか

ADR-0013 で `github-audit-charters`(横断監査)と `repo-charter`(1 リポずつの
対話型インタビュー)を導入した直後、実アカウントに対する実行結果は
`total 34 repo(s) — ok=0 drifted=34 exempt=0` だった。ADR-0013 Decision 4 は
「既存リポジトリへの適合化は一括では行わず、監査の drift 報告に任せて各リポを
次に触るタイミングで順次適合化する」と定めていたが、全リポが drifted の状態
では「次に触るタイミング」自体がいつ来るか分からず、収束の見込みが立たない。

一方で、Decision 4 がこの方針を選んだ理由(`docs/claude/repo-charter.md`
「パイロット適合で分かったこと」)は正当だった: `repo-charter` のインタビュー
は、charter の内容(litmus の質)だけでなく「リポジトリ名と実際の責務が
食い違っている」という構造的な問題を掘り当てた実績があり、それは対話でしか
拾えない。一括処理に振り切ってこの価値を失うのは本末転倒。

## 設計判断

- **監査駆動の一括整地**。`github-audit-charters --json` の findings を
  そのまま作業リストにする。監査は既に対象列挙と drift 判定(6 項目)を
  決定論的に行っているので、それを再実装しない。
- **反映は「リポごと PR → merge 確認後に metadata」**。charter は
  README(git 管理・レビュー可能)と GitHub メタデータ(`gh repo edit` で
  即時反映)の 2 層にまたがる。description は README のミラーという
  ADR-0013 の規定(逐語一致監査)から、README が先に確定してから
  description を合わせる順序が必然的に決まる。ruleset で merge が CI 待ちに
  なるリポでも、この順序を守れば直接 push で pipeline を迂回する必要が
  ない。
- **一括起草 + 一括レビュー(GO 1 回)+ 低確信リポは個別インタビュー送り**、
  という 2 レーン構造。`repo-charter` のインタビューが価値を発揮した
  ケース(名前と責務の不一致、目的自体が Issue で争われている)は機械的に
  検出できる特徴を持つ(README とコード構成の不一致、目的を問う open
  Issue の存在)。この特徴に該当するリポだけを人間対話に残し、それ以外は
  LLM 起草 + 表レビューで済ませることで、インタビューの価値と収束性を
  両立させる。
- **Issue 棚卸しは一括レビューの表に含めるが、close は GO 後のみ**。
  ADR-0013 を書く動機になった実害(README と矛盾する open Issue)は
  charter の README 側を直すだけでは解消しない。`repo-charter` SKILL.md
  §6 の「棄却例に合致する Issue は理由を名指ししてコメント付きで close」
  という既存作法をそのまま一括レビューのレーンに乗せる。close という
  外向きの不可逆操作だけは、一括起草・一括表提示のフェーズから切り離し、
  人間の GO を待つ。
- **別 skill `charter-sweep` として切り出す**。このリポジトリには
  `wrapup-inbox`(蓄積側・hook)と `wrapup-chores`(一括消化側・skill)を
  別々に分けた先例があり(`docs/claude/wrapup-chores.md`)、起動条件も
  作法も異なる「1 リポの対話型インタビュー」と「横断一括処理」を 1 つの
  SKILL.md に同居させると、両者のトリガー記述(`description`)が薄まる。
  charter スキーマの定義・見出しリテラルは重複させず `repo-charter` を
  正本として参照する。

## ADR-0013 との関係(Amendment)

ADR-0013 Decision 4 は immutable なので、本文を書き換えず Amendment 節を
追記する(ADR-0001・ADR-0003 と同じ作法)。Amendment の要旨は「LLM 起草 +
人間の一括レビュー + 低確信リポの個別インタビュー送り」という第 3 の
enforcement point を追加すること — creation-time(`repo-charter`)/
after-the-fact audit(`github-audit-charters`)の既存 2 点はそのまま残る。

## スコープ外(現時点)

- charter-sweep 自体を定期実行する仕組み(手動コマンドの延長のまま)。
- Issue 起票時のリトマス自動照合(ADR-0013 が既に第 2 弾送りにしている
  範囲と同じ)。
- 一括レビュー表の永続化(セッション内限り。private リポ名を含み得るため
  このリポジトリの成果物には残さない)。
