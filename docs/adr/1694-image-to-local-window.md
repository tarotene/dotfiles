# ADR-1694 — 本人にその場で見せる図・画像は、Web ページを公開せず、手元の別窓(Chrome の専用窓)に出す

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue（本人の依頼。図や画像を、Artifact などの Web ページでなく、その場で見せる方式の検討。途中で「常用の端末が Alacritty で、画像を出す用の端末が Ghostty なのは片手落ち」という指摘を受け、端末を替えずに別窓へ出す形に作り直した）
- Related: ADR-1660(用途で面を振り分けた前例)、ADR-1677(Pages のデザイン基準)

## Context

エージェントが本人に図・画像を見せる経路は、棚卸しすると 11 本あり(裁定ページ・plan-view・model-equivalence・監査の表・代行送信の確認ページ・PR の Before / After・diagramming の図・pipeline-map・スライド・個人用 Web アプリ・Playwright のスクリーンショット)、本人にその場で見せる図は、規範がなく Artifact などの Web ページに寄りがちだった。

エージェントの Bash には制御端末が無く、出力はハーネスがファイルに捕まえるので、エスケープシーケンスは端末に届かない(`docs/agents/pkexec-guard.md`)。画像を端末の中に出すには、herdr の別ペインの出力として描くしかなく、herdr が解釈する画像は Kitty graphics protocol だけである(herdr 公式文書の「Kitty graphics」の節、2026-11-24 取得)。常用の Alacritty はこれを実装していない。

最初の案は、画像を描ける Ghostty を「外側の端末」に足し、herdr の隣のペインに描く形だった(`herdr pane run` に `chafa -f kitty` を打つ)。実測で動いたが、常用の端末(Alacritty)と、画像を出す用の端末(Ghostty)が食い違い、半端な形になる。本人の問いは二択だった: Alacritty を使い続けてよいなら、ExitPlanMode の `plan-view` のように別窓へ出すほうが簡単。そうでないなら、全部 Ghostty に寄せるべき。

Alacritty に画像の対応が入る見込みは無い。依頼が名指しした alacritty#6378 は画像でなくキー入力の拡張(kitty keyboard protocol)の Issue で、2023-12-06 に完了し 0.13.0 で出荷された(2026-11-24 に `gh` で確認)。画像は #910(sixel、open)と PR #4763(未マージ)にあるが、メンテナ本人が、どのメンテナも端末内の画像に関心が無く、マージされそうにないと書いている(PR #4763 のコメント、2026-10-13。取得 2026-11-24)。

## Decision

### D1: 図・画像は、手元の別窓(Chrome の `--app` 専用窓)に、画像そのものを開いて出す

窓の出し方は `plan-view`(ExitPlanMode のプランの窓)と同じ: 毎回新規の窓、切り離して(待たずに)起動、Linux は `setsid`、darwin は `open -a`。この起動と、表示できるかの判定(画面・ブラウザ・darwin の `.app`)は、`plan-view` と `show-image` が共有するライブラリ `crates/app-window` が持つ(feature crate 同士は依存しない。`docs/agents/repo-structure-check.md`)。`file://` の URL は、パスのバイトをパーセントエンコードして作る。

### D2: 常用の端末は Alacritty のまま、画像の対応を端末に求めない

画像を出す仕組みは、外側の端末に依らない。外側の端末が画像に対応しているかを、ホストごとに宣言する必要が無くなる(herdr のペインの中からは、外側の端末が何かを検出できない。API に項目が無く、同じサーバーに別の端末がつながりうる)。画像以外の理由で端末を替えるかは、この ADR の外の独立した決定である。

### D3: エージェントの入口は `show-image <画像のパス>` 1 本にし、sandbox の除外はこの CLI だけに与える

