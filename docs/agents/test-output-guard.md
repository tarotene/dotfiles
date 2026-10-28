# test-output-guard — テストの出力を `head` / `tail` で切る形を、実行前に止める

実装: `crates/test-output-guard`(Rust)。`hook-router` の規則として走る(`crates/hook-router/src/rules.rs`、
`tools: Bash`)。エージェントの種類は問わない(`ci-wait-guard` と同じく、`Bash` のコマンド文字列だけを見る)。
Issue: No-Issue(wrap-up の気づき「`cargo test … | head -10` で出力を切って、reconcile の fixtures テストの失敗を
見落とし、落ちるテストを PR #1199 に載せた」。文章の規範は無く、検査も無かった)

## 動機

`cargo test -p x 2>&1 | head -10` は、失敗を示す行(`FAILED`・`panicked`)が先頭 10 行の外に出ると見えない。
`cargo test` は成功したテストも 1 行ずつ出すので、テスト数が多いほど失敗の行は後ろに回る。さらにパイプの終了コードは
最後のコマンド(`head`)のものなので、失敗がシェルからは成功に見える。2026-10-28 に、これで落ちるテストを
PR #1199 に載せ、後で気づいて直して積み直した。

同じ型の事例(文章の規範はあったが、1 セッションで 5 回以上破られた)に、sandbox の外で動かすコマンドの連結
(`docs/agents/sandbox-compound-guard.md`、#1164)がある。そこで入れた「トークン単位で見て、実行前に deny して
案内する」検査と同じ形を使う。

## 判定

コマンド文字列をトークン化し、区切りで「コマンド」に分ける。**単独の `|` でつながった連鎖の中で、`cargo test`
(`cargo t`・`cargo nextest`、`+toolchain` やフラグが前に付いても同じ)から、`tee` / `cat` だけを挟んで `head` /
`tail` に届く形**を deny する。

- 区切りは 1 文字ずつ別のトークンになる(`&&` は `&` `&`、`||` は `|` `|`)。前後が `|` でない `|` だけをパイプと数える。
  `2>&1`・`>&2` の `&` と、`|&`(標準エラーも流すパイプ)の `&` は区切りではない。
- 間に `grep`・`rg`・`awk` などの絞り込みが入れば通す。それは「必要な行を拾う」形で、切るのではない
  (`cargo test 2>&1 | grep -E 'FAILED|test result' | head -5`)。
- `;`・`&&`・`||` の後ろの `head` は連鎖の外なので通す(`cargo test; head -5 log`)。
- `bash -c '…'` の中身は再帰して見る(深さ 2 まで)。

拒否の文は、失敗の行が見えなくなること・終了コードが `head` / `tail` のものになることを説明し、絞るなら
`| grep -E 'FAILED|panicked|test result'`、またはファイルに書いて(`> $TMPDIR/test.log 2>&1`)終了コードを別に確かめるよう
案内する。

### 限界

- `cargo test` 以外(`cargo build | head`、`pytest | head`)は対象外。再発したら表に足す。
- `cargo` を別のコマンドが包む形(`nix develop --command cargo test | head`、`make test | head`)は見ない。
- ダブルクォート内のコマンド置換(`"$(cargo test | head)"`)は見ない。
- 切る形を書けなくはできない(検出のみ)。実行前に弾いて、読み直す往復を 1 回に抑えるところまでが責務。

## 逃げ道

切り詰めが要る正当な理由があるときは、コマンドに `TEST_OUTPUT_GUARD_ALLOW=1` の語を足す
(`TEST_OUTPUT_GUARD_ALLOW=1 cargo test | head`)。環境変数でも同じ。値は `1` だけ。コマンドに残るので、
あとから読める。

## 発火範囲

全リポジトリ。

## 執行点

- `crates/test-output-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `pipeline-map.toml`
