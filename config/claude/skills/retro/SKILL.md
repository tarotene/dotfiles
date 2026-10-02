---
name: retro
description: 作業の終わりに、このセッションの指摘・気付き・詰まり・hook に当たった事実を全て振り返り、仕組み化の候補として記録して起票経路(wrap-up inbox)に流す手順。PR を作ったセッションでは Stop hook が自動で求める。PR の無いセッション(調査・設計だけ)や、途中で区切りを付けたいときに手動で呼ぶ。レトロ・振り返り・レトロスペクティブ・retro・retrospective・今日の作業を振り返って・仕組み化の検討、といった文脈で使う。
---

PR を作ったセッションでは、最初の Stop で `wrapup-stop-gate` がレトロを求める
(ADR-697)。このスキルは、PR が無いセッションで同じ手順を手動で踏むためのもの。
手順の正本は `wrapup-stop-gate --retro-procedure` の出力で、ここには書き写さない。

## 手順

1. セッション ID を確かめる。SessionStart で渡されたものか、Stop の指示文
   (`--retro-procedure '<session_id>'`)に出ているものを使う。
2. 手順書を取る:
   ```
   ~/.claude/hooks/wrapup-stop-gate --retro-procedure '<session_id>'
   ```
   手動のセッションでは transcript のパスが hook に渡っていないので、ユーザー発言の一覧は
   出ない。その場合は会話を自分で見返して、ユーザーの指摘・不満・言い直しを拾う。
3. 出力に従って `--retro-add` で行を書く。語彙は閉じていて、語彙外は exit 64 になる。
   「何も無かった」は `kind: none` の 1 行で表す。黙って飛ばさない。
4. 全行を表にして、投稿先に 1 件コメントする。GitHub に出す先が無い(PR も Issue も無い)
   ときは、表をチャットに出し、`--retro-close '<session_id>' 'none:<理由>'` で閉じる。
   その場合は「投稿は未確認」と報告する。
5. `disposition: inbox` にした行は、通常の inbox と同じ流れ(Stop 時の `--procedure`、
   または `/wrapup-chores`)で起票される。ここで直接 `gh issue create` しない。

## 判断の目安

- **指摘(user-correction)**: ユーザーが方針・結果を直した発言。`mechanism` は、それを
  次から防ぐ最も軽い手段(`prose` < `script` < `gate`)。すでに gate があるのに漏れたなら
  `existing:<名前>` とし、その gate の欠陥として `what` に名前を書く。
- **気付き(insight)**: 次に効く発見。仕組みにするほどでなければ `disposition: none:<理由>`。
- **詰まり(friction)**: 手戻り・待ち・迷い。
- **gate 発火(gate-hit)**: deny や block に当たった事実。手順書の「events」に出る id を
  `evidence` にそのまま書く(書かないと完了にならない)。
- 同じ内容が既に Issue になっていてもよい。重複は再発コメントとして積まれ、ADR-543 の
  昇格判断の材料になる。
