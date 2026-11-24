# show-image — 本人にその場で見せる図・画像を、手元の別窓(Chrome の専用窓)に出す

`crates/show-image`(Rust、ADR-0024)の設計根拠と使い方。決定は `docs/adr/1694-image-to-local-window.md`(ADR-1694)。
窓の起動は `crates/app-window`(`plan-view` と共有)、sandbox の除外と許可は `home/modules/claude.nix`、PATH への配備は
`home/modules/packages.nix`。

## 使い方

```
show-image <画像のパス>      # png jpg jpeg gif webp svg
```

呼ぶのは、図・画像を本人にその場で見せたいときだけ。単体で呼ぶ(`;`・`&&`・リダイレクトを連結しない。連結すると sandbox の除外に合わず、
sandbox の中に残って窓が出ない。`docs/agents/sandbox-compound-guard.md`)。

| 終了コード | 意味 | 呼び出し側の動き |
|---|---|---|
| 0 | 別窓に出した。標準出力に画像のパス(SVG のときは変換後の PNG) | 本人に「別窓に出した」と伝える。窓は `Ctrl+W` で閉じられる |
| 2 | 使い方かパスの誤り(引数の数、ファイルが無い、ファイルでない、拡張子) | パスを直して呼び直す |
| 3 | 出せない(画面が無い、ブラウザが無い、SVG を PNG にできない) | 従来の経路(Web ページなど)に落ちる |

画面が無いセッション(`DISPLAY` と `WAYLAND_DISPLAY` が無い。SSH 越し・無人の実行)では 3 になる。パスの誤りは、画面が無くても 2 で返る。

## 窓の出し方

`plan-view` の ExitPlanMode の窓と同じ(`docs/agents/plan-view.md`「なぜ `--app` の専用窓なのか」)。窓は毎回新規で、`google-chrome
--app=file://<画像> --window-size=1100,800` を切り離して(待たずに)起動する。Linux は `setsid`、darwin は `open -a`。`file://` の
URL は、`#`・`?`・`%`・空白・日本語を含むパスが別の部分と読まれないよう、パスのバイトをパーセントエンコードして作る。

## なぜ入口を 1 本にしたか

GUI の Chrome は sandbox の中では窓を出せない(2026-11-24 に vega で実測。ADR-1694 の Verification)ので、`show-image` は sandbox の外で動かす。
除外に `google-chrome *` を入れると、任意のフラグで sandbox の外に出られる。`show-image` は、受け取るのを画像のパス 1 つにし、起動する
ブラウザとフラグを固定する(環境変数では替えられない)ので、除外と許可はこの形(`show-image *`)にだけ与える。

## SVG は PNG にしてから開く

SVG を文書として直接開くと、スクリプトが走りうる。`rsvg-convert` で `$XDG_CACHE_HOME/show-image/<名前>.png` に変換して開く(同じ名前は上書きする)。
darwin には、今は `rsvg-convert` が入っていない(`home/modules/packages.nix` の `librsvg` は Linux だけ)ので、SVG は 3 で終わる。

## 外側の端末に依らない

端末のペインに描く形(最初の案)では、外側の端末が画像に対応しているかを、herdr のペインの中から検出できず、ホストごとの宣言が要った。
別窓にした今は、外側の端末が何であっても(Alacritty のままで)同じに動く。

## 試験

`cargo test -p show-image -p app-window`(窓の起動と `rsvg-convert` は偽物に替えてある)。本物の窓が出るかは、`hms` で配備したあとに、
PNG と SVG を 1 回ずつ `show-image` で出して、本人が画面で確かめる。
