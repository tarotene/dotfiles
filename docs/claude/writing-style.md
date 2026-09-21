# writing-style — 執筆規約への薄いポインタ

スクリプト: `scripts/writing-style-hub`
スキル: `config/claude/skills/writing-style/SKILL.md`
Issue: #115

ブログ・教材・技術記事など公開向けの長文を書くときの執筆規約(文体・
和文数式 punctuation)の正本は、この public リポジトリではなく別の
private リポジトリの `docs/style/` に置く裁定になっている(ADR-0004 の
該当 Decision — 出典は private リポジトリ側のため、ここでは内容ではなく
決定の存在だけを参照する)。dotfiles はその内容を混ぜ込まず、参照先を
解決する手順だけを持つ。

## ハブの絶対パスをソースにハードコードしない理由

private リポジトリの存在・パスをこの public リポジトリのソースに直接
書くと、それ自体が非公開情報の漏洩になる(`docs/claude/
public-publish-guard.md` が扱う脅威モデルと同種)。そのため ADR-0019 の
ホスト・マーカー方式(`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/host`)
と同じ間接参照の型を採る: マーカーファイル 1 個(`dotfiles/style-hub`)
+ 環境変数(`WRITING_STYLE_HUB`、セッション限定の上書き用)のどちらかから
解決する。

このマーカーは ADR-0019 の `host` と違い home-manager では宣言しない
(宣言すると絶対パスがソースに写ってしまい、間接参照にした意味が消える)。
手置きファイルのまま運用する。

## 解決順序

1. `$WRITING_STYLE_HUB`(環境変数)
2. `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/style-hub` の中身(1行、絶対パス)

## 縮退は明示的に失敗する

`docs/claude/wrapup-inbox.md` と同じ binary-existence gating の流儀を
踏襲するが、方向が逆になる点に注意する: wrapup-inbox は「無ければ黙って
何もしない」(hook の縮退として正しい)。`writing-style-hub` は「無ければ
明示的なメッセージを出して止まる」— 執筆規約への参照漏れは hook の
沈黙で許容してよい種類の失敗ではなく、書く前に気づかせる必要がある
(#115 の敵対的レビュー指摘「無音失敗にしない」)。

具体的には次の3条件のいずれかで stderr にメッセージを出し非 0 で終わる:

- マーカーも環境変数も無い
- マーカー/環境変数が指すパスが存在しない
- 指すパスの `docs/style/README.md` が読めない(レイアウト不一致の疑い)

## CI selftest

`writing-style-hub --selftest`(`.github/workflows/ci.yml` に配線)は
上記の縮退経路すべてと、環境変数がマーカーより優先されることを、
実機の `$HOME`/`$XDG_CONFIG_HOME` から隔離した一時ディレクトリで検査する。
ネットワーク・実際の private リポジトリへのアクセスは不要。

## 参照

- private リポジトリの ADR-0004 D7(この裁定そのものの出典。内容は
  private 側にあるためここではリンクしない)
- private リポジトリの `docs/style/README.md` の「Consumer contract」節
  (`writing-style-hub` が解決した後に実際に読む場所)
- `docs/claude/wrapup-inbox.md`(binary-existence gating の先例)
- `docs/adr/0019-star-codename-hosts-and-marker-resolution.md`
  (マーカーファイルによる間接参照の先行例)
