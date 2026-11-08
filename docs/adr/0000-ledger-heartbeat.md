# ADR-0000 — 本人の Issue 台帳の、端末の生存の記録と、クラウドの日次の通知が途絶えたことを知る逆向きの確認

- Status: Accepted
- Date: 2026-11-08
- Issue: No-Issue(本人の Issue 台帳の運用で、クラウドと端末が互いの停止を知る仕組みを、裁定として決めた。台帳を持つリポジトリの名前と番号は、この公開リポジトリには書かない)
- Amends: ADR-543(日次の `promotion-detect` に、台帳の生存の記録と確認を足す)

## Context

本人の Issue 台帳は、未完了の Issue がボール保持者と次の契機を持つ運用で、その日次の通知は**クラウド(GitHub Actions)が毎朝、常設の Issue にコメントで届ける**。
クラウドは vault(本人の端末にあるノート。wrap-up inbox の実体)を読めない。台帳の通知に wrap-up inbox の件数を載せるには、端末の側が件数を渡す必要がある。

もう一つの問題は、止まりを誰も知らないことである。
クラウドの日次ジョブが止まる(workflow の無効化、権限の失効、Actions の障害)と、通知が来ないだけで、「来ない」ことは何も起こさない。
端末の timer も同じで、止まっても誰も気づかない。

本人は 2026-11-08 に次を裁定した(「決定済み」)。**端末の日次 timer が生存の記録を常設の Issue に書き、クラウドがそれを読む。さらに、クラウドが止まったことを端末が検知する逆向きの確認を足す(相互監視)。**

## Decision

### D1: 端末の日次 timer が、常設の Issue `ledger-heartbeat` の本文を全面的に書き換える

台帳のリポジトリに、タイトル `ledger-heartbeat`・ラベル `ledger-status` の open な Issue がある(無ければクラウド側が作る。**端末側は作らない**)。
`promotion-detect`(ADR-543、日次の timer)が、掃除(ADR-1381)のあとに、その Issue の本文を次の書式で書き換える。

```
asof: 2026-11-08T06:00:00Z
inbox <プロジェクト名>: <wrap-up inbox の open 件数>
inbox <別のプロジェクト名>: <n>
```

**本文の書式の契約**(クラウド側が読む。端末側はこれを変えない):

- 本文はこの行だけで、先頭行が `asof:` である。末尾に改行を付けない。
- `asof` は UTC の ISO8601(`YYYY-MM-DDTHH:MM:SSZ`、`[0-9T:Z-]+`)。端末が書いた時刻である。
- `inbox` の行は 1 プロジェクト 1 行で、プロジェクト名の昇順。プロジェクト名は wrap-up inbox のノートの `repo`(`OWNER/REPO`)である。
  空白・制御文字・`:` は `_` に置き換え(1 行の形を壊さない)、`repo` が空のノートは `unknown` にまとめる。件数が 0 のプロジェクトは書かない。
- 件数は、掃除を終えたあとの `open` の数である(`lapsed` は数えない)。
- 冪等: 既に書かれている本文と同じ(改行コードと末尾の空白を除いて)なら更新しない。`asof` が変われば本文が変わるので、日次の実行は毎回書き換える(生存の記録だから、`asof` を更新しないという選択はしない)。
- vault を宣言していない機体、または `open` を数えられなかった機体は、書かない。数えられなかったことを「件数 0」と書くと、クラウドが「inbox は空」と誤読する。
- `ledger-heartbeat` の Issue が無いときは、標準エラーに 1 行出して書かずに続ける。

台帳のリポジトリは非公開で、名前はこのリポジトリのコード・テスト・文書のどこにも書かない。**端末ローカルの設定**
`$XDG_CONFIG_HOME/ledger-heartbeat/target`(無ければ `~/.config/ledger-heartbeat/target`、1 行 `OWNER/REPO`)から読む。ファイルが無ければ機能は無効で、何もしない(標準エラーにも出さない)。
`OWNER/REPO` の形でない内容は、1 行出して飛ばす。

- 本命: 既存の日次 timer(`promotion-detect`)に載せ、`gh issue edit --body` で書く。認証は端末の `gh` のもので、`issues: write` が要る。
- 対抗馬: (a) 生存の記録専用の新しい timer と crate を足す案。(b) リポジトリのファイル(`state/` 配下)へコミットして push する案。
  表現不可能性: (a) は timer・サービス・launchd の双子・pipeline-map のノードを 1 組増やし、既存の timer が同じ vault を日次で読んでいる事実と二重になる。還元性: 既存の timer が担えるので、(a) は担う仕事が無い。
  (b) は、台帳のリポジトリにコミットを日次で積み、書き込みが main への push になる(保護された ruleset と衝突する)。Issue の本文は 1 つの値を上書きするだけで、履歴を積まない。本命が勝つ。
