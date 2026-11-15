# repo-scope-guard — 個人の契約の機体は、別の側の持ち主の非公開リポジトリを読まない

設計の正本は `docs/adr/0000-repo-scope-by-machine-contract.md`。この文書は、
`crates/repo-scope`(判定の規則)と `crates/repo-scope-guard`(`gh` のコマンドの解析と拒否)の
中身、見ないもの、運用をまとめる。共有の規範の節は `config/agents/AGENTS.md`「機体の契約の外にある
非公開リポジトリを読まない」。

## 規則

| 機体の契約 | 同じ側の持ち主 | 別の側・表に無い持ち主 |
|---|---|---|
| 個人 | 公開・非公開とも可 | 公開だけ可(非公開、公開と確かめられないものは不可) |
| 会社 | 可 | 可 |

理由は契約の非対称である。個人の契約は、入力を学習に使わないことを厳密には保証しない。会社の契約は保証する。
公開リポジトリは、持ち主を問わず気にしなくてよい。

- **側**: `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/github-owners.toml` の `[owners]`(GitHub のログイン名 →
  `personal` / `company`。大文字小文字を区別しない)。公開側の `home/modules/github-owners.nix` が
  `dotfiles.github.owners` として宣言し、値は非公開の設定リポジトリが入れる(ADR-0034。このリポジトリに
  持ち主の名前を書かない)。
- **機体の契約の側**: `dotfiles-identity verify` が通って `company` のときだけ会社。通らない・宣言が無い・
  Claude 以外(Codex・Copilot)は個人として扱う。
- **表のファイルが無い**機体は、規則を配っていないので何も制限しない。**あるのに読めない**ときは、`gh` を止める。
- 公開かどうかは `gh api repos/<o>/<r> --jq .visibility`。結果は
  `${XDG_STATE_HOME:-$HOME/.local/state}/claude/repo-scope/visibility.json` に 1 日だけ写す。引けなかった
  もの(404・権限・ネットワーク)は、公開と確かめられないので閉じる。同じ側の持ち主と、会社の契約の機体は、
  照会しない。

## 拒否するもの(Bash の `gh`)

コマンド位置(先頭・区切りの直後・`NAME=値` の直後)にある `gh` を、トークン単位で見る。クォートの中の文言・
heredoc の本文には当たらない。`bash -c '…'` の中身は再帰して見る。トークン化できないコマンドは通す(fail-open)。
ヘルプの表示(`--help`)は読まないので通す。

| 形 | 判定 |
|---|---|
| `-R/--repo o/r`、引数の URL(`https://github.com/o/r/…`)、`gh repo <verb> o/r`、`gh api repos/o/r/…` | リポジトリ単位(同じ側なら可、そうでなければ公開のときだけ可) |
| `gh api repos/{owner}/{repo}/…`、リポジトリを指さない `pr`・`issue`・`run`・`release` など | cwd の `origin` のリポジトリ(git の外・origin が無いときは通す) |
| `gh search …`・`gh api search/…` | `--visibility public`・検索語 `is:public` があれば可。無ければ、`--owner`・`--repo`・`user:`・`org:`・`repo:` のすべてが読んでよいものであること。絞りが無ければ拒否 |
| `gh repo list <持ち主>`・`gh api orgs/<持ち主>/…`・`gh status` | 持ち主の単位。公開だけに絞れないので、同じ側の持ち主だけ可(`gh status` は `--org <同じ側>` が要る) |
| `gh api user/repos`・`user/issues`・`issues`・`notifications` | すべての持ち主を横断するので拒否 |

`@me`(ログイン中のアカウント自身)は、個人の持ち主として可。拒否の文には、理由と、公開だけに絞る書き方が付く。
**逃げ道の環境変数は置かない**(`agent_session::escape` の表に載せない)。規則に例外が要るときは、持ち主の表を直す。

## 見ないもの(共有の規範の節が受け持つ)

機械で止まらない経路。ここを使って別の側の非公開を読まない責任は、規範の節にある。

- `gh api graphql` の本文(検索の語や対象リポジトリを解析しない)。
- ブラウザ操作(Playwright)、WebFetch、`curl` で GitHub を直接叩く経路。
- `GH_REPO` などの環境変数、`gh` を包む別の道具。
- `gh` を使わずに、ファイルシステム上のクローンを読む操作(`Read` など)。
- Bash 以外のツール(GitHub を呼ぶ MCP は、いまこの環境に無い。足すときは、この規則の対象に加える)。

## 自分の PR を一覧する道具

自分の open PR の汚れを見つける `pr-hygiene-check`(次の段で足す)のように、持ち主を横断して数える道具は、
母集団を `Policy::same_side_owners()` だけで作る。別の側の持ち主には、拒否に頼らず、最初から問い合わせない。

## 運用

- 持ち主を足す(新しい組織・他人の OSS): 非公開の設定リポジトリの持ち主の表に 1 行足して、`hms` で配る。
  机の段 3a が、表に無い持ち主を本人に聞く(ADR-523)。
- 配備の順序: この規則が `main` に入っても、非公開の設定リポジトリが `dotfiles.github.owners` を入れるまでは、
  ファイルが無いので働かない。入れた後は、`~/.config/dotfiles/github-owners.toml` が配られ、次のセッションの
  `gh` から効く。
- 確かめ方(個人の契約の機体): 絞りの無い `gh search prs --author @me --state open` が拒否されること。
  `--visibility public` を付けると通ること。

## 既存手段と代替

- 既存手段: 拡張 — `crates/hook-router/src/rules.rs`(規則の登録)、`crates/github-command`(`gh` の引数と URL の
  解析)、`crates/dotfiles-identity`(機体の契約の側)。
- 外した候補: 外部の検査(bleep)の持ち主の denylist に会社側の持ち主を足す案 — 名前の一致で止めるので、公開の
  会社リポジトリまで止めてしまう(裁定は、公開は読んでよい)。
