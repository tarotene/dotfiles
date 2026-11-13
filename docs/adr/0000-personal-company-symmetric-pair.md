# ADR-0000 — 個人と会社は、Slack の有無と GitHub アカウントの共用を除いて、対称な対にする

- Status: Accepted
- Date: 2026-11-13
- Related: ADR-0034(private の値は公開しない、マーカーの方式)、ADR-0019(host マーカー)、ADR-1136 D3(会社用 vault を分けて自動でコピーしない)、`docs/dotfiles-identity.md`、`docs/agents/my-desk.md`

## Context

このリポジトリは、個人の機体(vega・altair)と会社の機体(arcturus)の両方を配備する。会社の機体で朝の机(`my-desk`)を動かそうとして、個人の机と会社の机が、別々の作りで育っていることが分かった。本人の見立ては、「個人と会社は、Slack の有無と、GitHub アカウントが共通であることを除けば、インフラも権限設計も完全に対称にできるはず」だった。2026-11-13 に、実機(arcturus)と公開側・private 側のソースを読み取りだけで調べた(vega・altair の実機は読んでいない)。結果は次のとおり。

- **仕組みの層は対称である。** `home/modules/*.nix` には、identity や host で分岐するコードが無い(grep で `ConditionHost` が 1 件だけ)。差は、host のファイル・identity のファイル・private 側の module の並びに集まる。Claude の権限と sandbox は、同じ宣言から作られ、allowWrite の差は vault のパスだけ(`excludedCommands` の `ja-polish *` を除く)。`hms`・通知ツール・bleep・promotion-detect の timer・`daily-desk-reminder` も同じ。
- **「違いは 2 つだけ」は、いまは成り立たない。** 差は 25 件あり、2 つの理由(下の E1・E2)で説明できるのは 2 件だけだった。別の理由が書かれた本質的な差(E3)と、理由が書かれていない、またはもう成り立たない差(I)がある。
- **リポジトリの能力は非対称である。** 個人側の person-state リポジトリには、Issue の台帳、予定の射影、日次のクラウド通知、語彙と遷移の検査、記録の層がある。会社側にはどれも無い。どれも、E1・E2 で阻まれているのでなく、まだ作っていない。

## Decision

### D1: 「側」を定義する

側は、個人と会社の 2 つ。各側が持つものは、機体、Claude アカウント(コネクタ・Artifact・routine がこれに属する)、vault、Google アカウント、**person-state リポジトリ(private)**、通知ツールの `side`、GitHub の持ち主の表の片側、裁定ページ 1 枚、机の正本。2 つの側は、同じ仕組みの 2 つの実体として扱う。

### D2: 側ごとの違いは、4 つに分類して台帳に置く

| 分類 | 意味 | 扱い |
|---|---|---|
| E1 | Slack は会社の側にだけ存在する | 会社の入力元として足す。違いのまま残す |
| E2 | GitHub アカウントを両側で共用する | 持ち主の表で側に分ける(通知ツールの `[github] side`、台帳の走査も同じ表を使う)。違いのまま残す |
| E3 | 別の理由が書かれた本質的な差(別のアカウントの照合、GPG 鍵と YubiKey を identity ごとに別に持つ、会社用 vault を分ける、ハードウェア、本人の方針) | 理由を台帳に書き、違いのまま残す |
| I | 理由が書かれていない、またはもう成り立たない差 | 解消する対象。台帳に置き、別の変更で直す |

### D3: 公開の module は、側で分岐しない

`home/modules/**` は `dotfiles.identity` を読まない(例外は、option を宣言して環境変数へ変換する `home/modules/identity.nix` だけ)。側ごとに違う値は typed な option にして、identity のファイルが値を与える。host 名の不変条件(`host_names`)と同じ型の検査で守る。

### D4: 机の入口は、両側で同じ型にする

どちらの側も、person-state リポジトリの同じパス `state/reviews/daily/desk.md` に机の正本を置き、機体ごとの目印 `desk-hub` がそのリポジトリを指す(`docs/agents/my-desk.md`)。会社の機体に個人データ用の `performance-hub` を置かない。

### D5: 差の台帳(2026-11-13 時点、25 件)