- 既存手段: 拡張 — `crates/promotion-detect`(新しいモジュール `ledger_heartbeat`)。依存は `due-remind`(通知の応答の読み取り)と `issue-inbox`(open の数え方)で、どちらも既にある crate。新しい crate・スクリプト・timer は足さない。
- 先行例: Healthchecks.io, "Healthchecks.io Documentation"(listens for HTTP requests ("pings") from your cron jobs and scheduled tasks. It keeps silent as long as pings arrive on time. It raises an alert as soon as a ping does not arrive on time.) https://healthchecks.io/docs/ (取得 2026-11-08。著者は Healthchecks.io、公開日は取得した資料に記載なし)
- 差分: 一致 — 定期ジョブが生存の記録(ping)を外へ出し、受ける側が時間内に来ないことで止まりを知る。異なる — ping の受け手は外部サービスでなく、同じ台帳の Issue の本文で、受け手(クラウド)自身も別の記録(`ledger-digest` のコメント)を出す。記録が双方向で、端末が受け手にもなる。
- 軸: 還元 — 新しい timer を足さず、既存の日次の担い手に載せる。

### D2: 逆向きの確認 — `ledger-digest` の最新のコメントが 36 時間より古ければ、端末が Herdr で本人に知らせる

同じ日次の実行で、台帳のリポジトリの、タイトル `ledger-digest`・ラベル `ledger-status` の open な Issue の**最新のコメントの作成時刻**を読む。
それが 36 時間より古い(ちょうど 36 時間は古くない)か、Issue が無い、コメントが無いなら、本人に知らせる。

- 最新のコメントは GraphQL の `comments(last:1)` で 1 件だけ取る(コメントが日に 1 件ずつ積まれる Issue で、`gh issue view --json comments` が全件を取るかは確かめていない(未確認)ので、件数に依らない形にした)。
- 知らせ方は `due-remind`(ADR-528)と同じ経路で、`herdr notification show <題> --body <本文> --sound request` を呼ぶ。応答の `shown` の読み取りは `due_remind::find_shown_reason` を使う。
  題は「台帳の日次通知が届いていない」、本文は「日次の通知が 36 時間届いていない。台帳の常設 Issue(`OWNER/REPO#N`)を確かめる」(Issue が無いときは、見つからないことと `OWNER/REPO`)。
- 抑止は 1 日 1 回。`shown: true` を得たら、その日(JST の日付)を `<state>/promotion-detect/ledger-digest-notified` に書き、同じ日の再実行は Herdr を呼ばない。Herdr が前面に無いなど `shown: false` のときは書かず、次の実行で再試行する。
- 読み取りに失敗した(`gh` の失敗、応答の形が想定外)ときは、古いかどうかを判断できないので、通知しない。標準エラーに 1 行出して続ける。
  通知するのは、状態を読めたうえで古いと分かったときだけである(読めないことを、止まりの証拠にしない)。
- 生存の記録の書き込みと逆向きの確認は独立で、一方が失敗しても他方を行う。どちらの失敗も、日次の他の処理(候補の検出、掃除)を止めない。

