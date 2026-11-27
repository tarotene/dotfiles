# GitHub の項目(`owner/repo#N`)の鍵の符号(FORMAT.md「GitHub の鍵」)。
#
# 鍵は `gh:<スレッド>/<受信>`。スレッドは `owner/repo` の符号、受信は番号 N。鍵の文字種は [A-Za-z0-9_=-] なので、`/` と `.` は
# `=` を escape にして 16 進 2 桁で書く(`/` は `=2f`、`.` は `=2e`、`=` 自身は `=3d`)。英数字と `_` と `-` は、そのまま。
# 文書 id は鍵の `:` と `/` を `.` に、`=` を `+` に換えたもの(db の文書 id は `=` を許さない。テンプレートの docId)。
# 符号の中に `.` と `+` が出ないので、換えは 1 対 1 で、衝突しない
# (かつての `none:<owner>-<repo>-<番号>/issue` は、`.` を含むリポジトリ名を表せず、`a-b/c` と `a/b-c` が同じ形になった)。
#
# 使い方: jq -L <このディレクトリ> 'include "gh-key"; "example/sample.app" | gh_enc'
def _hexd: if . < 10 then . + 48 else . + 87 end;
def gh_enc:
  explode
  | map(. as $c
        | if ($c >= 48 and $c <= 57) or ($c >= 65 and $c <= 90) or ($c >= 97 and $c <= 122) or $c == 95 or $c == 45
          then [$c]
          else [61, (($c / 16 | floor) | _hexd), (($c % 16) | _hexd)] end)
  | add // []
  | implode;
def gh_dec:
  gsub("=(?<h>[0-9a-f]{2})"; (.h | explode | map(if . >= 97 then . - 87 else . - 48 end) | .[0] * 16 + .[1]) | [.] | implode);
