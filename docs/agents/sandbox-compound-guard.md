# sandbox-compound-guard — sandbox の外で動かす道具の連結を、実行前に止める

実装: `crates/sandbox-compound-guard`(Rust)。`hook-router` の規則として走る(`crates/hook-router/src/rules.rs`、
`tools: Bash`)。Claude Code の sandbox にだけ当たる検査なので、他のエージェント(Codex・Copilot)では何もしない。
Issue: No-Issue(wrap-up の気づき「`gh` を `;` や `|` やループの中で呼ぶと sandbox の中に残って 401 になる失敗を、
このセッションで 5 回以上繰り返した」。文章の規範が漏れた)

## 動機

`sandbox.excludedCommands`(`gh *`・`git commit *`・`git add *`・`nix build *` など。正本は `home/modules/claude.nix`)は、
鍵束・gpg-agent・nix daemon が要る道具を sandbox の外で動かす。ただし、**呼び出し全体のコマンドが除外パターンに
合わないと、全体が sandbox の中に残る**(Anthropic, "Sandboxing"、<https://code.claude.com/docs/en/sandboxing>、
取得 2026-10-27)。連結した呼び出しは次のように失敗する(2026-10-28、vega で実測。#1164)。

| 呼び出し | 失敗 |
|---|---|
| `gh auth status; echo "exit=$?"` | `gh` が認証できず HTTP 401 |
| `git add x && git commit -m …` | 署名が `No secret key` |
| `nix build … \| tail` | `Read-only file system` |

`config/claude/CLAUDE.md` の「sandbox の外で動かすコマンドは、単体で呼ぶ(#1164)」は同じことを書いていたが、
文章の規範は 1 セッションで 5 回以上破られた。この検査は、連結を実行前に止める。

## 判定

コマンド文字列をトークン化し、区切り(`;`・`&`・`|`・改行など `command-inspection` の `is_sep`)で文に分ける。
**文が 2 つ以上あり、そのどれかが除外パターンに当たれば deny** する。文が 1 つのときは、下の「前置」と「コマンド置換」の場合だけ deny する。

- 除外パターンは、実行時に `~/.config/agent-native/claude-permissions.json` の `sandbox.excluded_commands` から読む
  (home-manager が `home/modules/claude.nix` の宣言から配備する写し)。一覧をこの crate に持ち込まないので、
  除外の宣言を変えても検査の更新は要らない(単一正本)。ファイルが無い・読めない・空なら何も止めない。
- パターンの当たり方: 末尾が `*` なら、それより前の語が文の先頭と一致すれば当たる(`gh *` は `gh pr list` に、
  `git commit *` は `git commit -m x` に)。`*` で終わらないパターンは文全体の完全一致。先頭の `VAR=value`・`env`・
  `do`/`then`/`!` などの予約語は読み飛ばす(`for n in 1 2; do gh pr view $n; done` の `gh` を拾う)。
- 拒否の文は、除外対象のコマンド名(`gh`・`git add`・`nix build`)を挙げ、「単体で呼ぶ」「出力の加工は `gh … --jq` の
  ように道具側の機能を使う」「複数のコマンドは別々の Bash 呼び出しに分ける」を案内する。

検出はトークン単位で、クォートされた文字列は 1 トークンになるので、`git commit -m "a; b"` や `echo "gh pr list | head"`
には当たらない。heredoc の本文は見ない。トークン化できないコマンドは判定不能として通す(fail-open)。

### 限界

- `git -C <dir> commit` のように、除外パターン(`git commit *`)に当たらない単体の呼び出しは、この検査の対象外
  (連結ではないため)。文章の規範(`config/claude/CLAUDE.md`)が引き続き担う。
- プロジェクトのディレクトリと追加したディレクトリの外にある worktree では、シェルの cwd が呼び出しごとに戻る
  (Claude Code の仕様。Tools reference の "What persists between commands"、
  <https://code.claude.com/docs/en/tools-reference>、取得 2026-11-05)。`cd <dir> && git …` は連結なので sandbox に残り、
  署名が失敗する。この検査は拒否文で単体で呼ぶよう案内するが、この場合は単体では呼べない。正規の逃げ道は、
  `SANDBOX_COMPOUND_GUARD_ALLOW=1` を足し、承認を得て `dangerouslyDisableSandbox` で呼ぶこと(下の「逃げ道」)。
  別の worktree を `/add-dir` に足して cwd を保つ手順は、sandbox の書き込みが追加ディレクトリまで広がるかが未確認
  なので、書かない。
- `bash -c '…'` の中身は見ない。ダブルクォート内のコマンド置換の中身(`"$(gh pr list)"` の `gh pr list`)を別の文として数えないが、
  置換があること自体は下の「コマンド置換」で止める。
- リダイレクト(`> file`)は連結に数えない。
- 連結を書けない形にはできない(検出のみ)。実行前に弾いて、書き直す往復を 1 回に抑えるところまでが責務。

## 逃げ道

- `dangerouslyDisableSandbox: true` の呼び出しは、sandbox の外で動くので止めない(`tool_input` にその欄が届く
  ことは、実装時に読んだ範囲では未確認。届かない場合は次の語を使う)。
- 連結が要る正当な理由があるときは、コマンドに `SANDBOX_COMPOUND_GUARD_ALLOW=1` の語を足す
  (`SANDBOX_COMPOUND_GUARD_ALLOW=1 gh a; echo x`)。値は `1` だけ(呼び出し単位)。コマンドに残るので、
  あとから読める。人は、起動前に環境変数 `SKIP_SANDBOX_COMPOUND_GUARD=1` を設定してもよい(セッション単位)。名前と真偽は
  `agent_session::escape` の表が決める。

## 前置(文が 1 つの呼び出し)

連結でなくても、先頭の `VAR=value` が呼び出しを sandbox の中に残すことがある。2026-11-05、ある private リポジトリで
`GIT_EDITOR=true git rebase --continue`(単体)が sandbox の中で動き、署名が `No secret key` で失敗した。

- 根拠: 除外パターンは `Bash(...)` の権限規則と同じ書式で、呼び出しの文字列に合わせる(Anthropic, "Sandboxing"、
  <https://code.claude.com/docs/en/sandboxing>、取得 2026-11-05。逐語: 「Claude Code matches the text of the call」)。
  権限規則の照合は、既知の安全な環境変数の前置だけを剥がす。逐語: 「Claude Code also strips a leading assignment of
  certain known-safe environment variables, so `Bash(npm test *)` matches `NODE_ENV=test npm test`. An allow rule won't match
  past an assignment of any other variable.」(Anthropic, "Configure permissions"、Wrappers の節、
  <https://code.claude.com/docs/en/permissions>、取得 2026-11-05)。安全な変数の一覧は文書に無い。除外パターンの照合が
  同じ剥がし方をするとも、文書に書かれていない。ここは今回の失敗から推論している(`GIT_EDITOR` は一覧に無い)。
- 一覧: Claude Code 2.1.285 の実行ファイルにある集合(`GOOS`・`NODE_ENV`・`LANG`・`TZ`・`CI` など 39 個。実行ファイルは
  その外を "an environment variable prefix outside the safe list" と呼ぶ)を `SAFE_ENV` に写した。版が変わると食い違い
  うる。一覧に無い変数は止め、一覧にある変数は止めない(一覧が古くて余分に止めても、前置をやめるだけで済む)。
- 判定: 前置を全部剥がすと除外パターンに当たるのに、安全な変数だけを剥がしたのでは当たらない文を deny する。`env` も
  剥がされるラッパーの一覧(`timeout`・`time`・`nice`・`nohup`・`stdbuf`・`command`・`builtin`・`noglob`)に無いので同じ。
- 案内: 「前置せず単体で呼ぶ」「常に要る変数は設定の `env` に宣言する」(`dotfiles.claudePermissions.env`、
  [agent-permissions](agent-permissions.md))。`GIT_EDITOR=true` は Claude Code が Bash の環境に既に入れているので、前置は
  要らない。`git -c core.editor=true rebase` のように `-c` で渡す形も、`git rebase *` は呼び出しの先頭の語に合わせる
  ので合わず、sandbox の中に残ると推論する(`git -C <dir> commit` が合わないことは実測済み。`home/modules/claude.nix` の
  `excludedCommands` の注記。`-c` の形そのものは未実測)。この形は検出しない(拒否文で触れるだけ)。
- 限界: 変数を `export` した別の呼び出しや、`bash -c` の中身は見ない。前置が要る正当な理由があるときは、
  `SANDBOX_COMPOUND_GUARD_ALLOW=1` を足す(上の「逃げ道」)。

## コマンド置換(文が 1 つの呼び出し)

除外パターンに当たる単体の呼び出しでも、引数にバッククォートか `$(` があると、呼び出し全体が sandbox の中に残る。
`git commit -m` に本文のバッククォートを含めたら、署名が `No secret key` で失敗した(2026-11-09)。

- 実測(2026-11-10、vega。読み取りだけの `gh api user --jq .login -H '<値>'` を、1 回ずつ単体で呼んだ。sandbox の外
  なら `tarotene`、中に残れば HTTP 401 になる):
  | 引数の値 | 結果 |
  |---|---|
  | (付けない) | 外(`tarotene`) |
  | `'X-Note: a` + バッククォート + `b'`(単引用符) | 中(401) |
  | `"X-Note: $(echo hi)"` | 中(401) |
  | `"X-Note: $(cat <<'EOF'` + 改行 + `hi` + 改行 + `EOF` + 改行 + `)"`(heredoc 形) | 中(401) |
  | `'X-Note: $(x)'`(単引用符の中の文字そのもの) | 中(401) |
  | `'X-Note: $HOME'`、`"X-Note: a\\nb"` | 外 |
  つまり、引用の種類に関係なく、バッククォートと `$(` という文字列があるかどうかで決まる。変数(`$VAR`)とバックスラッシュ
  は影響しない。照合は呼び出しの文字列を見ていると推論する(Anthropic の文書は「Claude Code matches the text of the
  call」。上の「前置」の根拠)。
- 判定: 除外パターンに当たる文のトークンに、バッククォートか `$(` があれば deny する(引用の外の `$(` は `(` で区切られる
  ので、直前が `$` で終わるトークンと `(` の並びも数える)。heredoc の本文は見ない(`git commit -F - <<'EOF'` の本文の
  バッククォートが sandbox に残すかは未実測)。以前の試験が「通す」と固定していた `-m "$(cat <<'EOF' …)"` は、実測で
  sandbox に残ると分かったので、deny に改めた。
- 案内: 本文は Write でファイルにして `git commit -F <絶対パス>`、`gh … --body-file <絶対パス>` で渡す。値は、コマンド置換
  でなく、先に読んだ文字列を直接書く。

## 発火範囲

全リポジトリ。

## 執行点

- `crates/sandbox-compound-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `home/modules/claude.nix`
