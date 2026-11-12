# promotion-detect — Q2 の昇格・降格候補を日次で検出する

「非決定論的な処理(LLM・散文)を決定論的な処理に置き換えられないか」
(Q2)は、探索期には LLM・散文を許容し、安定の兆候が出たときにだけ
決定論スクリプト・gate へ昇格させる方式を採る(ADR-543「既存手段の前倒し
接地と、決定論への昇格導線」)。`promotion-detect`(`crates/promotion-
detect`)は、その「安定の兆候」を人間の想起に頼らず機械的に集計する
日次バッチ。

## 検出する候補

1. **再発(昇格候補)**: `tarotene/dotfiles` の `feedback` ラベル付き
   Issue のうち、同じ `Target:`(`feedback-target-guard`(`crates/feedback-target-guard`)が要求する
   閉語彙)を持つものが2件以上。同じ規範・skill への feedback が繰り返し
   起票されているので、gate 化を検討する合図。
2. **逐語反復(昇格候補)**: `~/.local/state/claude/cmd-hashes.jsonl`
   (`cmd-hash-log` が記録)に、同一の正規化コマンドハッシュが3セッション
   以上で出現し、かつそのハッシュが `~/.claude/skills/*/SKILL.md` の
   コードブロック(```` ```bash ````/```` ```sh ````)と一致するもの。
   プレースホルダ(`<...>`)を含むブロックは対象外。
   各レコードの `shape`(コマンドの形の閉じた語。ADR-1276)は、この検出では使わない
   (`session-retrospective` がセッション内の痕跡に使う)。
3. **降格(見直し候補・滞留)**: 既知の gate の skip ファイルが30日以上存在し続けているもの。
   置き場は gate ごとに違う(多くは `~/.claude/<gate>/skip`、`new-tool-guard` は
   `${XDG_STATE_HOME:-~/.local/state}/claude/new-tool-guard/skip`、`codex-plan-gate` は
   `~/.codex/codex-plan-gate/skip`)。一覧は `crates/promotion-detect/src/main.rs` の `SKIP_FILES` で、
   逃げ道の表(`agent_session::escape::ALL`)で skip ファイルを持つ項目と一致することを試験が見る。
4. **降格(見直し候補・多発)**: `~/.local/state/claude/gate-events.jsonl`
   (新規 Rust gate の deny/skip イベント、`agent_session::gate_event` が記録)
   で、ある gate の skip 回数が3件以上のもの。

5. **語彙の較正(ADR-1276)**: vault の `inbox/` のノートのうち、`signal` を持つもの(retro の `codify` などの
   行が渡した、as-code 化の候補)を、兆候ごとに採用(`fixed`・`issued`)と却下(`dropped`。理由は
   `drop_reason`)に分けて数える。しきい値は語彙(`config/agents/codify-signals/vocabulary.json` の
   `calibration`)が正本。vault を宣言していない機体では何もしない。
   - 却下が `min_dropped`(3)件以上で、裁定済みのうち却下が `drop_ratio`(2/3)以上、または
     `misread-signal` が `misread_min`(2)件以上 → 「兆候 X の定義を直す候補」。
   - `unclassified`(どれにも入らない)が `unclassified_min`(5)件以上 → 「語彙に足す兆候の候補」。
   - `existing-suffices` による却下が全兆候で `existing_suffices_min`(3)件以上 → 「既存手段の探索が浅い」。
   - 兆候の `since` より前に初出のノートは数えない。定義を直したら `since` を更新すると、数え直す。
   本文には件数と理由の id だけを書く(ノートの中身は書かない)。
6. **Git の外の状態の整合**: vault を宣言している機体で、`worklog mail check` を回す。
   Git の外(vault)に置いた状態は、PR の差分の遷移検査が効かない。書き込み口(`worklog add mail`)は語彙に無い遷移を
   拒むが、vault は Obsidian の画面から手で直せるので、書き込み口を通らない編集をここで毎日拾う。
   - 終了コード 1(食い違いあり) → 「vault の mail の状態に食い違いが N 件あります」。本文に先頭 10 件の
     `key: message` を並べ、残りは件数だけを書く。key は媒体・スレッド・受信の識別子で、メールの本文・相手の名前・
     アドレスは含まない。
   - 終了コード 2 以上・道具が無い・食い違いの行を読めない → 「vault の mail の状態の整合を読めませんでした」。
     検査が回っていないことを、黙って飛ばさずに候補にする。
   - vault を宣言していない機体では何もしない(`worklog` も呼ばない)。
   - 今後、状態を Git の外へ移す変更は、同じ形の check を移す PR に同梱して、ここに足す。
   `worklog` の場所は `PROMOTION_DETECT_WORKLOG_BIN`(既定 `~/.local/bin/worklog`)。

