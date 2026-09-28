# pr-confirm-guard — PR 本文の「要確認」に機械的下限を課す

判定エンジン: `config/claude/hooks/pr-confirm-guard.sh`
規約側: `config/claude/skills/pr-description/SKILL.md` §1・§6
設計判断: ADR-543「既存手段の前倒し接地と、決定論への昇格導線」D2(散文からの
昇格条件)
Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

`pr-description` スキル §6 は「要確認」を人間にしかできないブロッキング項目
専用の節と定め、閉じた語彙(資格情報発行・ハードウェア操作・secrets 衛生・
GUI 操作・人間の判断)のいずれかを理由に書くことを求めていた。この規律は
散文(スキルの記述)だけで支えられており、機械検査は一切無かった。

## 動機

「要確認」の質を欠く失敗が2度起きた。

1. tarotene/dotfiles#239: マージ後の残作業をすべて「要確認」に書いてしまい、
   ユーザーから「これは私がやることですか」と指摘された(ブロッキング理由の
   絞り込み自体が甘かった)。
2. 別のリポジトリで、ブロッキング理由は正しく1件(secrets 衛生)に絞れて
   いたが、その手順が「取り出した値をクラウド環境の credentials に登録する」
   としか書かれておらず、どの画面のどの項目に何を入れるか・完了をどう確認
   するかが無かった(理由の絞り込みは正しいが、手順の粒度が甘い)。

1度目は pr-description スキル §6 の「ブロッキング判定の閉集合」を明文化する
ことで対処済みだったが、2度目は別の観点(手順自体の具体性)での再発であり、
ADR-543 D2 の昇格条件(同じ規範違反の再発)に該当すると判断し、散文だけで
なく PreToolUse gate でも機械強制することにした。

## なぜ Stop hook(`G_visual`)ではなく PreToolUse か

`pr-gate.sh` の `G_visual`/`G_link` は Stop hook で、PR 作成後にセッション
終了時点で検査する。`pr-confirm-guard.sh` はそれより早い `gh pr create/edit`
の呼び出しそのものを deny する — `pr-title-guard.sh` と同じ理由で、本文の
不備を「呼び出しをもう一度正しく書く」形で1回で直させる。`pr-gate.sh` の
allowlist(既定 `tarotene/dotfiles` のみ)に入っていないリポジトリでも発火
させたかった、というのも理由の一つ — `pr-title-guard.sh` と同じく owner
スコープ(`tarotene/*` 全体)だけで判定でき、allowlist ファイルの追加管理を
要らない。

## 判定基準

`## 要確認`(または任意レベルの見出しで「要確認」を含むもの)が本文に無ければ
無条件 pass。見出しはあるが項目が無くても pass。配下の各トップレベル項目
(フェンス外・列頭の `- `/`* `/`N. ` で始まる行から次の列頭マーカーまたは
節末までを1項目とする)ごとに次の3条件の AND:

```
(a) ブロッキング: <資格情報|ハードウェア|secrets 衛生|GUI|判断>
    — pr-description スキル §6 の既存閉集合をそのまま採用(新語彙を作らない)
(b) フェンス外にインデントされた `N. ` 形式の手順行が1つ以上
(c) `完了確認:` 行(理由必須)
```

`G_visual` の3択OR(画像/fence/`No-Visual:`のいずれか)とは異なり必須3要素
の AND にした——「要確認」は「これはブロッキングだ」という主張そのものが
Before/After 証跡より強く、理由・手順・完了確認の3点セットが揃って初めて
読み手が自分の仕事だと判断できるため。

## 検査しないこと

手順が実際に妥当か(値が正しいか、画面遷移が実在のUIと一致するか)は検査
しない。`G_visual` が「対比として十分か」を検査せず「証跡の有無」までに
留めるのと同じ理由 —— 機械的に判定できるのは形式の有無までで、内容の正しさ
は `pr-description` スキル(LLM の判断)の責務にする。

## 発火範囲

`tarotene/*` のリポジトリでのみ発火する(owner を `--repo`/`-R` または
`git remote get-url origin` から解決。解決不能なら fail-open) —
`pr-title-guard.sh` と同じ理由(ADR-0031 D4、会社ホストにも common 層として
配備されるため)。

一時的に無効化したいときは `PR_CONFIRM_GUARD_ALLOW=1` を立てる
(`PR_TITLE_GUARD_ALLOW=1` と同型 — 本文タグ型の恒久エスケープではない)。

## 既存手段(ADR-543)

判定エンジンは `config/claude/hooks/attribution-guard.sh` の
`split_heredoc`/`tokenize`/`is_sep`/`CMD_SEPS`(コマンド位置判定・heredoc
分離・クォート解釈)を `source` して再利用する。本文抽出
(`extract_body`)は `decide_tokens()` の本文抽出部分と同じ形だが、マーカー
判定はせず本文テキストをそのまま返す。ADR-0024 は新規 hook の既定を Rust と
するが、この判定は既存の実戦検証済みエンジンの上に成り立ち、複製実装すると
単一正本が割れる(ADR-0035 D1)ため、`pr-title-guard.sh`/`stack-base-
guard.sh`/`feedback-target-guard.sh` と同じ bash 例外を採用した。
