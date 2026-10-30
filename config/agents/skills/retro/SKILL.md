---
name: retro
description: 作業の終わりに、このセッションの指摘・気付き・詰まり・hook に当たった事実を全て振り返り、仕組み化の候補として記録して起票経路(wrap-up inbox)に流す手順。PR を作ったセッションでは Stop hook が自動で求める。PR の無いセッション(調査・設計だけ)や、途中で区切りを付けたいときに手動で呼ぶ。レトロ・振り返り・レトロスペクティブ・retro・retrospective・今日の作業を振り返って・仕組み化の検討、といった文脈で使う。
---

PR を作ったセッションでは、最初の Stop で `session-wrapup stop` がレトロを求める
(ADR-697)。このスキルは、PR が無いセッションで同じ手順を手動で踏むためのもの。
手順の正本は `session-retrospective --retro-procedure` の出力で、ここには書き写さない。

## 手順

1. セッション ID を確かめる。SessionStart で渡されたものか、Stop の指示文
   (`--retro-procedure '<session_id>'`)に出ているものを使う。
2. 手順書を取る:
   ```
   ~/.claude/hooks/session-retrospective --retro-procedure '<session_id>'
   ```
   手動のセッションでは transcript のパスが hook に渡っていないので、ユーザー発言の一覧は
   出ない。その場合は会話を自分で見返して、ユーザーの指摘・不満・言い直しを拾う。
3. 出力に従って `--retro-add` で行を書く。語彙は閉じていて、語彙外は exit 64 になる。
   「何も無かった」は `kind: none` の 1 行で表す。黙って飛ばさない。
4. 全行がそろったら `--retro-close '<session_id>'` を実行する。台帳の全行を kind ごとの表にして、vault の
   `retro/<年>/` に 1 件のノートとして書き出し、読み戻してから完了にする(PR にはコメントしない。表を
   自分で書き直さない)。vault の無い機体では、台帳に残したまま完了になる。
5. `disposition: inbox` にした行は、通常の気づきと同じく inbox(vault のノート、vault の無い機体では
   jsonl)に入る。Issue にはならず、`/backlog-cleanup` で本人が裁定する。ここで直接
   `gh issue create` しない。ただし `kind: user-correction` の行は `inbox` にできない(共有 AGENTS.md の
   「フィードバックは Issue 化」)。先に `gh issue create` で Issue にして `issue:#<N>` と書くか、
   プロジェクト固有・機微な内容なら `none:<理由>` と書く。

## 判断の目安

- **指摘(user-correction)**: ユーザーが方針・結果を直した発言。`mechanism` は、それを
  次から防ぐ最も軽い手段(`prose` < `script` < `gate`)。すでに gate があるのに漏れたなら
  `existing:<名前>` とし、その gate の欠陥として `what` に名前を書く。
- **気付き(insight)**: 次に効く発見。仕組みにするほどでなければ `disposition: none:<理由>`。
- **詰まり(friction)**: 手戻り・待ち・迷い。
- **gate 発火(gate-hit)**: deny や block に当たった事実。手順書の「events」に出る id を
  `evidence` にそのまま書く(書かないと完了にならない)。
- 同じ内容が既に Inbox のノートになっていてもよい。同じ題の気付きはノートの行として積まれ、
  行数が ADR-543 の昇格判断の材料になる(`issue-inbox --list-open` の `count`、ADR-1136)。
