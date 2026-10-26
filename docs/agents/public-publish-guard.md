# public-publish-guard — 上流リポジトリへの配線(dotfiles 固有部分)

ファイル名は当時(ADR-0009)の名残のまま残す(ADR-0007 の遡及的な一括
リネーム禁止と同型 — このドキュメント自身の識別子として、19 ファイル
以上から相対パスで参照されている)。上流リポジトリは 2026-09-24 に
`publish-guard` から `bleep` へ改名した(tarotene/publish-guard#28)。
以下は改名後の現状を記述する。

設計根拠・判定ロジック・denylist/allowlist の仕組み・permission boundary
ではないことの説明は、上流リポジトリ
[tarotene/bleep](https://github.com/tarotene/bleep)(旧
`tarotene/publish-guard`)の README が正本(ADR-0008 決定1: 実装物の
利用者向け説明は実装物のリポジトリに置く)。ここに書くのは **dotfiles
側の配線だけ**。

なぜ別リポジトリに切り出したかの決定は `docs/adr/0009-publish-guard-
upstream-split.md` を参照(ADR ファイル名・本文は改名に追従させない、
D7 と同型)。

## dotfiles での配線

- `flake.nix` が `bleep` を `flake = false` の入力として commit SHA に
  pin する(`tarotene/bleep` は独立に semver/tag を持てるが、
  `tarotene/dotfiles` 自身は ADR-0004 の「No semver releases」のまま —
  だからこそ別リポジトリにした)。
- `home/modules/agents.nix` は上流のツリー全体を `home.file
  ".claude/hooks/bleep"` として**1つのディレクトリ symlink**で配る
  (個々のファイルを列挙しない)。上流の shim(`hooks/bleep.sh`)は
  `realpath "$0"` で自己解決するため(旧 `claude-adapter.sh` と違い
  `CLAUDE_PLUGIN_ROOT` の明示注入は不要)、ツリー構造をそのまま保てば
  それ以上の配慮は要らない。
- 登録する command:
  ```
  BLEEP_HOOK_BIN='$HOME/.claude/hooks/bleep-hook' BLEEP_LEX_BIN='$HOME/.claude/hooks/bleep-hook' bash '$HOME/.claude/hooks/bleep/hooks/bleep.sh' --host=claude
  ```
  旧 `claude-adapter.sh` は plugin マーケットプレイス経由のインストール
  時に Claude Code が自動設定する `CLAUDE_PLUGIN_ROOT` に依存していたため、
  `home.file` による直接配備ではこの env var を明示的に前置する必要が
  あった。`hooks/bleep.sh`(#25-28 の Rust hook cutover で旧 3 adapter を
  統合した単一 shim)は自己解決するため、この配慮そのものが不要になった。
  代わりに BLEEP_HOOK_BIN/BLEEP_LEX_BIN の2変数を前置する(#561):
  bleep(bash 本体)が呼ぶ Rust バイナリの継ぎ目は2箇所あり
  (`hooks/bleep.sh` の adapter 起動と、`bleep` 本体の
  `cmd_scan_bash_command` が使う字句解析)、どちらも既定では
  `~/.cargo/bin/bleep-hook`(手動 `cargo install` 経由、pin された
  `bleep` flake input とは無関係に更新される)にフォールバックしうる。
  `flake.nix` の `bleepHookOverlay` が同じ `bleep` input から crane で
  ビルドし `~/.claude/hooks/bleep-hook` に配備した実体を両変数で指すことで、
  bash 本体と Rust バイナリが常に同一 pin から導出されるようになり、
  手動 install 経由の版ずれが構造的に起きなくなる。
- matcher は複合1本 `"Bash|mcp__.*"`。`register()`(この nix ファイル内)の
  存在判定は command 文字列の完全一致だけで matcher を見ないため、Bash と
  MCP を2つの hook エントリに分けると2回目の登録が早期 return し、MCP
  経路が無検査のまま残る(旧実装が matcher `"Bash"` 単体だったために持って
  いた最大の機能欠陥そのもの)。
- 旧 command 3 世代分が `retiredHookEntries` で完全一致削除される:
  `bash '$HOME/.claude/hooks/public-publish-guard.sh'`(ADR-0009 分離前)、
  `CLAUDE_PLUGIN_ROOT=... bash '.../publish-guard/hooks/claude-adapter.sh'`
  (#25-28 の Rust hook cutover前)。matcher/timeout は既存エントリの
  command が変わらない限り更新されない(この nix ファイル内 `register()`
  のコメント参照)。
- Codex CLI(`~/.codex/hooks.json`)/ Copilot CLI(`~/.copilot/settings.json`)
  にも同じ shim を配線する(#160)。旧 `adapters/{codex,copilot}-adapter.sh`
  の command は register-{codex,copilot}-hooks に追加した `--retire` で
  完全一致削除してから新 command(`hooks/bleep.sh --host={codex,copilot}`)
  を登録する — Claude 側の `retiredHookEntries` に相当する仕組みが元々
  無かったため、改名 companion PR でこの2スクリプトに追加した。
- `config/git/hooks/pre-push`(`core.hooksPath` で全リポジトリ共通、
  `docs/git-sync.md`)が `BLEEP_HOST=git-pre-push` を付けて `scan-push` を
  呼ぶ(#196。`BLEEP_HOST` は ADR-478 の判定レッジャーが呼び出し元を
  区別するためのタグで、判定そのものには影響しない)。エンジン不在は
  黙ってスキップ(ADR-0005)。exit 1(ask)/2(deny)のどちらも push を
  block する(pre-push に対話の経路が無いため)。回避は `BLEEP_ALLOW=1
  git push` — ただし block 時に pre-push が表示する stderr にはこのコマンド
  を書かない(上流 bleep の「deny 理由に bypass を書かない」方針、
  README「Bypass: `BLEEP_ALLOW=1`」参照: 制約対象がその場で読む理由文に
  出すと、読んだそばから bypass を打ててしまう)。回避手段の存在自体は
  この文書のように能動的に探さないと出てこない場所にだけ書く。
  PRIVATE/INTERNAL リポジトリの自動スキップは engine 側の判定。

## commit 時の検査(#947・#949)

push 時の `pre-push` は最後の砦で、止まったときには commit が既にある(作り直しの往復が
増える)。`config/git/hooks/commit-msg` が、commit のメッセージとステージ済みの追加行を、同じ
denylist と commit 時に照合する。`pre-commit` は同じ実体を `commit-msg --staged` で呼び、
エディタを開く前にステージ済みの追加行だけを検査する。エージェントの経路(Edit・Write・Bash)
には依存せず、`git commit` を通るものすべてに効く。

- **既存手段: 採用**。denylist の照合・可視性の判定・判定レッジャー・理由文は、bleep の CLI
  (`scan-push --pre-push`)をそのまま呼ぶ。bleep は commit の範囲を取るので、これから作る
  commit を使い捨ての commit(`git write-tree` + `git commit-tree`、ref なし)として
  `$TMPDIR` 配下の隔離した object ディレクトリに作り、本物の object store は alternate
  として読むだけにする。使い捨ての commit は hook の終了時に消える。denylist のロジックは
  このリポジトリに持たない。
- **公開リポジトリ以外では何もしない**。bleep の可視性判定(PRIVATE / INTERNAL は検査しない)
  をそのまま使う。さらに、`origin` が無いリポジトリ、bleep が未配備の機体、merge の途中は
  黙って通す(ADR-0005)。可視性を取れないときは、bleep の既定どおり公開として検査する。
- **メッセージ**は bleep の既存の形のまま: 語は出さず、`term_hash` と `bleep explain` の
  案内だけ。bleep が一致した commit として挙げる短い SHA は使い捨ての commit のものなので、
  その 1 文だけ落とす。bypass の手段は書かない(上流の方針、pre-push と同じ)。
- エディタが付ける `#` の行(ブランチ名・ファイル一覧)は、git が commit の前に落とすので、
  検査からも外す。
- この hook の下ごしらえ(`write-tree`・`commit-tree`)が失敗したときは、警告を出して通す。
  bleep の判定が ask のときは、pre-push と同じく止める(commit の hook には確かめる対話が無い)。
- 判定レッジャーの `host` は `git-commit`(pre-commit 経由も同じ。`docs/schemas/
  agent-verdict.schema.json`、`crates/verdict-escalate/src/record.rs`)。
- 所要時間: bleep の起動(パターン構築)が 1 回あたり 0.8 秒前後で、pre-commit と commit-msg で
  2 回走るため、commit 1 回に約 1.6 秒かかる(2026-10-27、この機体で実測。可視性の取得は
  bleep のキャッシュが効いている間は含まない)。
- 回避は `--no-verify` だが、`--no-verify` は pre-commit のチェーン先(他リポジトリの ruff・
  mypy など)も飛ばすので、使わない(`docs/git-sync.md`)。push 時の検査は残るので、
  commit 時の検査を飛ばしても最終的には pre-push が止める。

自己検査: `./config/git/hooks/commit-msg --selftest`(CI で実行する。bleep の代わりに
一時 HOME の置き換えを使うので、denylist も network も要らない)。

## `~/.config/bleep/` の実体(dotfiles には無い)

`bleep` の判定語彙(`orgs.txt`・`repos.txt`)と、CLI ツール名と衝突する
private リポジトリ名を通す `allow-stopwords.txt`(bleep README
「Marking a private repo as "prospectively public"」節が正本、
tarotene/bleep#15 → PR #19 で決着)は、いずれも実値を含むため PUBLIC な
この dotfiles リポジトリには置かない(ADR-0034)。3 ファイルとも private
wrapper flake が `~/.config/dotfiles/private-hub` マーカー経由で生成する
(`crates/hms` の `resolve_default_ref()` と同じ間接参照。ADR-0034
Decision 6 と同型 — 正本は wrapper flake 側、ここはスキーマ・手順だけを
記す)。生成されたファイルは home-manager の読み取り専用リンクなので、機体上で
直接は書けない。変えるときは wrapper の宣言を直す(`allow-stopwords.txt` も同じ。
#995)。`allow-stopwords.txt` の各行は「実際には CLI ツール名としての
言及であり private リポジトリ名との衝突ではない」と判断した文字列を、
コメントで理由を添えて 1 行ずつ宣言する(#431)。`hms .` でこの3ファイルが
撤去される事故は、ADR-0034 の 2026-09-27 Amendment により解消済み
(wrapper 登録ホストでのローカル適用は既定で wrapper 経由になる)。
別経路(マーカーが一度も設置されないまま素の `hms` を実行し、無警告で
public 単体に縮退する、#567)は 2026-09-29 Amendment のマーカー宣言化 +
降格 guard で対処済み。

### `allow-stopwords.txt` を手書きから wrapper の宣言へ移す(#995)

2026-10-24 に `ls -la ~/.config/bleep/` で読んだ vega では、`orgs.txt` と
`repos.txt`(と、次節の `allow-tools.txt`)が nix store へのリンクで、
`allow-stopwords.txt` だけが、2026-10-18 更新・52 バイトの普通のファイルだった。つまり
wrapper に宣言が無いまま、機体上で手で書かれていた。上の「wrapper の宣言を直す」
が実態になるよう、wrapper に宣言を足して手書きを置き換える。移行前の機体では、
手書きのファイルが唯一の正本なので、この節の手順が済むまで中身を消さない。

移行(機体ごとに 1 回。語の実値はこの文書にも、コマンドの出力にも出さない):

1. 手書きのファイルを退避する。`cp -p ~/.config/bleep/allow-stopwords.txt
   ~/allow-stopwords.before`。中身は表示しない(`cat` しない)。行数は
   `wc -l ~/allow-stopwords.before` で控える。
2. wrapper の宣言に、退避したファイルの行を移す。語とその理由コメントを 1 行ずつ、
   順序も保って移す(bleep README「Words excluded from the denylist」の書式)。
   宣言の置き場所と書式は wrapper 側が正本で、`orgs.txt`・`repos.txt` を宣言している
   箇所と同じ経路に足す。
3. `hms` で適用する。既存の普通のファイルは、`-b backup` により
   `allow-stopwords.txt.backup` へ退避されてからリンクに置き換わる。
   `.backup` が前回から残っていて `checkLinkTargets` が止まったら、
   [`docs/operations.md`](../operations.md) の「`hms` fails at `checkLinkTargets`
   with a `.backup` clobber error」に従う(wrapper 側のモジュールで
   `dotfiles.quarantine.managedFiles` に `.config/bleep/allow-stopwords.txt` を足せば、
   前回の残りごと先に退避される。その場合の退避先は `.pre-nix` 接尾辞で、
   `.backup` ではない)。
4. 移行できたことを読み戻す。`ls -l ~/.config/bleep/` で `allow-stopwords.txt` が
   nix store へのリンクになっていること、
   `diff ~/allow-stopwords.before ~/.config/bleep/allow-stopwords.txt | wc -l`
   が `0` であること(語と理由を落としていない。中身は出力に出ない)。
   どちらかが違えば、退避したファイルから宣言を直して、もう一度適用する。
5. 4 が通ってから、`~/allow-stopwords.before` と、適用で退避された手書きの
   ファイル(`allow-stopwords.txt.backup` または `.pre-nix`)を消す。

### `allow-tools.txt` はこのリポジトリで宣言する(tarotene/bleep#106)

`allow-tools.txt`(Bash 以外のツールのうち、repo-ref の検査から外すものを、ツール名の
拡張正規表現で 1 行ずつ宣言する。bleep README が正本)は、ツール名だけで実値を含まない
ので、上の 3 ファイルと違い PUBLIC なこのリポジトリの `home/modules/agents.nix` が
`xdg.configFile."bleep/allow-tools.txt"` で宣言し、全機体に同じ内容を配る。読み取り専用の
リンクになるので、機体ごとの手での追記は食い違いの元になるため行わず、宣言を変える。現在の
宣言は組織内 Slack の下書き(`mcp__.*__slack_send_message_draft`)だけで、送信のツールは
宣言しない(送信は `external-send-guard` が下書き止まりにする)。`.*` の部分はコネクタの
サーバー名で、機体ごとに違いうるため任せている。

## 未検証の前提

- Claude Code hook の `deny` が `bypassPermissions` 下でも効くかは公式
  ドキュメントで断定できていない(上流 README・ADR-0009 と同じ記録)。

## 一般語の一致で push が止まったとき(#733)

denylist には非公開リポジトリ名が登録され、その中に一般語と同じ綴りのものが
ある。空白で区切られた単語として追加行や commit メッセージに現れると、pre-push が
一致で止める(ハイフン・下線でつながった形、`automated-…-fixes` のようなものは
通る)。後から語が denylist に加わると、過去に通った文章も止まりうる。

- **エージェントの扱い**: 追加行と commit メッセージを、その語を単語として
  使わない言い換えに直して通す。止まった語は黙って言い換えるだけで終えず、
  本人に報告する。
- **本人の判断**: 語が一般語として使われる文書が今後も増えるなら、
  `allow-stopwords.txt`(bleep の README「Words excluded from the denylist」)に
  入れる。入れる先は wrapper の宣言で、機体上のファイルではない(リンクなので書けない。
  上の「`allow-stopwords.txt` を手書きから wrapper の宣言へ移す」が済んでいない機体
  だけ、手書きのファイルが正本)。これはガード設定の変更なので、エージェントは書き込まない。
  言い換えで運用し続けるか、登録するかを本人が決める。
