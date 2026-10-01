# pkexec-guard — agent セッションから root 権限コマンドを polkit 経由で許す

`crates/pkexec-guard`(Rust、ADR-0024)の設計根拠。PR #326「sudo パスワードを
pinentry 経由の SUDO_ASKPASS で渡す」を再開する grill セッション(2026-09-28)
で安全性を裁定し直した結果の設計。#326 自体は不採用として close し、この PR
が代わりの設計を実装する。

## 動機

Claude Code(および Codex CLI / Copilot CLI)の Bash ツール実行には制御端末が
ない。`tty` は「not a tty」を返し、`/dev/tty` も開けない。そのため `sudo` を
含むコマンドは「a terminal is required to read the password」で即座に失敗し、
apt 層の適用(`scripts/install-packages.sh`)のような root 権限が要る単発の
確認・操作は、人間が端末から起動するしかなかった。

## 採らなかった設計: SUDO_ASKPASS

PR #326 は当初、`SUDO_ASKPASS` helper が pinentry に生の Assuan プロトコルで
`GETPIN` を発行し、得たパスワードを **stdout に平文で返す**方式だった。
`permissions.ask` によるコマンド単位の確認と pinentry への入力を「二段承認」
として安全性を論証していたが、再開にあたって裁定し直した結果、次の 3 点で
不採用とした。

1. **#348 → #421 が確定した事実と衝突する。** #348 は、`pinentry-gnome3` に
   生の Assuan `GETPIN` を直接送ると、人間の操作なしに実際のログイン
   パスワードが `PASSWORD_FROM_CACHE`(libsecret 経由の外部パスワード
   キャッシュ)として返ってくることを実機で 2 回再現した。#421 は
   `services.gpg-agent.noAllowExternalCache = true` でこの経路を
   `gpg-agent` 自身からは塞いだが、ADR-0003 Amendment 5 item 2 が明記する
   とおり、**同じ uid のプロセスが pinentry を直接叩く経路は塞げない**
   残余リスクとして受け入れた。SUDO_ASKPASS 方式は、まさにその経路を
   正規の手順として使ってしまう。
2. **helper の stdout は agent から読める。** agent(プロンプト
   インジェクションされた場合を含む)が `$SUDO_ASKPASS` を直接実行すると、
   「sudo」と題した本物と見分けのつかないダイアログが出る。人間がそこに
   入力した値は、そのまま tool 出力・トランスクリプト・API に流れる。
   呼び出し元プロセスを検査する対策も、`exec > >(tee …)` してから exec
   すれば stdout だけ横取りしてすり抜けられるため、検出止まりで表現不可能
   にはできない。
3. **auto mode では「二段承認」が一段に落ちる。** #348 の記録が示すとおり
   auto mode(分類器が permission ask を代行するモード)も使われており、
   その場合コマンドを目視する関所が失われ、残る人間の関所は pinentry
   ダイアログだけになる。pinentry のダイアログにはどのコマンドが root で
   実行されるかが一切表示されない。

代わりに polkit(`pkexec`)を使う——パスワードは root 側の
`polkit-agent-helper-1` が検証するだけで agent の出力には一切出ない。しかも
`pkexec` は既にどのホストにも入っており、**agent は今でも guard なしで任意の
`pkexec` を叩ける**。この PR の実体は、既にある経路を絞る guard である。

## 仕組み

PreToolUse(`Bash`、matcher のみで `if` は付けない — 後述)専用。

### 不変条件

1. **ダイアログに映った文字列 = 実際に root で動く argv。** pkexec(1)の
   ダイアログは `cmdline_short`(polkit-org/polkit `src/programs/pkexec.c`
   tag 124)を表示する。このロジックは argv を空白で連結し、80 文字を
   超えると先頭 38 文字 + ` ... ` + 末尾 37 文字に省略する
   (https://raw.githubusercontent.com/polkit-org/polkit/124/src/programs/pkexec.c、
   取得 2026-09-28)。省略が起きると、実際に実行される引数の一部がダイアログ
   に表示されなくなる。この guard は複合コマンド・展開・80 文字超をすべて
   deny する。
