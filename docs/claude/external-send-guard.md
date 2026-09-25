# external-send-guard — 外部宛メール・Slack 送信を下書きへ誘導する PreToolUse hook

判定エンジン: `config/claude/hooks/external-send-guard.sh`
規約側: `config/agents/AGENTS.md`「外部発信は下書き止まり」節(#463。方針の
宣言はそちらが正、機械強制はこの hook 単体が担う)

Claude Code が Gmail MCP tool 経由で外部(自分以外)宛にメールを直接送信するのを
deny し、`create_draft`(返信は `replyToMessageId` 付き)で下書きを作るところで
止める。同様に、Slack MCP tool 経由の送信・返信・予約投稿も宛先(チャンネル)を
問わず無条件 deny し、draft 系 tool へ誘導する(#463)。ユーザーが Gmail/Slack
上でその下書きを編集・送信することが実質的な承認点になる。

## なぜ必要だったか(motivating case)

とある個人プロジェクトで、Claude が公式サイトに実在するメールアドレスを一般
問い合わせに使って直接送信した。ところが実際は「スタッフ募集」案内に続く
求人問い合わせ専用の窓口で、送信先の担当者から「一般の方とはこのアドレスで
やりとりすることは無い」との指摘を受けた(2026-09-22)。

アドレスが公式サイトに**実在する**ことは確認していたが、**何の窓口か**という
文脈までは確認していなかった。この文脈判定は機械では原理的に難しい(公式サイトの
自然文から「これは一般問い合わせ用か、求人専用か」を確実に判定する既製手法は
見当たらなかった — attribution-guard.md・bleep README・Claude Code hooks
公式ドキュメントを確認したが該当なし)。よって機械判定は諦め、**外部宛の直接送信
そのものを一律止め、人間の目視確認を通過点にする**設計にした。

## なぜ deny であって ask ではないか

attribution-guard.sh と同じ理由(投稿・送信は通知が飛ぶ**不可逆操作**で、
PreToolUse でしか止められない)に加えて、Claude はこの操作の代替
(`create_draft`)を自分で実行できるので、`ask` にして人間の手数を増やす理由が
ない。deny なら Claude が `create_draft` に切り替えるだけで往復 0 回、人間の
手数は「Gmail の下書き欄で確認・送信する」という、どのみち必要な作業だけになる。

## 判定の境界

### Gmail

| 形 | 判定 |
|----|------|
| `send_message` / `reply` / `forward` の宛先(to/cc/bcc)が全て自分のアドレス | 通す |
| 上記以外(外部宛を1件でも含む) | **deny** |
| `reply` で to/cc/bcc が全く指定されていない(宛先がスレッド由来で不明) | **deny**(安全側) |
| `create_draft` / `update_draft` / `list_drafts` 等の下書き系 | 対象外(常に通す) |
| Gmail 以外の MCP tool・読み取り系(`get_thread` 等) | 対象外(常に通す) |
| `self.txt` が存在しない/空 | fail-closed(自分宛でも deny) |

### Slack(#463)

宛先(チャンネル)による分岐は無い — Gmail と違い「自分宛」という概念が
Slack のチャンネル/DM には自然に対応しないため、送信系 tool は宛先を問わず
無条件 deny する。

| 形 | 判定 |
|----|------|
| `send_message` / `reply` / `schedule_message` / `post_message`(`slack_` 接頭辞の有無どちらも) | **deny**(宛先を問わない) |
| tool 名に `draft` を含む(`slack_draft_message` / `send_message_draft` 等) | 対象外(常に通す) |
| Slack 以外の MCP tool・読み取り系(`search_messages` 等) | 対象外(常に通す) |

tool 名のサーバー部分は `mcp__.*[Ss]lack.*__` で緩く見る — claude.ai
コネクタ(`mcp__claude_ai_Slack__send_message`)と Slack 公式 MCP
(`mcp__slack__slack_send_message`)の両方の命名を拾うため。draft 判定は
Gmail/Slack 共通のマッチング(`decide_mcp`)より先に行う: tool 名に `draft`
を含む呼び出しは、Slack の draft 系 tool 名がたまたま送信系パターンの部分
文字列を含む場合(例: `slack_send_message_draft`)でも deny させないため。

`self.txt`(後述)が無い状態を「安全側」にしたのは、代替手段(`create_draft`)が
常に使えて実用上の支障がないため。bleep のように「見つからない
= 危険を見逃す」方向の縮退ではなく、「見つからない = 常に確認を挟む」方向の
縮退にできるのは、この hook の代替コストがほぼ 0 だからである。

## 自分のアドレス判定と denylist データの扱い

`${XDG_CONFIG_HOME:-~/.config}/external-send-guard/self.txt`(1行1アドレス、
`#` コメント・空行は無視)を読み、to/cc/bcc の全宛先がこの集合に含まれる場合
だけ通す。bleep(`docs/claude/public-publish-guard.md` 相当、README
「denylist データは一切このリポジトリにコミットしない」)と同じ理由で、個人の
メールアドレスをこのリポジトリに書かない。ファイルは各自の home ディレクトリに
手で置く(home-manager の管轄外)。

## reply の宛先暗黙ケースを deny にした理由

`mcp__claude_ai_Gmail__reply` は `to`/`cc`/`bcc` を省略でき、省略時は元メールの
送信者への返信になる(ツール定義に明記)。この場合の実際の宛先は元メッセージ
(hook からは見えない)に依存するため、hook 側では「自分宛か外部宛か」を
判定できない。判定不能を「通す」に倒す attribution-guard.sh の縮退方針とは
逆に、ここでは deny に倒した — 誤って通した場合のコスト(外部への誤送信)が
誤って deny した場合のコスト(`create_draft` への切り替え)よりずっと大きい
非対称性があるため。この非対称性の扱いは `git-stash-guard.sh` が deny 側で
「判定を諦める = 素通し」を避ける方針(`docs/claude/git-stash-guard.md`)と
同じ考え方。

## bypass: `EXTERNAL_SEND_GUARD_ALLOW=1`

bleep と同じ形の env var bypass を持つ。**deny の理由文にはこの env var
名を書いていない** — 制約される当事者(Claude)が deny の理由を読んで自分で
bypass を再実行できてしまうため。この bypass の存在自体は、この hook を設定する
人間だけが知っていればよい。

## 対象範囲外(意図的な選択)

- **GitHub**(Issue/PR コメント・`gh pr create` 等)は対象外。このリポジトリ
  自身の完了定義(`AGENTS.md`「実装タスクの完了定義」)が `gh pr create` /
  `gh issue comment` 等の実行を確認を挟まず一続きで行うことを要求しており、
  GitHub への発信を一律 deny すると自己矛盾する。GitHub 側の「取り消し
  づらさ」への対処は PR/Issue のレビュープロセス(merge/close されるまでは
  訂正可能)に委ねる。
- **Bash 経由の送信**(`curl`/`sendmail`/他 CLI 経由の SMTP 等、`gh` を含む
  Bash コマンド全般)は対象外。この hook が仲介できるのは Claude Code の
  PreToolUse イベントだけで、Bash tool の任意のコマンド文字列から「これは
  外部発信だ」と一般的に検出するのは attribution-guard.sh の gh コマンド
  検出以上に困難(宛先の抽出・往復確認の手段が無い)。
- **LINE・Web フォーム送信**は対象外。MCP tool として接続されていても、
  「外部宛かどうか」の判定に使える宛先フィールドの形が保証されない。
  Saltzer & Schroeder の complete mediation の限界(bleep README が引く
  のと同じ根拠)がここでも成立する — この hook が仲介するのは Gmail/Slack
  の MCP tool だけであり、それ以外の経路は一切見ない。

## テスト

`external-send-guard.sh --selftest` がネットワーク不使用で以下を検査する:

- Gmail: 自分宛 send_message(通す)/ 外部宛 send_message(deny)/ cc への
  外部宛混入(deny)/ reply 宛先暗黙(deny)/ reply 自分宛明示(通す)/ reply
  外部宛明示(deny)/ forward 外部宛(deny)/ create_draft 非対象(通す)/
  無関係ツール非対象(通す)/ 宛先の大文字小文字を無視した一致(通す)/
  `self.txt` 不在時の fail-closed(deny)。
- Slack(#463): claude.ai コネクタ形・Slack 公式 MCP 形どちらの
  send_message も deny / reply も deny / schedule_message も deny /
  post_message も deny / draft 系 tool(`slack_draft_message` /
  `send_message_draft`)は対象外(通す)/ 無関係ツール(読み取り系)は
  対象外(通す)。

CI(`.github/workflows/ci.yml`)の shellcheck ステップ(`scandir:
'./config/claude/hooks'`)は新規ファイルを自動的に含む。
