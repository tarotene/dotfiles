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
**文が 2 つ以上あり、そのどれかが除外パターンに当たれば deny** する。

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
- ダブルクォート内のコマンド置換(`"$(gh pr list)"`)と、`bash -c '…'` の中身は見ない。
- リダイレクト(`> file`)は連結に数えない。
- 連結を書けない形にはできない(検出のみ)。実行前に弾いて、書き直す往復を 1 回に抑えるところまでが責務。

## 逃げ道

- `dangerouslyDisableSandbox: true` の呼び出しは、sandbox の外で動くので止めない(`tool_input` にその欄が届く
  ことは、実装時に読んだ範囲では未確認。届かない場合は次の語を使う)。
- 連結が要る正当な理由があるときは、コマンドに `SANDBOX_COMPOUND_GUARD_ALLOW=1` の語を足す
  (`SANDBOX_COMPOUND_GUARD_ALLOW=1 gh a; echo x`)。環境変数でも同じ。値は `1` だけ。コマンドに残るので、
  あとから読める。

## 発火範囲

全リポジトリ。

## 執行点

- `crates/sandbox-compound-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `home/modules/claude.nix`