GUI の Chrome は、sandbox の中では窓を出せない(2026-11-24 に vega で実測。下の Verification)ので、sandbox の外で動かす。`google-chrome *` を除外に入れると、任意のフラグで sandbox の外に出られるので入れない。`show-image` は、受け取るのを画像のパス 1 つ(png・jpg・jpeg・gif・webp・svg の拡張子を持つ実在のファイル)に絞り、起動するブラウザ・フラグ・窓の大きさを固定する(環境変数では替えられない)。SVG は `rsvg-convert` で PNG にしてから開く(D5)。

終了コードは、0 = 窓に出した、2 = 使い方かパスの誤り、3 = 出せない(画面が無い、ブラウザが無い、SVG を PNG にできない)。3 のとき、呼び出し側は従来の経路(Web ページなど)に落ちる。

### D4: 振り分けの規範

その場で見せるだけの図・画像は、別窓(`show-image`)。Web ページに残すのは、入力を受ける面(裁定ページ)、他人と共有する面、GitHub 上で読む面(PR の Before / After、コミットした SVG)、操作のある面(model-equivalence のスライダー)。図でない表と文書(plan-view、監査の表)は、この ADR の対象外。規範は `config/agents/AGENTS.md` に書く。

### D5: SVG は PNG にしてから窓に渡す

SVG を文書として直接開くと、スクリプトが走りうる。MDN は、SVG を画像として使うときは JavaScript が無効だと書くが、「一部のブラウザで」と限定し、直接開く場合は制限が当たらないと明記している(MDN "SVG as an image"、2025-05-13 更新、取得 2026-11-24)。スクリプトを持てない PNG に変えてから渡す。

## Alternatives considered

