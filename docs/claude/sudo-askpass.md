# sudo-askpass — sudo パスワードを pinentry 経由で渡す

Claude Code(および Codex / Copilot CLI)の Bash ツール実行には制御端末がない。
`tty` は「not a tty」を返し、`/dev/tty` も開けない。そのため `sudo` を含む
コマンドは従来「a terminal is required to read the password」で即座に失敗し、
apt 層の適用(`scripts/install-packages.sh`)等の sudo 系統は人間が端末から
起動するしかなかった。

## 仕組み

sudo(8) は「端末がなく `SUDO_ASKPASS` が設定されている」場合、`-A` を付けなくても
自動的に askpass helper を使う(`SUDO_ASKPASS` — "used to read the password if
no terminal is available or if the -A option is specified" — sudo(8) manual,
https://www.sudo.ws/docs/man/sudo.man/、取得 2026-09-22)。したがって:

- **sudo にラッパーや alias は要らない**。`home.sessionVariables.SUDO_ASKPASS`
  を 1 本宣言するだけで、tty のない agent セッションだけが askpass 経路になり、
  対話端末(tty あり)は従来どおり端末プロンプトのまま — 切り替えは sudo 自身の
  tty 判定に委ねている。
- helper(`~/.local/libexec/sudo-askpass`、配備元 `scripts/sudo-askpass`)は
  sudo からプロンプト文字列を `$1` で受け取り(sudo.conf(5) の askpass 項の
  記述どおり)、`home/modules/gpg.nix` が選んだ pinentry パッケージに生の
  Assuan プロトコルで `GETPIN` を発行し、`D <pin>` 応答を percent-decode して
  stdout に返す。pinentry のバイナリ名は home-manager 自身の
  `services.gpg-agent.pinentry.program`(`meta.mainProgram` から解決済み)を
  再利用しており、Linux/darwin の分岐を自前で持つ必要がない
  (nix-community/home-manager `modules/services/gpg-agent.nix`)。

## なぜ既製の ssh-askpass ではなく pinentry ラッパーを自作したか

`ssh-askpass-gnome` 等を apt に足す経路は一見自然だが、システム層の定義
(ADR-0001: root・システムサービス・apt プロセスへのロード)には該当しない
単なる GUI ヘルパーであり、GPG と同じ入力面(pinentry-gnome3 の grab 挙動)に
揃えるほうが一貫する。既に gpg.nix が pinentry を選定しているので、それを
再利用するだけで新しい依存が増えない。

## なぜ毎回プロンプトか(sign-prewarm.sh と対照的)

`sign-prewarm.sh` は GPG の [S]/[E] パスフレーズをログイン後の安全な瞬間に
前倒しし、以後の入力をゼロにする。sudo-askpass はあえてその逆——**キャッシュも
prewarm も持たない**——を採る。理由は非対称性にある: askpass の GUI には
「どのコマンドが root で実行されるか」が一切表示されない(sudo が helper に
渡すのは汎用のプロンプト文字列のみ)。実行コマンドを目視できる関所は Claude
自身の Bash permission ダイアログにしかないため、`home/modules/claude.nix` の
`permissionRules` には sudo 系を一切足さず default ask のままにしてある。
askpass 側の入力(毎回)と permission 側の確認(コマンド単位)の**二段承認**を
維持することが、この機能をキャッシュしない理由そのものである。

sudo 自身の timestamp キャッシュも実質効かない — Claude の Bash ツール呼び出しは
毎回別プロセスなので、agent からの sudo は結局毎回 askpass を経由する。

## 変更しないもの

- `/etc/sudoers` / `/etc/sudoers.d`(システム層に触れない、timestamp_timeout 等も不変)
- `claude.nix` の `permissionRules`(sudo は暗黙 default ask のまま)
- `sign-prewarm.sh`(sudo の prewarm はしない)
- `gpg.nix`(pinentry 選択は参照するだけ)

## 既知の注意点: pinentry は「人間が入力した」ことを保証しない

この機能の設計を進める過程で、実機検証中に重大な事象が発生した: `pinentry-gnome3`
に生の Assuan `GETPIN` を送ったところ、GUI ダイアログへの人間の入力を一切介さず
実際の sudo ログインパスワードが返ってきた(利用者は GNOME keyring 機能を意図的に
使った記憶がないとのこと)。作業仮説は、PAM 連携で `login` keyring がログイン
パスワードで自動アンロックされており、pinentry-gnome3 / GCR 側の何らかの経路が
それを参照してしまった、というもの — 未確定(調査は wrap-up inbox 経由で別途
追跡)。

この事象は本機能の安全性の前提(「pinentry のプロンプトが出る = 人間が今その場に
いて操作した」)が、少なくともこのホストでは技術的に保証されないことを意味する。
そのため:

- **askpass だけを唯一の関所にしない** — 上記の二段承認(permission ask を残す)
  は、この既知の弱点に対する多層防御としても機能する。
- **検証は人間が自分の端末で行う** — Claude(や他の agent)の Bash 経由で実物の
  `sudo`/`pinentry`/`gpg-agent` を叩く形の動作確認は行わない。エージェントの
  ツール出力はこの会話のログに残るため、実パスワードが返ってきた場合にそのまま
  平文でログに固定されてしまう(実際に本セッションで 2 回発生した)。この文書の
  実装時点の検証はすべてスタブ pinentry(固定ダミー値を返すシェルスクリプト、
  `sign-prewarm.sh` の `gpg-stub` と同じ発想)でのみ行っている。

## 検証(スタブのみ)

`scripts/sudo-askpass --selftest`(CI にも組み込み済み)が、固定ダミー応答を
返す pinentry スタブだけを使って以下を検証する:

- `percent_escape`/`percent_decode` の往復、および `%0A` を「$(...) コマンド
  置換で末尾の改行が消える」バグなしに復元できること(開発中に一度この形で
  壊れた回帰)。
- 正常系: Assuan コマンド(`SETTITLE`/`SETPROMPT`/`GETPIN` 等)が送られ、
  `D` 応答が percent-decode されて 1 行返ること。
- キャンセル系: `D` 行のない `ERR` 応答で非ゼロ終了すること(fail closed)。
- ハング系: `SUDO_ASKPASS_TIMEOUT_SECONDS`(本番は `timeout` 引数、既定 90 秒)
  で確実にプロセスが刈られること。

加えて:

- `nix fmt` / `nix flake check`(personal-pop / company-pop-old / company-pop-new
  の 3 ホストとも evaluation + build 成功)。
- `shellcheck --severity=error` 通過。
- store に配備された `sudo-askpass` の pinentry 呼び出し先が、pinentry
  パッケージの store path(`services.gpg-agent.pinentry.program` 経由で解決)
  に正しく固定されていることを確認。

開発中に見つかった実装バグ(いずれも `--selftest` 導入時に発見・修正済み):
`ask()` 内の `[[ ... ]] && printf ...` 形式のガードが `set -e` 下では条件が
偽のときブロック全体を即終了させ、以降の Assuan コマンドが一切送られない
不具合、および `response="$(...)"` の非ゼロ終了(timeout 由来の 124 等)が
`set -e` で早期リターンし、意図した「fail closed で exit 1」ではなく
timeout 自身の exit code が漏れる不具合。

**実機での最終確認(端末に tty がある状態で `sudo -K && sudo true` を叩き、
GUI プロンプトが出て成功すること/キャンセルで即座に失敗すること)は、利用者
自身が自分の端末から行うこと** — 上記の理由により agent からは行わない。
