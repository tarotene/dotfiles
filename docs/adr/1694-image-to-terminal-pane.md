# ADR-1694 — 本人にその場で見せる図・画像は、Web ページでなく herdr の隣のペインに出す

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue（本人の依頼。図や画像を、Artifact などの Web ページでなく端末の中で見せる方式の検討。対象を聞いた質問への回答は、画像・図を端末に出したい）
- Related: ADR-1660(用途で面を振り分けた前例)、ADR-1677(Pages のデザイン基準)

## Context

エージェントが本人に図・画像を見せる経路は、棚卸しすると 11 本あり(裁定ページ・plan-view・model-equivalence・監査の表・代行送信の確認ページ・PR の Before / After・diagramming の図・pipeline-map・スライド・個人用 Web アプリ・Playwright のスクリーンショット)、本人にその場で見せる図は、規範がなく Artifact などの Web ページに寄りがちだった。本人は、これを端末の中のやり取りへ寄せることを検討したいと言った。

依頼が名指しした alacritty#6378 は、画像ではなくキー入力の拡張(kitty keyboard protocol)の Issue である。2023-12-06 に完了し、0.13.0 で出荷された(2026-11-24 に `gh` で確認)。画像を端末に出す仕組みは Alacritty 本体に無い(#910 が open、PR #4763 が未マージ。2026-11-24 に確認)。

エージェントの Bash には制御端末が無く、出力はハーネスがファイルに捕まえるので、エスケープシーケンスは端末に届かない(`docs/agents/pkexec-guard.md`)。画像は、別のペインの出力として描くしかない。herdr 0.9.5 はペインを自前の端末エミュレーションで受け、画像は Kitty graphics protocol だけを解釈して、対応する外側の端末に描く(herdr 公式文書の「Kitty graphics」の節、2026-11-24 取得)。

## Decision

### D1: 図・画像は、herdr の別ペインの出力として、Kitty graphics protocol で描く

エージェントの Bash の出力に画像を載せない。`herdr pane split` で隣のペインを作り、`herdr pane run` で `chafa -f kitty <画像>` を打つ。

### D2: 外側の端末を、Kitty graphics protocol に対応した Ghostty にする

pin した nixpkgs の ghostty 1.3.1(Linux は nixGL ラッパー、darwin は `ghostty-bin`)から宣言する。この ADR の PR は端末を替えない(替える宣言は後続の段)。外側の端末が対応しているかは herdr のペインの中からは検出できないので、対応するホストが `dotfiles.terminal.graphics.kittyProtocol = true` と宣言する(`home/modules/terminal-graphics.nix`)。宣言は `~/.config/dotfiles/terminal-graphics` に `kitty` と書かれる。

### D3: 描画の道具は chafa にする

`-f kitty` で Kitty graphics、`-f symbols` で文字での近似が出せる。chafa 1.18.2(pin した nixpkgs)。

### D4: エージェントの入口は `show-image <画像のパス>` 1 本にし、sandbox の除外はこの CLI だけに与える

`herdr pane run` は、ペインのシェルに任意のコマンドを打てる(引数は空白で連結され、クォートされずに打たれる。実測)。sandbox の外で動かす口としては広すぎるので、除外に入れない。`show-image` は、受け取るのを画像のパス 1 つ(png・jpg・jpeg・gif・webp・svg。`'`・`\`・制御文字を含むパスは拒否)に絞り、ペインに打つ行を固定する(`clear; chafa -f kitty '<path>'; printf '%s\n' '<path>'`)。SVG は `rsvg-convert` で PNG にしてから渡す。閲覧ペインは herdr のサーバーごとに 1 つを使い回す。

宣言の無いホスト・herdr の外では、理由を出して終了コード 3 で終わる。呼び出し側はこのとき従来の経路(Web ページなど)に落ちる。描いたあとに画像のパスを文字でも出すので、外側が対応していない端末で画像の部分が空白になっても、原因を追える。

### D5: 振り分けの規範

その場で見せるだけの図・画像は、端末のペイン(`show-image`)。Web ページに残すのは、入力を受ける面(裁定ページ)、他人と共有する面、GitHub 上で読む面(PR の Before / After、コミットした SVG)、操作のある面(model-equivalence のスライダー)。図でない表と文書(plan-view、監査の表)は、この ADR の対象外。規範は `config/agents/AGENTS.md` に書く。

## Alternatives considered

- **Alacritty のまま chafa の文字での近似(`-f symbols`)**: 採らない。実測で、日本語ラベルの図の大きい見出しも読めなかった(下の Verification)。
- **alacritty-graphics 0.17.0(PR #4763 の fork)**: 採らない。sixel 系で、herdr が解釈する画像の仕組みは Kitty graphics だけ(herdr の文書に sixel の記述が無い)。fork は upstream 正本ではない。未実測。
- **kitty 0.49.1**: Ghostty が不合格のときの次点。合格したので採らなかった。
- **`herdr pane split *`・`herdr pane run *` を sandbox の除外に入れ、手順を skill の文章で持つ**: 採らない。`pane run` は任意のコマンドを sandbox の外で打てる口になる。
- **herdr の plugin のペイン**: 採らない。plugin のペインが文字の大きさを返さない Issue(herdr #5594、題名のみ読んだ)があり、画像の大きさを決められない恐れがある。未実測。
- **WezTerm・Warp**: 感触で外した。分析ではない。

## Consequences

- 外側の端末が Ghostty でない間(今の Alacritty)は、`show-image` は理由を出して 3 で終わる。端末を替える宣言は後続の段で足す。
- herdr には、Kitty graphics の open Issue が多い(#5985 は Ghostty 1.4.0 で画像が出ない報告、題名のみ読んだ)。pin する 1.3.1 での合格は実測したが、版を上げるときは同じ試験を回す。
- arcturus と altair の実機は、まだ測っていない(実機確認の台帳に載せる)。
- `alacritty#6378` のキー入力の拡張は、Alacritty 0.17.0 から herdr のペインまで届く(下の Verification)。端末内でキー操作のアプリ(裁定ページの置き換えなど)を作るときの前提は満たしている。この ADR はそれを決めない。

## 執行点

- `crates/show-image/src/main.rs`
- `home/modules/terminal-graphics.nix`
- `home/modules/claude.nix`

## Verification

2026-11-24、vega、herdr 0.9.5、chafa 1.18.2(store から直接実行。インストールなし)。見たのは本人。

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

未測: arcturus と altair、Ghostty 1.4.0 以降、`herdr pane split` の戻りが遅いときの `show-image` の挙動。