7. **記録の層の宣言と実測のずれ(#1310、ADR-1358 record-layers)**: `PROMOTION_DETECT_GHR_DIR`(既定 `~/.ghr`)の下の
   `<host>/<owner>/<repo>` のうち、`.config/record-layers.toml`(記録の層の宣言)を持つチェックアウトだけを読む。宣言の無い
   チェックアウト(会社・org のリポジトリを含む)は読まない。判定は `crates/record-layer`(`record-layer-guard` と同じ宣言・
   同じ照合)。詳細は [record-layers.md](record-layers.md)。
   - 宣言の欠陥: 宣言を読めない、または `git ls-files` の各ファイルに当たる glob がちょうど 1 つでない → 「記録の層の宣言に欠陥があります
     (owner/repo、…)」。本文に先頭 10 件の `path: 原因` を並べ、残りは件数だけを書く。
   - 実測のずれ: 直近 7 日に書き換わった日の数が、L0 は 3 日以上・L1 は 5 日以上・L2 は毎日のファイル → 「記録の層の宣言と、実測の変化の
     速さがずれています(owner/repo)」。本文は `path(層)` を先頭 10 件並べ、日数は書かない(毎日変わる数が入ると、同じ候補を毎日入れ直す)。
   - 題に件数・日付を入れない(同じ題は同じ候補として、`emitted.json` と `--check-dup` が畳む)。

8. **Issue の流量(ADR-1529)**: 共有 AGENTS.md「Issue に置くのは、マージ後も残る状態を持ち、ほかに置き場が無いものだけ」の
   置き場の順から外れた起票を数える。`gh issue list -R <repo>` を 2 回呼ぶ(直近 7 日の起票を `--state all --search created:>=<UTC の日付>`
   で、open の全件を `--state open` で)。判定は `crates/promotion-detect/src/issue_flow.rs`(純粋関数)。bot(login が `app/` で始まる)の起票は数えない。
   - PR の影: PR の `Closes` で閉じ(`closedByPullRequestsReferences` が空でない)、起票から 24 時間以内に閉じ、コメントが 0 件の Issue が
     2 件以上 → 「Issue の流量: PR で 1 日以内に閉じた、コメントの無い Issue が続いています」。PR の作成時刻は `gh issue list` が返さないので見ない。
   - 台帳の外の実機確認: 題が `human:`・`verify:`・`verify(` で始まるか `handoff:human` を持ち、題が `verify-ledger:` で始まらない Issue が
     1 件以上 → 「Issue の流量: 実機確認が、確認の台帳の外に起票されました」。
   - open の件数: bot を除く open が 30 件を超えた → 「Issue の流量: open の Issue が 30 件を超えています」。本文の件数は 10 件単位に丸め、
     古い順に 5 件の番号を並べる(毎日変わる数を入れると、同じ候補を毎日入れ直す)。
   - 取得か解析に失敗したら、この検出だけを飛ばす(次の日にやり直す)。閾値は `issue_flow.rs` の定数(`WINDOW_DAYS`・
     `SHADOW_THRESHOLD`・`OPEN_THRESHOLD`)。

9. **気づきの失効(ADR-1381)**: 候補の検出とは別に、vault を宣言している機体で、`issue_inbox::vault::sweep` を呼んで、
   状態が `open` のまま最後の行(初出か再発)から 14 日以上たった気づきを `lapsed` にする。vault の `inbox/` は repo をまたいで
   1 つなので、全 repo のノートが対象で、セッションを始めたプロジェクトには依らない。
   - `status` だけを書き換える。繰り返しても二重に書かない(`lapsed` は `open` でない)。`--dry-run` では書かず、件数を標準エラーに出す
     (標準出力は候補の JSON 用)。同じ操作は `issue-inbox --sweep [--dry-run]` でも手で行える。
   - 失効したノートの題が `emitted.json` にあれば外す。検出が続いている候補は、同じ run で入れ直され(再発として `open` に戻り)、
     14 日で黙って消えない。検出されなくなった候補は `lapsed` のまま残る。
   - 失敗は理由を 1 行出して続ける(翌日にやり直せる)。vault を宣言していない機体では何もしない。

10. **本人の Issue 台帳の生存の記録と逆向きの確認(ADR-1382)**: 実行の最後に、`ledger_heartbeat` が次を行う。台帳のリポジトリは非公開で、
    名前は端末ローカルの設定 `$XDG_CONFIG_HOME/ledger-heartbeat/target`(無ければ `~/.config/ledger-heartbeat/target`、1 行 `OWNER/REPO`)から読む。
    ファイルが無い機体では何もしない。
    - 生存の記録: タイトル `ledger-heartbeat`・ラベル `ledger-status` の open な Issue の本文を、掃除のあとの wrap-up inbox の open 件数で
      全面的に書き換える(先頭行 `asof: <UTC の ISO8601>`、続けて `inbox <プロジェクト名>: <件数>` を 1 プロジェクト 1 行、0 件は書かない)。
      Issue が無ければ作らず、1 行出して続ける。vault を使えない機体は、「件数 0」と誤読されないよう書かない。
    - 逆向きの確認: タイトル `ledger-digest`・ラベル `ledger-status` の Issue の最新のコメントが 36 時間より古い(Issue・コメントが無い場合を含む)なら、
      `herdr notification show` で知らせる。1 日 1 回(`<state>/promotion-detect/ledger-digest-notified`)。読み取りに失敗したときは通知しない。
    - `gh` の失敗は理由を 1 行出して続け、日次の他の処理を止めない。`--dry-run` は書かず、本文を標準エラーに出す。
    - **`--digest-check`**: 逆向きの確認だけを行う入口。候補の検出・気づきの失効・生存の記録はしない。日次の実行の時刻に Herdr が
      起動していなくて通知を出せなかった日(出せなかったときは 1 日 1 回の印を書かない)を、その日のうちに再試行するため、
      時間ごとの timer(`promotion-detect-digest-check`、毎日 09〜19 時の毎時。`due-remind` と同じ予定)が呼ぶ。印は日次と共通で、
      どちらかが出せば、その日の残りは出さない。台帳の最新のコメントが新しい、または設定ファイルが無いときは何もしない。
      `--dry-run` では何も確かめない(確認は通知を出す副作用があるため)。
    - Herdr は `PROMOTION_DETECT_HERDR_BIN`、時刻は `PROMOTION_DETECT_NOW`(unix 秒)で差し替えられる(テスト用)。

いずれも閾値は `crates/promotion-detect/src/lib.rs` の定数
(`RECURRENCE_THRESHOLD`・`VERBATIM_MIN_SESSIONS`・`SKIP_STALE_DAYS`・
`DEMOTION_SKIP_THRESHOLD`)。

## 出口は wrap-up inbox(公開 Issue 化は人間レビュー経由)

候補は `issue-inbox --check-dup` で重複を避けたうえで
`--add` により dotfiles 自身の wrap-up inbox に 1 候補 1 件で流す。
vault のある機体では vault の `inbox/` のノート(同じ題なら行が積まれる)、
vault の無い機体では `~/.local/state/claude/wrapup/github-com-tarotene-dotfiles.jsonl`
の 1 行になる(ADR-1136)。新しい出口は作らず、既存の「気付き → inbox →
`/my-cleanup`」経路をそのまま再利用する。裁定は `/my-cleanup` だけが
行い、Issue にするのは本人が選んだものだけである — 候補の文面は判断を含むため、
公開 Issue 化は必ず人間のレビューを経る。Stop hook は Issue 化を促さない。

## 同じ候補を毎日入れ直さない

timer は毎日動く。`add` は同じ日・同じ本文の行しか重複とみなさないので、翌日に同じ候補を入れると、ノートの行が 1 つ
増え、「再発の回数」(行数)が膨らんでいた。いまは `<state>/promotion-detect/emitted.json`(題 → 本文)に、
`--add` が成功した候補を残し、題も本文も同じなら入れ直さない(本文が変われば、たとえば件数が増えれば入れ直す)。
`--dry-run` はこの状態を読まず、書かない。`--add` が失敗した候補は残さないので、次回また試す。入れる行には
`repo` を必ず付ける(timer の作業ディレクトリは `$HOME` で、付けないと `--add` が repo を取り違える)。

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
