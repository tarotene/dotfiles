# ADR-0022 — esa MCP トークンをホストローカル GPG ファイル + 専用 launcher で供給する

- Status: Accepted
- Date: 2026-09-19
- Issue: No-Issue(grill-me セッション中に発見・裁定)
- Amends: ADR-0010 の「消費者不在と判明したチャネルは復活させない」判断の
  初適用例(ADR-0010 本文は immutable のまま、関係だけをここに記録する)。
  `~/.claude.json` への登録は `home/modules/claude-mcp-servers.nix`
  (`dotfiles.claude.mcpServers` の「口」)を消費する最初の実例でもある。

## Context

esa.io MCP サーバは `~/.claude.json` に既にグローバル登録されていたが、
そのディレクトリ内で direnv を通したセッション以外では常に Unauthorized
だった。実際の供給元は別の private リポジトリ(SOPS 暗号化トークン 1 個・
direnv ローダー・プロジェクトスコープの `.mcp.json` だけを持つ「シークレット
1 個のための器」)で、`env.ESA_ACCESS_TOKEN = "${ESA_ACCESS_TOKEN}"` という
記法が使われていた。

**訂正(初稿の誤り)**: 初稿では「`${VAR}` 記法は Claude Code の MCP 起動
プロセスでは展開されない」と断定していたが、これは誤り。Claude Code 公式
ドキュメント(<https://code.claude.com/docs/en/mcp>、2026-09-19 取得)は
`~/.claude.json` / `.mcp.json` いずれの `env` でも、セッション起動時の
シェル/プロセス環境変数で `${VAR}` を展開すると明記している。実際の原因は
単純で、`ESA_ACCESS_TOKEN` という環境変数自体が、供給元だったそのディレクトリ
の direnv セッションの外では一度も export されていなかっただけだった。

この訂正は解決策の骨格を変えない: `ESA_ACCESS_TOKEN` をどのセッションでも
展開可能にするには、ログインシェル起動時に毎回この変数を復号・export する
必要がある。これは ADR-0010 が明示的に退役させたパターン(シェル起動時の
GPG PIN プロンプト・全プロセスへの秘密展開)そのものであり、採用しない。

sops を利用するローカルプロジェクトはその private リポジトリが唯一で、
`home/modules/packages.nix` が sops パッケージを残していた理由の実体もこれ
だった。ADR-0010 は Consequences に「新しいシークレットが必要になったら、
供給チャネルはそのとき都度選び直す(自動ロードを安易に復活させない)」と
残しており、本 ADR はその最初の適用例になる。

private リポジトリの実名・team 名・トークン値は本 ADR にも他のリポジトリ内
ドキュメントにも書かない(public repo。この既存 private リポジトリ自体は
本 ADR の Decision 5 に従い archive する)。

**時系列の注記**: 本 ADR の作業ブランチを切った後、`home/modules/
claude-mcp-servers.nix`(`~/.claude.json` の `.mcpServers` を宣言的に足す
汎用の「口」、`dotfiles.claude.mcpServers` extensible option)が別ブランチで
先に merge された。この機構は本 ADR が最初に書かれた時点では存在せず、後から
riベース時に発見して採用した — Decision 3 はこの機構の消費で置き換えている。

## Decision

1. **トークンはホストローカル**に置く: `~/.config/esa/token.gpg`
   (`gpg --encrypt --recipient <personal identity の master fingerprint>`、
   退役する旧 private リポジトリの `.sops.yaml` の recipient 規約を踏襲)。
   git 管理外。
2. **esa 専用の launcher**(`scripts/esa-mcp-launcher` →
   `~/.local/libexec/esa-mcp-launcher`)が MCP サーバ起動時にトークンファイルを
   復号し、`ESA_ACCESS_TOKEN` として `exec npx -y @esaio/esa-mcp-server` する。
   `${VAR}` 展開は使わない(Context の訂正どおり展開自体は機能するが、その
   ためには変数をどこかで export し続ける必要があり、それが ADR-0010 の
   退役パターンに戻ってしまうため)。汎用 secret ラッパーにはしない — env
   トークン依存の MCP サーバは esa だけで、抽象化する 2 件目の消費者がいない。
3. **`~/.claude.json` の `mcpServers.esa`** は `home/modules/
   claude-mcp-servers.nix` が公開する `dotfiles.claude.mcpServers` に
   populate するだけでよい。冪等 jq merge の仕組み自体はそちらが一元的に
   持つため、`home/modules/esa.nix` は launcher の配備と値の populate だけを
   持つ(独自の activation スクリプトは書かない)。
4. **esa.nix は personal identity 層にのみ import する**(common.nix ではない)。
   トークンの gpg 宛先が personal の master fingerprint である以上、GnuPG は
   company ホストの YubiKey では復号できない([E] サブ鍵解決、ADR-0003
   Amendment)。common.nix に置くと company ホストに「毎セッション必ず起動
   失敗する MCP 登録」だけが残る。
5. **旧 private リポジトリは archive し、ローカル checkout は削除**する。
   移行時に esa 側でトークンを再発行し旧トークンを失効する(archive 履歴に
   残るトークンをすべて無効化するため)。
6. **`home/modules/packages.nix` の `sops` パッケージを削除**する
   (ADR-0010 と同じ判断基準: 消費者不在の経路は残さない。唯一の消費者
   だったそのリポジトリの archive でこの条件が満たされる)。
   > **[本 ADR の Amendment(#451)により撤回]** 「唯一の消費者」という前提が
   > 誤りで、リポジトリ外に現役の SOPS 消費者があった。`sops` は
   > `home/modules/packages.nix` に戻した。この決定が書かれた時点の判断として残す。

## Alternatives considered

- **`${ESA_ACCESS_TOKEN}` 展開 + ログインシェル起動時に復号・export**:
  Context の訂正どおり `${VAR}` 展開自体は機能するが、変数をどのセッション
  でも見えるようにするには shell 起動のたびに GPG 復号が要る。ADR-0010 が
  明示的に退役させた形(シェル起動時の GPG PIN プロンプト・全プロセスへの
  秘密展開)そのものであり、棄却。
- **SOPS ランタイムローダーの復活**: 同じく ADR-0010 が明示的に退役させた
  機構の再導入になる。棄却。
- **旧 private リポジトリの `.mcp.json` + direnv を継続利用**: そのディレクトリ
  内のセッションでしか esa MCP が動かない現状そのものであり、「全セッションで
  動くようにする」という到達目標と矛盾する。棄却。
- **平文トークンファイル**: at-rest 保護が無い。ADR-0010 が FALCON_CID を
  SOPS ではなく対話プロンプトに倒した理由(そもそも自動ロードしない)とは
  逆に、ここは自動ロードが要件なので at-rest 暗号化を外す理由がない。棄却。
- **汎用 secret ラッパー**: 現時点で env トークンを要求する MCP サーバは
  esa 1 件のみ。抽象化する消費者がいない YAGNI。棄却(将来 2 件目が現れたら
  再検討)。
- **`home/modules/claude-mcp-servers.nix` を使わず独自の jq merge を書く**:
  本 ADR の初回実装はこれだった(当時この機構が存在しなかったため)。後から
  同機構が merge されたと分かった時点で、重複する activation スクリプトを
  2 本 `~/.claude.json` に書き込ませる状態を避けるため、独自実装を捨てて
  共通の「口」へ乗り換えた。
- **esa 公式のリモート MCP サーバー(`https://mcp.esa.io/`、OAuth 2.1、β
  公開)への切り替え**: レビュー中に指摘され一次情報で調査した(取得日
  2026-09-19)。esa 公式ドキュメント(<https://docs.esa.io/posts/582>)は
  「Claude Desktop から連携することで、Claude Code で利用可能です。
  Claude Code から連携は今後対応予定です」と明記しており、このリポジトリの
  実際の利用クライアント(Claude Code CLI)からの直接接続は esa 自身が
  未対応と述べている。リモート MCP サーバー自体も
  「本機能は現在ベータ(β)版です。仕様は予告なく変更される場合があります」
  (<https://docs.esa.io/posts/584>)と明記されたベータ機能。加えて Claude
  Code の OAuth トークン保管は、このリポジトリの主要ホスト(Linux)で
  ADR-0003 の YubiKey ルート secrets モデルと同水準の保護を持つか
  ドキュメントから確認できなかった(<https://code.claude.com/docs/en/mcp>
  は「client secret はキーチェーン(macOS)または credentials file に保管」
  としか書いておらず、Linux での暗号化・ハードウェア連動の有無が不明)。
  以上 3 点(vendor 未対応・ベータ依存・保護水準不明)により今は見送り、
  棄却。再検討条件は tarotene/dotfiles#247 に追跡する。

## Consequences

- esa MCP は YubiKey 挿入が前提になる。カード不在時は launcher が明確な
  診断(トークンファイル欠如/復号失敗いずれの原因かを区別)を出して起動
  失敗する。
  > **[ADR-0003 Amendment 4(#252)により部分的に superseded]** [E] サブ鍵が
  > on-disk per-machine 化されたことで、日常運用でのカード挿入は不要になった
  > (カード上の元 [E] は disaster-recovery 用途として残置)。この行が書かれた
  > 時点の設計上の前提だった事実として残す。
- ログイン後、esa MCP を最初に起動したときにカード PIN の pinentry が
  1 回出る(以後は gpg-agent がキャッシュ)。sign-prewarm(on-disk [S] 鍵の
  パスフレーズを前倒しする別機構)はこの PIN を温めないが、発火点が
  セッション開始 = 画面を見ている瞬間である点は同じ設計思想を共有する。
  > **[ADR-0003 Amendment 4(#252)により superseded]** sign-prewarm は [E] も
  > 対象に拡張された(独立したキャッシュエントリのため個別に温める)。「カード
  > PIN」は「on-disk [E] のパスフレーズ」に置き換わった。
- company ホストには esa MCP を配らない(company-pop-old / company-pop-new
  は esa.nix を import しない)。
- `sops` パッケージの削除により、他にリポジトリ外の SOPS 消費者が現れた
  場合は `home/modules/packages.nix` へ 1 行戻すだけで復活できる。
- `docs/claude/claude-mcp-servers.md` の esa 例示(populate 前の想定コード)は
  本 ADR の実装が実例に置き換える。

## Verification

- `nix fmt` / `nix flake check`(全ホストの活性化パッケージが評価される —
  esa.nix は personal-pop / altair 経由でのみ評価に含まれる)。
- `shellcheck scripts/esa-mcp-launcher` がクリーン、
  `./scripts/esa-mcp-launcher --selftest` が gpg/npx をスタブして
  5 ケース(トークン欠如・復号失敗・空トークン・npx 不在・token handoff)を
  ネットワークなしで検査し OK になること。
- `hms .` を実際に適用し、`~/.claude.json` の `mcpServers.esa` が launcher
  パスへ書き換わること、トークン未配置状態で launcher が意図どおり
  診断つきで fail-closed することを実機確認済み。
- 実機での secrets 配置手順は `docs/setup.md` の「esa MCP token」節を参照。

## Amendment (2026-09-24 — Decision 6 の前提訂正: リポジトリ外に現役の SOPS 消費者がある, #451)

Decision 6 は「`sops` の消費者は archive した旧 private リポジトリ 1 件だけ」
という前提で `sops` パッケージを削除した。この前提は誤りだった。このリポジトリの
外に、現役の private な消費者がある。`.sops.yaml` の creation_rule 群・
スクリプト・skill から `sops` を直接呼んでおり、削除後はそのリポジトリで
`sops: command not found` になった。ADR-0034 に従い、その名前はここに書かない。

Consequences が明記していた復旧経路をそのまま踏み、`sops` を
`home/modules/packages.nix` に戻す。Decision 6 の他の部分(esa MCP の
トークン供給を SOPS から切り離したこと)は変えない。消費者を特定しないまま
パッケージを消す判断は、ADR-0010 の「消費者不在の経路は残さない」を適用する
前に、リポジトリ外の呼び出しを確かめる手段が無いと成り立たない。今回はその
確認が無かった。

### 執行点

- `home/modules/packages.nix` — `sops` を戻す(#451)