2. **パスワードは同じ uid のパイプを通らない。** 上記「採らなかった設計」の
   経路の再導入を防ぐため、`sudo -A`/`-S`/`--askpass`/`--stdin`/
   `SUDO_ASKPASS` の出現を全 Agent 共通で deny する(軸: 検出のみ — 主たる
   防御はこの仕組み自体を配備しないことで、これは再導入の安全網)。
3. **裸の `pkexec` は許さない。** `config/zsh/modules/10-path.zsh` は
   `~/.local/bin`・`~/bin` を PATH の先頭に置く。ユーザーが書き込めるこの
   場所に同名の偽 `pkexec` を置かれると、本物の polkit を経由せず
   pinentry 等を直接叩かれてしまい、不変条件 2 が崩れる。`pkexec` は常に
   絶対パス `/usr/bin/pkexec` として書かれていることを要求し、実行時には
   そのバイナリ自身が root 所有・setuid・group/other 書き込み不可である
   ことも確かめる。
4. **root で実行してよい相手は閉じた許可リストのみ。** 対象パスと、
   引数中の絶対パスの各構成要素すべてが、root 所有かつ group/other
   書き込み不可であることも確かめる(相対パスは deny)。
5. **Codex CLI / Copilot CLI からは pkexec を一切許さない。** 明示的な
   ask ルール(`permissions.ask` の `Bash(/usr/bin/pkexec *)`)が auto 系
   モードでも確認を強制することを Claude Code の公式ドキュメントで確認
   できているのは Claude のみ(
   https://code.claude.com/docs/en/auto-mode-config.md、取得
   2026-09-28: 「明示的な ask ルールは分類器より先に評価され、auto mode
   でも必ず確認を出す」)。二段承認が成立しない CLI では全 deny にする。

### 許可リスト

実装時点(2026-09-28、vega)でこのホストに実在を確認した絶対パス:

```
/usr/bin/apt-get
/usr/bin/apt
/usr/bin/dpkg
/usr/bin/systemctl
/usr/bin/journalctl
/usr/bin/udevadm
/usr/sbin/ufw
```

`tailscale` は当初の裁定(Q2)で候補だったが、実装時にこのホストへ
未インストールだった(`command -v tailscale` が失敗)ため見送った。
「実装時に実在パスを確認し、実在しないものは載せない」方針(AGENTS.md の
ADR-0034 節と同じ精神)による。導入されたホストで実パスを確認したうえで、
`crates/pkexec-guard/src/lib.rs` の `ALLOWED_TARGETS` に PR レビューを
経て追加する。

### 検出粒度: 部分一致ではなく語単位

実装中、この crate 自身を検証するコマンドで 2 つの誤検知を実機(`hms .` で
適用したセッション)で踏んだ——どちらも「コマンド文字列に `pkexec` という
**部分文字列**が含まれるか」を検出条件にしていたことが原因:

- `jq -r '.command | test("pkexec")' crates/pkexec-guard/src/lib.rs`
  のような、単純コマンド(複合演算子なし)の引数・パスに `pkexec` が
  部分一致するだけのコマンドまで deny してしまった。
- `cargo test -p pkexec-guard | tail -30` のような、この crate 自身の名前
  (`pkexec-guard`)を含むパイプ済みコマンドまで、fail closed(複合コマンド
  は解析不能として deny する設計)に巻き込まれた。

対応(`crates/pkexec-guard/src/lib.rs` の `mentions_pkexec`):

- **解析できたコマンド**(複合演算子なし)は、分割した語のいずれかが
  `pkexec` または `/usr/bin/pkexec` と**完全一致**するときだけ「pkexec の
  呼び出しに見える」と判定する。`test("pkexec")` のような引用符内の部分
  一致や、`crates/pkexec-guard/...` のようなパスの部分一致は対象にしない。
- **解析できなかったコマンド**(複合コマン・展開等)は、語の完全一致が
  取れないため、独立した語としての `pkexec` の出現(前後が英数字・`_`・
  `-` でない)まで許容範囲を広げる——`cd /tmp && pkexec ...` のような
  compound を引き続き fail closed にするための安全網(軸: 検出のみ)。
  `pkexec-guard` のようにハイフンで続く語は対象にしない。