- 本命: 受け手が自分の側で「来ているはずのものが来ていない」を検出する(dead man's switch)。
- 対抗馬: クラウドの側だけが端末の `asof` の古さを検出する案(片方向)。表現不可能性: 片方向だと、止まったのがクラウド自身のとき、検出する主体がいない。両方向にして初めて、片方の停止をもう片方が知る。本命が勝つ。
- 既存手段: 拡張 — `crates/due-remind` の通知の呼び出しと応答の読み取り(`find_shown_reason`)、1 日 1 回の抑止の考え方を、`promotion-detect` から使う。新しい通知経路は足さない。
- 先行例: kube-prometheus の Watchdog アラートの runbook(This is an alert meant to ensure that the entire alerting pipeline is functional. This alert is always firing, therefore it should always be firing in Alertmanager and always fire against a receiver.) https://runbooks.prometheus-operator.dev/runbooks/general/watchdog/ (取得 2026-11-08。prometheus-operator の runbook で、公開日は取得した資料に記載なし)
- 差分: 一致 — 常に出ているはずの信号が止まったことを、外の系が検出する。異なる — Watchdog は外部サービス(PagerDuty など)が受け手で、検出の主体が別にいる。ここは受け手が本人の端末で、通知は Herdr の 1 日 1 回のトーストである。
  外部サービスを足さず、端末とクラウドが互いに相手の信号を見る。
- 軸: 表現不可能 — 片方向の監視では「監視する側が止まった」状態を表現できないので、双方向にする。

### D3: 36 時間の根拠

しきい値は 36 時間である。次の 3 つから決める。

- **日次ジョブの定刻の遅れ**: クラウドの日次ジョブは、定刻から 3〜4 時間遅れて届くことがある(本人の台帳の日次ジョブでの観測。この ADR では再測定していない)。24 時間 + 遅れで、正常な運用でも最新のコメントが 27〜28 時間前になる日がある。24 時間を下回るしきい値は、正常を異常と誤検出する。
- **48 時間より短い**: 48 時間では、日次の通知が 1 日まるごと欠けても、次の通知が来るまで気づかない。1 日の欠けを、その翌日のうちに知りたい。
- **1 日の欠けを検出できる**: 通知が 1 日欠けると、最新のコメントは 48 時間前 + 遅れになる。36 時間は、遅れの上限(28 時間)と 1 日の欠け(48 時間)の間にあり、どちらにも倒れない。

36 時間は、コード上は `DIGEST_STALE_SECS`(`crates/promotion-detect/src/ledger_heartbeat.rs`)の 1 つの定数で、通知の本文の「36 時間」もここに書いてある。

- 本命: 36 時間。
- 対抗馬: 30 時間(遅れの上限に近く、遅い日に誤検出する余地がある)、48 時間(1 日の欠けを翌日のうちに検出できない)。
- 先行例なし: 探した範囲は Healthchecks.io の docs(Grace Time は「チェックが遅れたときに、アラートを送る前に待つ追加の時間」で、値は利用者が決める)と Watchdog の runbook(しきい値を示さない)。具体的な値は運用の周期と遅れの実測から決めるものなので、36 時間はここで決めた値である。
- 軸: 検出のみ — 値は設定でなく定数で、誤検出と見落としの間に置く。

## 先行例との対比

D1〜D3 の各項の中に書いた。

## 却下した案

- 端末側から `ledger-heartbeat` の Issue を作る。台帳の Issue の作成は、ボールのラベル(ADR-1379 の `issue-create-guard`)の対象外の `ledger-status` ラベルを持つ常設の 2 本で、作るのはクラウド側に決めている。端末が作ると、作成の責務が二重になる。
- `asof` が変わらない日は書き込みを省く。生存の記録は `asof` が変わることが本体で、省くとクラウドが「端末が止まった」と読む。
- 台帳のリポジトリ名を dotfiles の設定(nix)に置く。公開リポジトリに非公開のリポジトリ名が入る。端末ローカルのファイルなら、公開される場所に出ない。
- 読み取りの失敗でも通知する。通知は「止まりの証拠」を示す手段で、`gh` の一時的な失敗や認証切れを止まりと取り違えると、通知が信号にならない。読めなかったことは標準エラーに出す。

## Consequences

- **相互監視の限界(設計限界)**: クラウドと端末が**同時に**止まると、誰も気づかない。端末はクラウドの停止を、クラウドは端末の停止を検知するので、片方の停止は検知されるが、両方が止まった状態を知らせる第三者はいない。
  例: 端末の電源が切れていて、同じ日にクラウドの workflow が無効化された場合。第三者の監視(外部の死活監視サービス)を足せば塞げるが、この ADR は足さない(足すと、外部サービスという依存と、非公開の台帳の存在を外に知らせる経路が増える)。
- **通知の届く機会は、日次の実行 1 回**: `promotion-detect` の timer は日次(`OnCalendar = daily`、00:00)で、そのとき Herdr が前面に無ければ `shown: false` になり、再試行は翌日の実行になる。通知の遅れは最大で 1 日増える。
  `due-remind` のように時間ごとに再試行する周期は、この PR では足していない(未実装。必要になったら、確認だけを時間ごとの timer に分ける)。
- 台帳のリポジトリに、端末の `gh` の認証が `issues: write` を持つことが前提である。このセッションでは実機の認証を確かめていない(未確認)。権限が無ければ、書き込みは `Failed` になって標準エラーに出る。
- 設定ファイルが無い機体(会社の PC、台帳を持たない機体)では、何も起きない。
- `--dry-run` は台帳の Issue を読み書きせず、書くはずの本文を標準エラーに出す。
- クラウド側の契約(Issue のタイトル・ラベル・本文の書式)は、この ADR の D1 が端末側の正本である。クラウド側が書式を変えるときは、この ADR を改める。

## 執行点

- `crates/promotion-detect/src/ledger_heartbeat.rs`
- `crates/promotion-detect/src/main.rs`
- `crates/promotion-detect/tests/ledger_heartbeat.rs`
- `crates/issue-inbox/src/vault.rs`
- `home/modules/promotion-detect.nix`
- `pipeline-map.toml`