| # | 項目 | 個人 | 会社 | 理由・出典 | 分類 |
|---|---|---|---|---|---|
| 1 | Claude アカウントの照合 | `exact:`(メールアドレス) | `domain:` | 別のアカウント。`home/identities/company.nix` の「未確認」の記述は古い(実機で `dotfiles-identity verify` が成功) | E3、記述は I |
| 2 | GPG の master 鍵と YubiKey | personal 用 | company 用 | identity ごとに別に持つ(ADR-0003) | E3 |
| 3 | Tailscale | 出口ノードを常時使う | 出口ノードなし | 会社の通信を個人の出口に流さない | E3 |
| 4 | calendar-handshake-reminder | なし | あり(host で宣言) | 握手の手順で会社側が担う役(`docs/agents/calendar-handshake.md`) | E3、宣言の層(host)は I |
| 5 | feedback の試行の collector と timer | なし | あり | 理由の記述なし | I |
| 6 | ja-polish(Stop の hook、`excludedCommands`、Codex の rule) | あり | なし | 会社契約の送り先の要件を確かめてから(`docs/agents/ja-polish.md`) | I(確認待ち) |
| 7 | warp-terminal | あり | なし | クラウドの AI ツールを会社の機体に入れない(本人の方針) | E3 |
| 8 | playwright MCP | あり | なし | 理由の記述なし | I |
| 9 | esa | MCP | ローカルのミラーと注入 | アカウントと鍵が別。仕組みが違う点は理由なし | E3、仕組みは I |
| 10 | LINE・spotify-player | あり | なし | 個人のサービス。会社側に identity 層の口(importDir)が無い | E3、口は I |
| 11 | 既定のブラウザ | 宣言なし | Chrome | 理由の記述なし | I |
| 12 | Flatpak の宣言の層 | host | identity | 理由の記述なし | I |
| 13 | Obsidian のアプリ・バックアップ・同期 | あり | vault だけ | vault を分けて自動でコピーしない(ADR-1136 D3)。バックアップ・同期・アプリが無い点は理由なし | E3、残りは I |
| 14 | vault の場所の宣言の置き場 | 公開側に既定値 | private 側 | 理由の記述なし | I |
| 15 | メール整理ツールの timer | あり | なし | 個人のメールを会社のアカウントで処理しない。会社のメールに当たる仕組みが無い点は理由なし | E3、残りは I |
| 16 | checkout の鮮度の対象 | 9 件 | 1 件 | 「会社の機体は clone を持たない」。実際は持っている | I |
| 17 | update-own-tools の registry | 宣言 | 手置き | 理由の記述なし | I |
| 18 | apt の drift の baseline | 宣言 | 手置き | 理由の記述なし | I |
| 19 | github-app-snapshot | あり | なし | 「vega だけ、今は」 | I |
| 20 | ledger の heartbeat の target | 個人の機体(未確認) | ファイル無し | 端末ローカルのファイル | I(未確認) |
| 21 | meeting-prep の reminder の timer | なし | 手置き(管理外) | 理由の記述なし | I |
| 22 | `private-hub.backup` | — | 古い worktree を指す残骸 | — | I |
| 23 | Slack | アプリのみ | アプリに加え、コネクタ | 会社にだけ存在する | E1 |
| 24 | 通知ツールの通知の範囲 | `side = personal` | `side = company` | GitHub アカウントの共用 | E2 |
| 25 | ハードウェア | 機体固有 | 機体固有 | 機体の事情(アプリの選択は host ごとの選択で、側の差ではない) | E3 |

計(25 件): E1 が 1(23)、E2 が 1(24)、E3 だけが 4(2・3・7・25)、E3 の差に I の部分が混ざるものが 6(1・4・9・10・13・15)、I だけが 13(5・6・8・11・12・14・16・17・18・19・20・21・22)。会社の規程の確認が要る項目(6 の送り先の要件、13 のバックアップ先、15 の Workspace の OAuth の方針)は、確認できていない。

## Consequences

- 公開の module に、側ごとの分岐を入れると、`cargo test -p repo-structure-check` が落ちる。側ごとの違いは、option を宣言して identity・host のファイルで値を与える形にする。
- I の項目は、1 つずつ別の変更で解消する。まとめて 1 本の Issue に置く(この ADR の台帳を入力にする)。会社側の person-state リポジトリの新設と、個人側の仕組みの引数化(台帳の走査の持ち主、裁定ページの読む側の契約)は、別の変更で行う。
- 台帳は 2026-11-13 の実測である。vega・altair の実機は読んでいないので、個人側の値は公開側のソースと private 側の module の並びから読んだものを含む。

## 却下した案

- **会社の作業ツールのリポジトリを、会社の person-state にも使う案。** 公開範囲(org の全員が読める)と charter(文書とツールの 2 本の柱)が person-state と合わず、約 30 のディレクトリを動かすことになる。
- **側ごとの分岐を、公開の module の中に `mkIf` で入れる案。** 差が option の宣言の外に入り、読み手に見えなくなる。
- **個人の person-state リポジトリに、会社の机の正本も置く案。** 会社のデータが個人のリポジトリへ流れる。

## 執行点

- crates/repo-structure-check/src/side_symmetry.rs
- crates/repo-structure-check/tests/tree.rs

## References

- `docs/agents/repo-structure-check.md` — 構造の不変条件の検査の設計。
- `docs/agents/my-desk.md` — 机の入口(`desk-hub`)。