### 複合コマンドの文単位判定(#548)

上記の対応後も、複合コマンドは「解析できなかったコマンド」の枠に丸ごと
入るため、`;`/`&&`/`|` 等で他の文と連結された途端、地の文で `pkexec` を
言及しているだけの引用符付き文字列(例: `bash script --check-dup
"pkexec-guard が pkexec という語を含む..."; echo done`)まで fail closed に
なっていた。回避策(この語を含む文字列は他の文と連結せず単独の Bash
呼び出しにする)をこの文書に明記するだけで済ませる案もあったが、
`hook_io::shell` の既存 quote-aware tokenizer を文分割まで拡張する対応の
方が、同じ誤検知クラスの再発(実装中に本 PR 自身の作業で 2 回踏んだ)を
構造的に防げるため、そちらを採った。

`hook_io::shell::split_segments` が `;`/`&&`/`||`/`|`/`|&`/`&`/改行(クォート
外でのみ区切りとして働く)で複合コマンドを文に分割し、各文を独立に
`split` と同じ規則で解析する。`mentions_pkexec` の判定は文ごとに次のいずれか:

- 解析できた文に `pkexec`/`/usr/bin/pkexec` と完全一致する語がある
  (**exact mention**) → 複合コマンドの中に実際の呼び出しに見える文が
  あるとして deny(不変条件1は変わらない——polkit のダイアログは pkexec に
  渡された argv しか表示せず、同じ Bash 呼び出し内の他の文は承認の目に
  入らず実行される)。
- 解析できない文(展開・リダイレクト等)が語境界一致で `pkexec` を含む
  (**fuzzy mention**) → リダイレクトで偽装された呼び出しを排除できないため
  引き続き deny(軸: 検出のみ、安全側を維持)。
- どの文にも exact/fuzzy mention が一切無い → 判定しない(pass)。上の
  引用符付き文字列の例はこれに当たる——引用符全体が1つの語として正しく
  解析でき、`pkexec` 単体とは完全一致しないため。

単一コマンド(区切りが無い)の判定は一切変えていない——`split_segments`
が1文しか返さない場合は旧来どおり `split` + `mentions_pkexec` の経路を
そのまま通る。

## `permissions.ask` の拡張

`settings-reconcile claude-permissions`(旧 `home/modules/claude.nix` の
`registerPermissions`)は、この PR まで `.permissions.allow` しか冪等更新して
いなかった。`retire_ask` / `ask`(旧 bash では `--retire-ask <r>… --ask <a>…`)の
2 欄を追加し、`.permissions.ask` も同じ retire/add パターンで
更新するようにした。`askRules = [ "Bash(/usr/bin/pkexec *)" ];` が唯一の
エントリ。

## 変更しないもの

- `scripts/install-packages.sh`(apt 層の一括適用): 人間が端末で実行する
  ことに変わりはない。個々の短い管理コマンド(`apt-get install <pkg>` 等)
  だけを agent の `pkexec` 経由に開放する。
- `/etc/sudoers` / `/etc/sudoers.d`
- darwin(altair): macOS の管理者権限確認ダイアログには実行コマンドが
  表示されないため、不変条件 1 を満たせない。この guard は Linux ホスト
  限定で、darwin では pkexec 自体が存在しない。

## 既知の限界

- **許可リスト内バイナリ自身のオプションによる踏み台**(GTFOBins 型)は
  対象外。例えば `dpkg` に任意の `.deb` を渡せば任意コードが root で
  実行されうる——この guard は「呼び出し形とパスの所有権」だけを検証し、
  渡す**中身**の安全性までは検証しない。最終防波堤は認証ダイアログでの
  人間の目視判断であり、機械的な検証だけでは閉じない。
