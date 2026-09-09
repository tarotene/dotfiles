# public-publish-guard — 会社/private リポジトリの実名漏洩を防ぐ PreToolUse hook

## 背景

PUBLIC な `tarotene/dotfiles` に、会社(org)の private リポ名と tarotene
所有の private リポ名が、PR 本文・Issue 本文・コミット済みファイルの
複数箇所にわたって混入した(2026-09-10)。git 管理下のファイルに残った分は
`docs/adr/0004-repo-identity-and-relocation.md` と同じ clean orphan history
の手法で main の履歴ごと除去したが、履歴書き換えは事後対応であって再発防止
にはならない。人手のレビューに頼らず、機械的に検査する PreToolUse hook が
必要だった。

## なぜ2段階判定(deny/ask)なのか

denylist と一致すればすべて deny、という単純な設計は現実に合わない。
会社の org 名は、`home/identities/company.nix` の git 識別情報用メール
アドレスのように、ユーザーが意図的に non-secret として公開している文脈にも
部分文字列として出現しうる。これを一律 deny すると、正当な既存内容まで
毎回ブロックされてしまう。

一方で、`org/repo` という**組織スコープ付きの参照**(URL やドキュメント内の
言及)や、denylist に載った**具体的なリポジトリ名**(裸の単語一致)は、
それが出現すること自体がほぼ確実に意図しない漏洩であり、曖昧さの余地が
ない。

そこで判定を2層に分けた:

| 一致するもの | 判定 |
|---|---|
| `<org>/`(スラッシュ付き、URL 含む) | `deny` |
| denylist 上の具体的なリポ名(裸の単語一致) | `deny` |
| denylist 上の org 名の裸の単体出現(スラッシュなし) | `ask` |

`ask` は Claude Code の `permissionDecision` としてこのリポジトリで初めて
使う値。deny 一辺倒だと `company.nix` のような正当な既存内容が今後も
毎回引っかかり続けるが、ask ならその場で人間が「意図した内容か」を判断
できる。

## なぜ denylist はローカル専用なのか

denylist の中身(会社の org 名・private リポ名)自体が、まさにこの hook が
守ろうとしている情報そのものである。dotfiles にコミットしたら本末転倒
なので、`scripts/github-audit-rulesets` の `overrides.tsv` と同じ
「ツールは公開・データはローカル」原則に従う:

- `$XDG_CONFIG_HOME/public-publish-guard/orgs.txt` — 1行1 org 名(手動管理)
- `$XDG_CONFIG_HOME/public-publish-guard/repos.txt` — 1行1 `org/repo`
  (会社リポジトリなど、gh から一覧を引けない対象。手動管理)
- 自分(tarotene)の private リポ名は `gh repo list --visibility private` から
  live 取得し、`$XDG_STATE_HOME/public-publish-guard/private-repos-cache.json`
  に TTL 付きでキャッシュする。手動管理が要らない代わりに、直前に作った
  private リポは TTL の間は穴になる(`--refresh-cache` で手動更新可能)。

## 判定フロー

1. **高速フォールスルー**: stdin の `tool_input.command` に `git push` /
   `gh pr|issue create|edit|comment` の気配が無ければ、jq すら呼ばず即
   `exit 0`。
2. `PUBLIC_PUBLISH_GUARD_ALLOW=1` なら即 `exit 0`(`GIT_ALLOW_MAIN_COMMIT=1`
   の命名慣習に合わせた意識的 bypass)。
3. 対象リポジトリの可視性を判定する(`cmd` 中の `--repo`/`-R` を優先、
   無ければ cwd の `origin` リモートから導出)。`PRIVATE`/`INTERNAL` と
   判明した場合のみ検査をスキップする。**判定できない場合は PUBLIC と
   みなして検査を続ける**(安全側 fail — 今回の事故の再発防止という目的上、
   判定不能を無検査の理由にしない)。
4. スキャン対象を集める:
   - `gh pr|issue create/edit/comment` → `tool_input.command` 全体
     (heredoc 本文もそのまま含まれる)。
   - `git push` → `resolve_default_branch`(`scripts/git-audit-worktrees`
     と同じ idiom)で解決した default branch との `merge-base` を基点に、
     `git diff` と `git log --format=%B` を連結。
5. denylist と突き合わせ、deny/ask/フォールスルーを決める。

## 監査用サブコマンド

`--audit [path...]`(既定: `git ls-files` 全体)と `--audit --remote`
(open な Issue/PR の title/body も対象)。ライブフックと同じ
`match_verdict` を使うので、判定基準が二重管理にならない。既存漏洩の
是正がやり切れているかの確認に使う。

## 既知の限界

- `gh api -f body=...` のような生の API 呼び出しは対象外。コマンド文字列に
  `gh pr|issue create/edit/comment` という形が現れることを前提にしている。
- private リポ一覧のキャッシュ TTL(既定 86400 秒)の間に作った新しい
  private リポは検査対象に入らない。
- 新規ブランチの初回 push で `origin/<default>` をローカルにまだ fetch
  していない場合、`merge-base` が解決できず検査自体が黙って空振りする。
- 脅威モデルは敵対的入力ではなく Claude 自身が生成するコマンド
  (git-worktree-allow.sh / git-stash-guard.sh と同じ前提)。
