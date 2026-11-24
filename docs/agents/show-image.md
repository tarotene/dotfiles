# show-image — 本人にその場で見せる図・画像を、herdr の隣のペインに描く

`crates/show-image`(Rust、ADR-0024)の設計根拠と使い方。決定は `docs/adr/1694-image-to-terminal-pane.md`(ADR-1694)。
宣言は `home/modules/terminal-graphics.nix`、sandbox の除外と許可は `home/modules/claude.nix`。

## 使い方

```
show-image <画像のパス>      # png jpg jpeg gif webp svg
```

呼ぶのは、図・画像を本人にその場で見せたいときだけ。単体で呼ぶ(`;`・`&&`・リダイレクトを連結しない。連結すると sandbox の除外に合わず、herdr の socket に届かない。`docs/agents/sandbox-compound-guard.md`)。

| 終了コード | 意味 | 呼び出し側の動き |
|---|---|---|
| 0 | 隣のペインに描いた。標準出力にペイン id と画像のパス | 本人に「隣のペインに出した」と伝える |
| 2 | 使い方かパスの誤り(引数の数、ファイルが無い、拡張子、`'`・`\`・制御文字を含むパス) | パスを直して呼び直す |
| 3 | 出せない(herdr の外、ホストが画像対応の端末を宣言していない、herdr の操作の失敗) | 従来の経路(Web ページなど)に落ちる |

## なぜ入口を 1 本にしたか

エージェントの Bash には制御端末が無いので、画像は別のペインの出力として描く。そのために `herdr pane split`・`herdr pane run` が要るが、
`herdr pane run` はペインのシェルに任意のコマンドを打てる(引数は空白で連結され、クォートされずに打たれる)。これを sandbox の除外に入れると、
sandbox の外で何でも実行できる。`show-image` は受け取るのを画像のパス 1 つにし、打つ行を固定するので、除外と許可はこの形(`show-image *`)にだけ与える。

## 外側の端末の判定

外側の端末が画像に対応しているかは、herdr のペインの中からは検出できない(herdr の API に項目が無く、同じサーバーに別の端末がつながりうる)。
判定は、ホストの宣言 `dotfiles.terminal.graphics.kittyProtocol`(`~/.config/dotfiles/terminal-graphics` に `kitty`)で行う。
対応していない端末では、画像の部分は何も出ずに空白になる(Alacritty 0.17.0 で実測)ので、描いたあとに画像のパスを文字でも出す。

## 閲覧ペイン

herdr のサーバーごとに 1 つの閲覧ペインを使い回す。ペイン id は `$XDG_STATE_HOME/show-image/viewer-<socket のパス>` に書く。
ペインが閉じられていたら、`herdr pane split <今のペイン> --direction right --no-focus` で作り直す。SVG は `$XDG_CACHE_HOME/show-image/<名前>.png` に
変換して描く(同じ名前は上書き)。

## 試験

`cargo test -p show-image`(herdr と `rsvg-convert` は偽物に替えてある)。本物の herdr と画像対応の端末での確認は、実機確認の台帳に載せる。