- **同一 uid からの Secret Service 読み取り**(#348/ADR-0003 Amendment 5)
  は、この guard の対象外——sudo askpass 経路の再導入を防ぐだけで、
  pinentry を直接叩く他の経路そのものは(#421 の対処後も)残っている。
- **agent の Bash ツールは認証ダイアログを観測できない**(既知の限界では
  なく仕様上の事実)。次節「認証フローの実態」参照。

## 認証フローの実態(実機調査、2026-09-28)

実機検証中、`/usr/bin/pkexec /usr/bin/systemctl --version` を agent から
実行したところ、tool 出力に目視できる新規の認証ダイアログが一切現れずに
成功した(`pkcheck --action-id org.freedesktop.policykit.exec --process $$
--allow-user-interaction` も即座に rc=0)。`sudo -n true` は別途パスワード
を要求するため sudo 自体がパスワード無しというわけではなく、当初は
polkit 側の cache/grant(`auth_admin_keep` 相当や、このホスト固有の
polkit ルール)を疑ったが、`journalctl -b` を調べた結果、**キャッシュでは
なく毎回の正規認証だった**ことを確認した:

```
polkitd[1090]: Operator of unix-session:3 successfully authenticated as
unix-user:tarotene to gain ONE-SHOT authorization for action
org.freedesktop.policykit.exec for unix-process:... [...eval
'/usr/bin/pkexec /usr/bin/systemctl --version' ...]
```

- `ONE-SHOT` は polkit の用語で「キャッシュ・保持なし」を意味する
  (`auth_admin_keep` のような保持型とは異なる)。実際、2 回の独立した
  `pkexec` 呼び出しそれぞれについて、独立した `successfully authenticated`
  ログが出ていた——1 回目の認証を 2 回目が使い回した形跡はない。
- PAM 側(`/usr/lib/pam.d/polkit-1` → `@include common-auth` →
  `/etc/pam.d/common-auth` の `pam_unix.so nullok`)にも、パスワード無しで
  通す仕組み(生体認証モジュール等)は設定されていない。`nullok` は
  「アカウントのパスワードハッシュ自体が空なら許す」という意味で、この
  アカウントには該当しない。
- `49-ubuntu-admin.rules`/`50-default.rules`(`/usr/share/polkit-1/
  rules.d/`)の `polkit.addAdminRule` は「`unix-group:sudo` を admin
  identity として扱う」という定義であり、認証そのものを省略する設定では
  ない。

結論: **agent の Bash ツールには制御端末が無いため、polkit の認証
ダイアログ(COSMIC では `cosmic-osd`)を agent 自身は一切見ることも
操作することもできない**。しかし、その認証ダイアログは agent とは別の
経路(GUI セッション)で実際に人間に提示され、人間がそこへ入力して初めて
`pkexec` が成功している——journalctl のログはその入力が本物の PAM
認証(パスワード等)を経て成立したことを裏付けている。「新規ダイアログ
なしに成功した」ように見えたのは、agent 側にダイアログの可視性が無いこと
による見かけ上の現象であり、認証そのものがスキップされたわけではない。

この構造は、この guard が前提とする「認証ダイアログを唯一の人間の関所に
する」設計と矛盾しない——むしろ `ONE-SHOT`(非保持)であることは、
`sudo` の timestamp キャッシュより強い性質(毎回人間の入力を要求する)を
意味する。

## 先行例との差分

- **`rulesets-write-guard`**: 同じ「`hook_io::shell::split` で語分割し、
  未知の入力は判定しない/deny-only」の型を踏襲する。ただし pkexec-guard は
  「判定できない(複合コマン等)」を rulesets-write-guard と違い fail
  **closed** にする——rulesets-write-guard は gh の書き込みを見逃しても
  `apply-rulesets.sh` 側の検証が最終防波堤になるが、pkexec-guard には
  それに相当する後段の検証が無いため。
- **実装言語**: ADR-0024 の既定どおり Rust(bash 資産を source する必要が
  無いため)。
- **Codex/Copilot 展開**: `attribution-guard`/`pr-title-guard` と同じ
  「lost-update 対策で順序付けた 2 本の activation」の型を踏襲するが、
  bash の `source` に相当する仕組みが Rust に無いため、per-agent adapter
  ファイルではなく `--agent <name>` CLI フラグで出力形式を切り替える単一
  バイナリにした(`crates/pkexec-guard/src/main.rs`)。