- **Ghostty を外側の端末にして、herdr の隣のペインに描く(最初の案、PR #1695)**: 採らない。(1) 外側の端末の対応は herdr から検出できず、ホストごとの宣言ファイルが事実の複写として残り、宣言と実際がずれると空白が出る。(2) herdr の Kitty graphics には open の Issue が多い(2026-11-24 の検索が上限の 30 件に当たった。題名を読んだ範囲)。Ghostty が絡むものも複数ある(#5985 は Ghostty 1.4.0 で画像が出ない報告。本文は読んだ: macOS・herdr 0.9.4。#3697・#5656・#5134 は題名のみ読んだ)。(3) 常用の Alacritty を完全に外す覚悟が要り、arcturus と altair は未測。段 0 で、Ghostty 1.2.3(apt)と 1.3.1(nix)のどちらでも画像が出ることは実測した(下の Verification)。**全部 Ghostty に倒す条件**: 画像が「隣のペインに出る」ことが本人にとって譲れない体験で、Alacritty を完全に外すとき。そのときは、arcturus・altair を先に実測し、Ghostty は 1.3.1 に pin して、#5985 が解けるまで 1.4.0 以降に上げない。
- **Alacritty のまま chafa の文字での近似(`-f symbols`)**: 採らない。実測で、日本語ラベルの図の大きい見出しも読めなかった。
- **alacritty-graphics 0.17.0(PR #4763 の fork)**: 採らない。sixel 系で、herdr が解釈する画像の仕組みは Kitty graphics だけ。fork は upstream 正本ではない。未実測。
- **Alacritty 本体が画像に対応するのを待つ**: 採らない。上の Context のとおり、メンテナが取り込まないと書いている。
- **`herdr pane split`・`herdr pane run` を sandbox の除外に入れる**: 採らない。`pane run` は引数を空白で連結し、クォートせずにペインのシェルへ打つ(実測)ので、任意のコマンドを sandbox の外で打てる口になる。この案を採らないので、`herdr pane` 自体を使わなくなった。
- **SVG を直接開く、HTML の `<img>` で包む**: 採らない(D5)。ブラウザごとの制限の差が未確認。
- **kitty 0.49.1**: Ghostty が不合格のときの次点として挙げていた。Ghostty が合格したので調べなかった。
- **WezTerm・Warp**: 感触で外した。分析ではない。

## Consequences

- 窓は毎回新規に増える。読み終わったら `Ctrl+W` で閉じる(`plan-view` と同じ)。フォーカスを奪うかは未測。
- 画面が無いセッション(`DISPLAY` と `WAYLAND_DISPLAY` が無い)・ブラウザが無いホストでは、終了コード 3 で終わり、従来の経路に落ちる。
- herdr を remote でつなぐ使い方では、窓は、`show-image` が動くマシンの画面に出る。その使い方の有無は未確認。
- darwin(altair)の窓の起動は、`plan-view` の `open -a` の分岐を共有しているが、実機では未測。darwin の `rsvg-convert` は、今は Linux だけに入っている(`home/modules/packages.nix`)ので、darwin では SVG は 3 で終わる。
- 外側の端末が画像に対応しているかを、ホストごとに宣言する仕組み(`dotfiles.terminal.graphics`)は持たない。
- `alacritty#6378` のキー入力の拡張は、Alacritty 0.17.0 から herdr のペインまで届く(下の Verification)。端末内でキー操作のアプリを作るときの前提は満たしている。この ADR はそれを決めない。

## 執行点

- `crates/show-image/src/main.rs`
- `crates/app-window/src/lib.rs`
- `home/modules/claude.nix`
- `home/modules/packages.nix`

## Verification

2026-11-24、vega。見たのは本人。

採った案(別窓):

| 条件 | 結果 |
|---|---|
| sandbox の中から `setsid google-chrome --app=file://<PNG> --window-size=1000,800` を起動 | 窓は出ない |
| sandbox の外から同じコマンドを起動 | 窓が出る |
| `hms .` で配備した `show-image` に、PNG を渡す(単体呼び出し。sandbox の除外が効く形) | 終了コード 0。窓が出て、文字が読める |
| 同じく、ファイル名に空白と `#` を含む SVG を渡す | 終了コード 0。PNG(`~/.cache/show-image/poc2__x.png`)に変換され、その窓が出て、文字が読める。URL のエンコードが効いた |
| `DISPLAY`・`WAYLAND_DISPLAY` を外して呼ぶ | 終了コード 3。「画面が無いセッションです」と出て、窓は出ない |
| 存在しないパス・引数なし | どちらも終了コード 2 |

採らなかった案の根拠(Ghostty と herdr のペイン。herdr 0.9.5、chafa 1.18.2):

| 条件 | 結果 |
|---|---|
| Ghostty 1.2.3(apt)+ herdr のペインで `chafa -f kitty <png>` | 画像が出る。日本語ラベルと小さい英数字が読める。分割・切り替え・スクロールで崩れない。日本語入力が通る |
| 同じ構成で、sandbox の外から `herdr pane split` と `herdr pane run` を呼ぶ(エージェント側) | 隣のペインに図が出る(pipeline-map の図) |
| nix 版 Ghostty 1.3.1(nixGL ラッパー)+ herdr | 窓が開き、図が 2 つとも出て、日本語入力が通る。最初の試行は、herdr のペインから起動したため herdr の環境変数が引き継がれ、`nested herdr is disabled by default` で拒否された(こちらの起動の誤り。環境変数を外すと通った) |
| Alacritty 0.17.0 + herdr で `chafa -f symbols --size 90x26` | 日本語の大きい見出しも読めない |
| Alacritty 0.17.0 + herdr で `chafa -f kitty` | 何も出ない(空白。エラーも文字化けも無い) |
| Alacritty 0.17.0 → herdr のペインで、kitty keyboard protocol の flag 1 を要求して押したキー | 問い合わせの応答 `ESC[?0u`。Enter は `\r`、Ctrl+Enter は `ESC[13;5u`、Ctrl+a は `ESC[97;5u`、Ctrl+Shift+a は `ESC[97;6u`(区別されて届く) |
| `herdr pane run P printf '[%s]\n' 'a b' ';' echo SECOND`(引数の扱い) | 引数は空白で連結され、クォートされずにペインのシェルへ打たれた |
| sandbox の中で `herdr pane current` | `Operation not permitted`(外では通る) |

未測: 窓がフォーカスを奪うか、arcturus と altair、Ghostty 1.4.0 以降。窓が出たかは、画面を読み戻せないので、本人が見て確かめた。
