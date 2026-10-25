# dotfiles-identity — 機体の identity とログイン中の Claude アカウントの一致を確かめる

`dotfiles-identity verify` は、この機体の宣言(`DOTFILES_IDENTITY`)と、ログイン中の Claude
アカウントのメールアドレスが一致したときだけ、identity(`personal` / `company`)を標準出力に
1 行出して終了コード 0 で終わる。それ以外は、理由を標準エラー出力に出して終了コード 1 で終わる。
引数の誤りは終了コード 2。

## なぜ要るか

1 つの外部アカウント(1 つの GitHub アカウントなど)を、会社と個人の両方の機体で使うと、
「この機体は、どちらの側として操作してよいか」を、ツールが機械的に決める必要がある。機体の
宣言だけに頼ると、宣言と実際のログインが食い違ったとき(個人の機体に会社のアカウントで
ログインしているなど)に気づけない。そこで、独立した 2 つの事実の一致を要件にする。

## 入力

| 入力 | 出どころ | 内容 |
|---|---|---|
| `DOTFILES_IDENTITY` | nix の `dotfiles.identity.name` | `personal` か `company`(閉じた集合) |
| `DOTFILES_CLAUDE_ACCOUNT` | nix の `dotfiles.identity.claudeAccount` | `exact:<メール>` か `domain:<ドメイン>` |
| `oauthAccount.emailAddress` | `$CLAUDE_CONFIG_DIR/.claude.json`、無ければ `$HOME/.claude.json` | ログイン中の Claude アカウントのメール |

最初の 2 つは `home/modules/identity.nix` が `home.sessionVariables` に出す。正本は
identity の層(`home/identities/{personal,company}.nix`)の `dotfiles.identity` の 1 か所で、
option に既定値は無い(宣言を忘れた層は、nix の評価で失敗する)。

## 判定

- `exact:<メール>`: メールが完全一致(大文字小文字は区別しない)。
- `domain:<ドメイン>`: `@` より後ろが完全一致。サブドメイン(`sub.example.com`)と、
  末尾だけ一致する別ドメイン(`evil-example.com`)は一致しない。ローカル部が空のものも一致しない。

次のいずれでも失敗する(閉じる側): `DOTFILES_IDENTITY` が無い・集合に無い、`DOTFILES_CLAUDE_ACCOUNT`
が無い・形が違う、設定ファイルが読めない・JSON でない、`oauthAccount.emailAddress` が無い、不一致。
エラー文にメールアドレスは含めない。

## 既知の未確認

- 会社の機体で使っている Claude アカウントのメールが、`company` の期待(会社のメールドメイン)に
  合うかは、まだ確かめていない。合わなければ、その機体では `verify` が失敗する(閉じる側)。
  確かめたら、この節を消し、`home/identities/company.nix` のコメントも直す。
- `CLAUDE_CONFIG_DIR` を設定した場合の設定ファイルの場所(`$CLAUDE_CONFIG_DIR/.claude.json`)は、
  この機体では `CLAUDE_CONFIG_DIR` が未設定のため、実機では確かめていない(単体テストで場所の
  決め方だけを固定している)。

## 使う側

private なリポジトリの手順が、GitHub 通知のような共用アカウントの操作の前にこれを呼び、
失敗したら何にも触れずに止まる。使う側の名前や手順は、この公開リポジトリには書かない(ADR-0034)。

実装: `crates/dotfiles-identity`(`pkgs.dotfiles-tools` 経由で `~/.local/bin/dotfiles-identity`)。
