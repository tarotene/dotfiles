# promotion-detect — Q2 の昇格・降格候補を日次で検出する

「非決定論的な処理(LLM・散文)を決定論的な処理に置き換えられないか」
(Q2)は、探索期には LLM・散文を許容し、安定の兆候が出たときにだけ
決定論スクリプト・gate へ昇格させる方式を採る(ADR-543「既存手段の前倒し
接地と、決定論への昇格導線」)。`promotion-detect`(`crates/promotion-
detect`)は、その「安定の兆候」を人間の想起に頼らず機械的に集計する
日次バッチ。

## 検出する4種類の候補

1. **再発(昇格候補)**: `tarotene/dotfiles` の `feedback` ラベル付き
   Issue のうち、同じ `Target:`(`feedback-target-guard.sh` が要求する
   閉語彙)を持つものが2件以上。同じ規範・skill への feedback が繰り返し
   起票されているので、gate 化を検討する合図。
2. **逐語反復(昇格候補)**: `~/.local/state/claude/cmd-hashes.jsonl`
   (`cmd-hash-log` が記録)に、同一の正規化コマンドハッシュが3セッション
   以上で出現し、かつそのハッシュが `~/.claude/skills/*/SKILL.md` の
   コードブロック(```` ```bash ````/```` ```sh ````)と一致するもの。
   プレースホルダ(`<...>`)を含むブロックは対象外。
3. **降格(見直し候補・滞留)**: 既知の gate の skip ファイル
   (`~/.claude/<gate>/skip`)が30日以上存在し続けているもの。
4. **降格(見直し候補・多発)**: `~/.local/state/claude/gate-events.jsonl`
   (新規 Rust gate の deny/skip イベント、`hook_io::gate_event` が記録)
   で、ある gate の skip 回数が3件以上のもの。

いずれも閾値は `crates/promotion-detect/src/lib.rs` の定数
(`RECURRENCE_THRESHOLD`・`VERBATIM_MIN_SESSIONS`・`SKIP_STALE_DAYS`・
`DEMOTION_SKIP_THRESHOLD`)。

## 出口は wrap-up inbox(公開 Issue 化は人間レビュー経由)

候補は `wrapup-stop-gate.sh --check-dup` で重複を避けたうえで
`--add` により dotfiles 自身の wrap-up inbox
(`~/.local/state/claude/wrapup/github-com-tarotene-dotfiles.jsonl`)に
1候補1行で流す。新しい出口は作らず、既存の「気付き → inbox → Stop hook /
`wrapup-chores` → Issue 起票」経路をそのまま再利用する — 候補の文面は
判断を含むため、公開 Issue 化は必ず人間のレビューを経る。

## なぜ段階(prose/scripted/gated)を宣言させないか

各成果物(skill・規範・gate)が「今どの段階か」を frontmatter 等に宣言
させる案は採らなかった。検出器は実体(スクリプト・hook 登録が実在する
か)を直接見て判断すればよく、宣言欄を作ると実体とのズレという不正状態
が新たに表現可能になる(単一正本 > 複写+同期、ADR-543 D3)。

## 手動確認

```bash
PROMOTION_DETECT_REPO=tarotene/dotfiles promotion-detect --dry-run
```

`--dry-run` は inbox へ書かず、候補を1行1 JSON で stdout に出す。
