# シェルスクリプトの後継技術調査 — investigation record

Status: **調査完了。結論は [ADR-0024](adr/0024-hook-cli-scripts-target-rust.md) に接地。**
本ファイルは腐る事実(バージョン・ベンチマーク数値・ツールの成熟度)の記録であり、
決定そのものは ADR 側にある(ADR-0008 の分離規約)。

Tracked in: No-Issue(ユーザー依頼の技術調査 — 対応する既存 Issue なし)。

## 動機・スコープ

このリポジトリは home-manager (nix) を決定論的な source of truth とする一方、
実際に動く道具は **87 本・約 17,900 行のシェルスクリプト**で、外部コマンド依存
(jq 541 回/37 ファイル、gh 214 回/19 ファイル、git 673 回/47 ファイル)が nix の
closure に固定されていない。加えて上位 4 本(pr-gate.sh 1634 行、
copilot-plan-review.sh 1588 行、github-audit 1511 行、gpg-subkey 1120 行)は
bash の表現力の限界に達しつつある。

調査対象スコープ(想定ワークロード)は Claude Code hooks + statusline
(18 本 / 8,918 行)と `~/.local/bin` デプロイの CLI 群(19 本 / 5,770 行)ほか
activation スクリプト・codex/copilot hooks を合わせた**約 40 本 / 15,000 行**。
escape-hatch(bootstrap.sh・install-packages 等)、他リポジトリへ配布する skills
の templates/scripts、git hooks、zsh モジュールは対象外(性質上シェルが適切、
または可搬性を壊すため)。

非機能制約(グリルセッションで確定):

1. nix flake でビルドでき、依存が closure に固定されること(must)
2. hook 起動が 50ms 未満であること(must、PreToolUse はツールコール毎に発火)
3. 単一言語に寄せること(should、ただし zsh モジュール・bootstrap.sh は対象外)
4. 既存の `--selftest`(表形式の deny/pass テスト)を通常のユニットテストへ
   移行できること(must)

## 候補と調査方法

深掘り 3 候補(ベースライン・Rust・Deno)は各々一次情報に当たった上で、代表的な
hook 1 本(`config/claude/hooks/git-stash-guard.sh`, 320 行 — stdin JSON・git
コマンド解析・deny 応答という hook の典型要素を全部含む)を実際に移植し、nix
ビルド・起動レイテンシ・closure サイズ・テスト移行を実測した。浅掘り 3 候補
(Go・Babashka・Nushell)は一次情報 1 点以上を引いた上で不採用/深掘り昇格の
判定のみ行った。

計測環境: `personal-pop` 相当のホスト、Determinate Nix 2.34.8 / nixpkgs
(2026-09-21 時点の channel)、rustc 1.94.0、Deno 2.6.9、hyperfine 1.20.0
(`nix shell nixpkgs#hyperfine` で一時導入)。

## 1. ベースライン: bash 続投 + `writeShellApplication`(+ resholve)

一次情報: Nixpkgs Reference Manual「Trivial build helpers」章
(<https://raw.githubusercontent.com/NixOS/nixpkgs/master/doc/build-helpers/trivial-build-helpers.chapter.md>、
NixOS/nixpkgs、取得日 2026-09-21)、実装ソース
(<https://raw.githubusercontent.com/NixOS/nixpkgs/master/pkgs/build-support/trivial-builders/default.nix>)。

- `runtimeInputs` は `lib.makeBinPath` で各パッケージの `bin/` を連結し、
  生成スクリプト冒頭の `export PATH=...` に埋め込む。`inheritPath = false`
  (デフォルト)ならホスト PATH を継承せず、宣言した closure だけが見える。
- build 時に shellcheck が既定で走り、`excludeShellChecks` / カスタム
  `checkPhase` で調整できる。
- **resholve**(<https://github.com/abathur/resholve>、abathur、取得日
  2026-09-21)は「シェルスクリプトの linker」を謳い、外部コマンド参照を静的解析
  して store パスに書き換える。`eval $variable` のような動的実行や二重引用符内の
  変数展開は原理的に解決不能(Known Gaps に明記)。nixpkgs には
  `resholve.mkDerivation`/`writeScript` があり、`dgoss` が実採用例
  (<https://github.com/NixOS/nixpkgs/pull/85827>)。writeShellApplication
  (書く側の軽量検証)と resholve(既存の複雑なスクリプトの後付け固定)は
  排他ではなく粒度が違う。
- テスト移行先候補: **bats-core**(nixpkgs に `pkgs.bats` として収録済み、TAP
  出力、<https://bats-core.readthedocs.io/en/stable/>、取得日 2026-09-21)が
  ShellSpec より nix との摩擦が小さい。

### 実測: PoC ビルドで実際に踏んだ摩擦

`git-stash-guard.sh` をそのまま `writeShellApplication` に載せたところ、
**既定の shellcheck が info レベルの SC2016(197 行目、シングルクォート内で
`$(` を展開しようとしていると誤検知)でビルドを失敗させた**。
`excludeShellChecks = [ "SC2016" ]` を明示しない限りビルドが通らない。
「既存の 87 本をそのまま writeShellApplication に載せる」という続投案自体、
ゼロコストではない(個々のスクリプトの shellcheck 適合を都度確認・調整する
作業が要る)ことが実測で確認できた。

## 2. Rust

一次情報: Nixpkgs Reference Manual Rust 節
(<https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/rust.section.md>)、
crane(<https://github.com/ipetkov/crane>、<https://crane.dev/API.html>、
ipetkov、取得日 2026-09-21)、The Cargo Book(cargo targets / workspaces、
取得日 2026-09-21)、serde.rs・docs.rs/serde_json(取得日 2026-09-21)、The Rust
Programming Language ch11(取得日 2026-09-21)。

- `buildRustPackage`(依存とアプリを 1 derivation にまとめる)と `crane`
  (`buildDepsOnly` で依存だけ先にビルドしキャッシュを複数バイナリ間で共有)の
  2 経路がある。20〜40 個の小 bin を 1 workspace にまとめる今回の想定構成では、
  依存(serde 等)が全 bin 共通になるはずなので、crane の依存キャッシュ共有が
  再ビルド時間で有利と考えられる(定量差は一次情報になく、上記は設計上の推論)。
- 1 workspace 内の複数 `[[bin]]`、または workspace メンバー分割のどちらでも
  多数の小プログラムを表現できる(Cargo 公式)。
- stdin JSON → stdout JSON は `serde_json::from_str`/`from_reader` +
  `#[derive(Deserialize)]` が定番。
- `cargo test` の `#[test]` 関数は `--selftest` の表形式ケースとほぼ 1:1 で
  移植できる(実際に PoC で確認、後述)。
- 起動レイテンシの一次情報は見つからず(サードパーティ実測
  <https://github.com/ngs/cli-lang-bench> はあるが hook ワークロードと異なる
  負荷)、**実測が必要**と調査時点で明記していた。

## 3. Deno + TypeScript

一次情報: Deno 公式ドキュメント(<https://docs.deno.com/>、Deno Land Inc.、
取得日 2026-09-21)。

- shebang 運用(`#!/usr/bin/env -S deno run ...`)は公式サポート。`-S` は GNU
  coreutils の拡張で POSIX 外だが、Pop!_OS(apt ベース)では問題にならない。
- `deno compile` で単一実行可能バイナリを作れる。cross-compile 可、
  `--engine quickjs` で軽量化可能(V8 の代替、ただし JIT なし)。
- **nix 統合: 確立された手法が無い。** `pkgs.deno` は nixpkgs 本流にあるが
  (<https://github.com/NixOS/nixpkgs/blob/master/pkgs/by-name/de/deno/package.nix>)、
  外部 import を closure にロックするコミュニティツール **deno2nix**
  (<https://github.com/SnO2WMaN/deno2nix>)は **2024-06-07 にアーカイブ済み**
  (非メンテナンス)。前身の `esselius/nix-deno`・`brecert/nix-deno` も個人
  プロジェクトの域を出ない。制約 1(must: closure 固定)を満たす成熟した経路が
  無いという、この調査で最も重い negative finding。
- stdin/stdout の JSON 専用ヘルパーは公式に無く、`Deno.stdin.readable` を
  自前でバッファリングする必要がある(PoC で実装、後述)。
- テスト: `deno test` は組み込みで追加設定不要。

### 訂正: `Deno.test.each()` は実在しない

机上調査の段階では「`Deno.test.each()` で表形式テストを 1:1 移植できる」と
報告されていたが、**Deno 2.6.9 の実 API に `Deno.test.each` は存在しない**
(PoC 実装時に `TS2339: Property 'each' does not exist on type 'DenoTest'` の
型検査エラーで発覚)。標準の `Deno.test` + `for` ループで代替した(後述コード)。
一次情報だけでなく実装で裏取りしないと机上調査の誤りに気づけない、という
本調査自体の教訓でもある。

## 4. 浅掘り: Go / Babashka / Nushell

- **Go**: `buildGoModule`(nixpkgs 公式、`vendorHash` で依存固定)は closure
  固定が容易、`encoding/json` の書き味も良好。ただしこのリポジトリ・
  エコシステムに Go の既存資産が無く、Rust/Deno 案に対する差別化が薄い。
  出典: <https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/go.section.md>
  (取得日 2026-09-21)。**深掘り候補だが優先度低、不採用。**
- **Babashka**: 公式サイトが謳う起動 0.026 秒という数値は魅力的で、
  `pkgs.babashka` も nixpkgs にある(PR #241119)。ただし Clojure(Lisp 系)の
  習熟が前提で、このリポジトリの他資産と地続きでない。
  出典: <https://babashka.org/>(取得日 2026-09-21)。**言語習熟コストで不採用。**
- **Nushell**: 公式ドキュメントの "Nu as a Shell" が hooks 等の機能について
  「対話的体験の拡張のために実装されており、スクリプト実行時には存在しない」
  と明記しており、設計の重心が対話シェルにあり単発起動のプログラム用途とは
  ミスマッチ。出典: <https://www.nushell.sh/book/nu_as_a_shell.html>
  (取得日 2026-09-21)。**設計目的の不一致で不採用。**

## 5. 学術文献: シェルスクリプトの構造的欠陥

- **Dong, Y., Li, Z., Tian, Y., Sun, C., Godfrey, M. W., Nagappan, M.
  (2022)『Bash in the Wild: Language Usage, Code Smells, and Bugs』**
  ACM Trans. Softw. Eng. Methodol., DOI: 10.1145/3517193(取得日
  2026-09-21)。GitHub 上の 100 万件超の bash スクリプトを ShellCheck で
  静的解析した大規模実証研究。code smell が皆無なのは一般スクリプトの約 20%、
  人気スクリプトでも約 50% に留まる。**「エラー傾向はスクリプトのサイズと
  中程度の正の相関を持つ」**——本調査の「大規模化で構造的に劣化する」という
  主張に対する直接的な実証的裏付け。
- 隣接領域として Tamanna et al.(2025)『Your Build Scripts Stink』
  (ASE 2025, arXiv:2506.17948)がビルドスクリプトの smell を同様に実証。
- ShellCheck 作者 Vidar Holen 自身の記述(<https://www.vidarholen.net/contents/blog/?p=859>、
  取得日 2026-09-21)は、2012 年に Bash FAQ の頻出反復アンチパターンを自動
  検出する IRC bot として始まった、という開発動機の一次証言(査読無し)。
- **先行例なし**: 「シェルという言語設計自体がなぜ大規模化に構造的に不向き
  なのか」を主題とした理論的・設計論的な論文は、試した検索範囲(Google 経由
  の WebSearch、ACM/arXiv/ResearchGate のヒットのみ確認、ACM Digital
  Library・IEEE Xplore 本体・Google Scholar 被引用リストは未確認)では
  見つからなかった。見つかったのは実証的な相関に留まる。

## PoC 実測結果

### 6.1 出力の等価性

代表入力(`{"tool_name":"Bash","tool_input":{"command":"git stash pop"}}`)に
対し、bash 版・Rust 版・Deno 版の deny 判定と理由文字列は **完全一致**
(整形の違いのみ)。selftest/テストケース(deny 11 + compound-deny 4 + pass 9 +
pass-unrelated 4 + 複数行系 6 = 計 34 パターン、bash 版の全ケース)を Rust
(9 `#[test]` 関数、テーブル駆動)と Deno(37 テストケース、`for` ループで
テーブル展開)の双方に移植し、**全ケース pass** を確認した。

### 6.2 起動レイテンシ(hyperfine 1.20.0, warmup 5, min-runs 50, 同一ホスト)

| 系列 | mean | p50 | p95 | max |
|---|---|---|---|---|
| bash(直接実行) | 13.0ms | 12.8ms | 15.2ms | 16.4ms |
| bash(`writeShellApplication`, runtimeInputs=[jq, git]) | 13.7ms | 13.6ms | 15.1ms | 18.0ms |
| **Rust**(`buildRustPackage`, release) | **1.2ms** | **1.1ms** | **1.7ms** | 5.3ms |
| Deno(`deno compile`) | 23.4ms | 22.8ms | 29.6ms | 31.8ms |
| Deno(`deno run`, インタプリタ) | 30.5ms | 29.6ms | 38.2ms | 40.0ms |

**制約 2(50ms 未満)は全系列で満たす**が、余裕度は大きく異なる: Rust は
50ms 予算の 1/30 以下、Deno は compile 版で p95 が予算の約 6 割、interp 版は
約 8 割まで迫る。bash 系列(直接・writeShellApplication とも 13〜14ms)は
Rust より遅いが Deno よりは速い——bash 起動そのものは軽量で、この hook の
遅さの主因は「jq を都度 fork する」設計にはなく(この hook は jq を呼んで
いない)、bash インタプリタ起動自体のコスト。

### 6.3 closure / バイナリサイズ

| 系列 | サイズ | 備考 |
|---|---|---|
| bash(`writeShellApplication`, runtimeInputs=[jq, git]) | 404.8MB(`nix path-info -S`) | git の推移的依存(perl, openssl 等)が支配的 |
| Rust(`buildRustPackage`) | 50.4MB(`nix path-info -S`)、バイナリ本体 2.2MB | glibc 等が支配的 |
| Deno(`deno compile`、単体バイナリ) | 89MB | denort(V8 込み)を丸ごと埋め込む。nix 未経由(比較用の素の deno CLI) |
| 参考: `pkgs.deno`(nixpkgs 本流のランタイム単体) | 202MB(`nix path-info -S`) | 個々のスクリプトの依存ロックは含まない(deno2nix はアーカイブ済みで使えない) |

**重要な留保**: 上記は `nix path-info -S` が返す**フル closure**であり、
「この hook を追加することで新たに増える限界コスト」ではない。`git`
自体は `home/modules/git.nix` の `programs.git.enable = true` により、
どの hook 実装を選んでも home-manager が既に nix store に常駐させている
(ADR-0002 以前からの既存事実)。したがって bash 続投案の 404.8MB は
「新規追加コスト」ではなく大部分が**既に支払い済みの共有 closure**。
同様に、glibc は bash/coreutils 経由で既に常駐しており、Rust バイナリの
50.4MB も大部分は新規コストではない。**真の限界コストで比較すべき**なのは:

- bash 続投: jq(既に `home/modules/packages.nix` 経由で常駐の可能性が高い、
  本調査では未確認)以外はほぼ限界コストゼロ。
- Rust: crate 依存(serde/serde_json/regex 等、今回の PoC で数十 MB 未満)が
  新規の限界コスト。
- Deno: **`deno compile` で 1 hook ごとに単体バイナリ化する設計だと、
  denort(V8 込み)がバイナリごとに毎回埋め込まれ、40 本に適用すると
  真の限界コストが 40 × 89MB 相当まで膨らみうる**(共有されない)。
  `pkgs.deno` ランタイムを共有し `deno run` で都度実行する設計なら
  ランタイム自体は 202MB を 1 回払うだけで済むが、それでも「各スクリプトの
  import 依存を closure にロックする」問題(deno2nix アーカイブ済み)は
  未解決のまま残る。

### 6.4 ビルド時間

- Rust: `nix-build`(cargo build + cargo test 込み、クリーンに近い状態から)
  57.3 秒。
- bash(`writeShellApplication`): `nix-build` 2.2 秒(jq/git は既に
  nixpkgs キャッシュ済みのため実質ラップのみ)。
- Deno: `deno compile` 2.9 秒(初回のみ denort 約 90MB のダウンロードが
  別途発生、以降はキャッシュ)。nix 経由のビルドは未実施(deno2nix
  アーカイブのため確立手法が無く、比較対象外)。

## PoC ソースコード(再現用)

以下はすべて scratchpad(`/tmp/.../scratchpad/poc/`)で作成・実行したもので、
このリポジトリのソースツリーには追加していない(D4: 調査記録に埋め込む方針)。

### git-stash-guard (Rust) — `Cargo.toml`

```toml
[package]
name = "git-stash-guard"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "git-stash-guard"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
regex = "1"

[profile.release]
lto = true
strip = true
```

### git-stash-guard (Rust) — `src/main.rs`

```rust
// git-stash-guard (Rust PoC) — bash 版 config/claude/hooks/git-stash-guard.sh の
// decide/decide_single ロジックの移植。
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use std::io::{self, Read};
use std::sync::OnceLock;

#[derive(Deserialize)]
struct ToolCall {
    tool_name: Option<String>,
    tool_input: Option<ToolInput>,
}

#[derive(Deserialize)]
struct ToolInput {
    command: Option<String>,
}

fn has_flag(long: &str, short: &str, args: &[&str]) -> bool {
    args.iter()
        .any(|a| *a == long || a.starts_with(&format!("{long}=")) || *a == short)
}

fn has_sha(args: &[&str]) -> bool {
    static SHA_RE: OnceLock<Regex> = OnceLock::new();
    let re = SHA_RE.get_or_init(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());
    args.iter().any(|a| re.is_match(a))
}

/// 単一の呼び出し1つを判定する。deny なら理由文を返す。
fn decide_single(seg: &str) -> Option<String> {
    let tok: Vec<&str> = seg.split_whitespace().collect();
    if tok.is_empty() || tok[0] != "git" {
        return None;
    }

    let mut idx = 1;
    if tok.len() > 1 && tok[1] == "-C" {
        idx = 3;
    }
    if idx >= tok.len() || tok[idx] != "stash" {
        return None;
    }

    let args = &tok[idx + 1..];
    let sub = args.first().copied().unwrap_or("push"); // 裸の `git stash` は push 相当
    // bash の `${args[@]:1}` は空配列に対しても空配列を返す(境界エラーにならない)。
    // Rust の `args[1..]` は args が空だと panic するため、安全側の skip(1) を使う。
    let rest_of: Vec<&str> = args.iter().skip(1).copied().collect();

    match sub {
        "list" | "show" => None, // 参照のみ、通す
        "push" => {
            let rest = rest_of.as_slice();
            if has_flag("--include-untracked", "-u", rest) && has_flag("--message", "-m", rest) {
                None
            } else {
                Some(
                    "git stash push は -u と -m <tag> を両方付けてください(deny)。\
この worktree の退避は git shelve \"<メモ>\" で積めます(推奨)。手動なら -u と -m <tag> を両方付けてください。"
                        .to_string(),
                )
            }
        }
        "apply" => {
            let rest = rest_of.as_slice();
            if has_sha(rest) {
                None
            } else {
                Some(
                    "git stash apply は SHA を明示してください(deny)。\
この worktree の退避を戻すなら git unshelve を使ってください。手動なら git stash list --format=\"%H %gs\" で確認してから git stash apply <SHA> としてください。"
                        .to_string(),
                )
            }
        }
        "drop" => {
            let rest = rest_of.as_slice();
            if has_sha(rest) {
                None
            } else {
                Some(
                    "git stash drop は SHA を明示してください(deny)。\
この worktree の退避を消すだけなら git unshelve が apply と drop をまとめて安全に行います。"
                        .to_string(),
                )
            }
        }
        "pop" => Some(
            "git stash pop は他の worktree の WIP を巻き込みます(deny)。\
この worktree の退避は git unshelve で戻せます。"
                .to_string(),
        ),
        "clear" => Some(
            "git stash clear は他の worktree の WIP を含めて全消去します(deny)。\
個別に戻すなら git unshelve、内容を確認したいだけなら git stash list / show を使ってください。"
                .to_string(),
        ),
        _ => Some(format!("未知の git stash 呼び出しです(deny): {seg}")),
    }
}

fn compound_stash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(^|[^[:alnum:]_])git[[:space:]]+(-C[[:space:]]+[^[:space:]]+[[:space:]]+)?stash([^[:alnum:]_]|$)").unwrap()
    })
}

/// Bash ツールのコマンド文字列全体を判定する。deny なら理由文を返す。
fn decide(cmd: &str) -> Option<String> {
    static WORD_STASH_RE: OnceLock<Regex> = OnceLock::new();
    let word_re = WORD_STASH_RE.get_or_init(|| Regex::new(r"(^|[^[:alnum:]_])stash([^[:alnum:]_]|$)").unwrap());
    if !word_re.is_match(cmd) {
        return None;
    }

    let normalized = cmd
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace(';', "\n")
        .replace('|', "\n");

    for seg in normalized.lines() {
        if seg.is_empty() {
            continue;
        }

        if seg.contains("$(") || seg.contains('`') || seg.contains('>') || seg.contains('<') {
            if compound_stash_re().is_match(seg) {
                return Some(format!("複合コマンドの中に git stash が含まれています(deny): {seg}"));
            }
            continue;
        }

        if let Some(reason) = decide_single(seg) {
            return Some(reason);
        }
    }

    None
}

fn main() {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return;
    }

    let Ok(call) = serde_json::from_str::<ToolCall>(&input) else {
        return;
    };
    if call.tool_name.as_deref() != Some("Bash") {
        return;
    }
    let Some(cmd) = call.tool_input.and_then(|t| t.command) else {
        return;
    };

    if let Some(reason) = decide(&cmd) {
        let out = json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }
        });
        println!("{out}");
    }
}

// #[cfg(test)] mod tests { ... } は 9 関数・34 ケースを bash 版 selftest から
// 1:1 移植し、全て pass した(本文参照)。全文は git 履歴のこの調査時点の
// scratchpad には残っていないため省略し、対応関係のみここに記録する:
// expect_deny(11 パターン) / expect_pass(9 パターン) / 複合コマンド系
// (deny 4 + pass 6 + 単語境界回帰 3 + リダイレクト系 2)を、bash の
// expect_deny/expect_pass ヘルパーと同型の assert 関数でそのまま踏襲した。
```

### git-stash-guard (Deno) — `main.ts`(抜粋、コア判定ロジック)

```typescript
#!/usr/bin/env -S deno run --allow-read=/dev/stdin
interface ToolCall {
  tool_name?: string;
  tool_input?: { command?: string };
}

function hasFlag(long: string, short: string, args: string[]): boolean {
  return args.some((a) => a === long || a.startsWith(`${long}=`) || a === short);
}
function hasSha(args: string[]): boolean {
  return args.some((a) => /^[0-9a-f]{40}$/.test(a));
}

function decideSingle(seg: string): string | undefined {
  const tok = seg.split(/\s+/).filter((s) => s.length > 0);
  if (tok.length === 0 || tok[0] !== "git") return undefined;
  let idx = 1;
  if (tok.length > 1 && tok[1] === "-C") idx = 3;
  if (idx >= tok.length || tok[idx] !== "stash") return undefined;

  const args = tok.slice(idx + 1);
  const sub = args[0] ?? "push";
  const rest = args.slice(1);

  switch (sub) {
    case "list":
    case "show":
      return undefined;
    case "push":
      if (hasFlag("--include-untracked", "-u", rest) && hasFlag("--message", "-m", rest)) {
        return undefined;
      }
      return "git stash push は -u と -m <tag> を両方付けてください(deny)。...";
    case "apply":
      if (hasSha(rest)) return undefined;
      return "git stash apply は SHA を明示してください(deny)。...";
    case "drop":
      if (hasSha(rest)) return undefined;
      return "git stash drop は SHA を明示してください(deny)。...";
    case "pop":
      return "git stash pop は他の worktree の WIP を巻き込みます(deny)。...";
    case "clear":
      return "git stash clear は他の worktree の WIP を含めて全消去します(deny)。...";
    default:
      return `未知の git stash 呼び出しです(deny): ${seg}`;
  }
}

const COMPOUND_STASH_RE =
  /(^|[^A-Za-z0-9_])git[ \t]+(-C[ \t]+[^ \t]+[ \t]+)?stash([^A-Za-z0-9_]|$)/;
const WORD_STASH_RE = /(^|[^A-Za-z0-9_])stash([^A-Za-z0-9_]|$)/;

export function decide(cmd: string): string | undefined {
  if (!WORD_STASH_RE.test(cmd)) return undefined;
  const normalized = cmd.replaceAll("&&", "\n").replaceAll("||", "\n")
    .replaceAll(";", "\n").replaceAll("|", "\n");
  for (const seg of normalized.split("\n")) {
    if (seg.length === 0) continue;
    if (seg.includes("$(") || seg.includes("`") || seg.includes(">") || seg.includes("<")) {
      if (COMPOUND_STASH_RE.test(seg)) {
        return `複合コマンドの中に git stash が含まれています(deny): ${seg}`;
      }
      continue;
    }
    const reason = decideSingle(seg);
    if (reason !== undefined) return reason;
  }
  return undefined;
}

async function readStdinText(): Promise<string> {
  const chunks: Uint8Array[] = [];
  let total = 0;
  for await (const chunk of Deno.stdin.readable) {
    chunks.push(chunk);
    total += chunk.byteLength;
  }
  const merged = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder().decode(merged);
}

async function main() {
  let input: string;
  try {
    input = await readStdinText();
  } catch {
    return;
  }
  let call: ToolCall;
  try {
    call = JSON.parse(input);
  } catch {
    return;
  }
  if (call.tool_name !== "Bash") return;
  const cmd = call.tool_input?.command;
  if (!cmd) return;
  const reason = decide(cmd);
  if (reason !== undefined) {
    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: reason,
      },
    }));
  }
}
if (import.meta.main) await main();
```

`main_test.ts` は `Deno.test` + `for` ループで、bash selftest の 34 ケースを
37 個の独立テストケース(一部分割)に展開し、全 pass を確認した(§Deno.test.each
訂正を参照。実装は `denyTable`/`passTable` ヘルパー関数でループを共通化)。

### wsa-guard(baseline 比較用)— `default.nix`

```nix
{ pkgs ? import <nixpkgs> { } }:
pkgs.writeShellApplication {
  name = "git-stash-guard";
  runtimeInputs = [ pkgs.jq pkgs.git ];
  excludeShellChecks = [ "SC2016" ]; # 既存スクリプトの SC2016 info を実測で確認、後述
  text = builtins.readFile ./git-stash-guard-body.sh; # 既存 bash 本体をそのまま読む
}
```

### rust-guard の nix ビルド式(closure 計測用)

```nix
{ pkgs ? import <nixpkgs> { } }:
pkgs.rustPlatform.buildRustPackage {
  pname = "git-stash-guard";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
```

## 総合評価表

| 観点 | bash 続投 | Rust | Deno |
|---|---|---|---|
| (1) closure 固定 | ◯(runtimeInputs で固定、ただし個々の shellcheck 適合コストあり) | ◯(buildRustPackage/crane とも成熟) | ✗(deno2nix アーカイブ済み、確立手法なし) |
| (2) 起動 50ms 未満 | ◯(mean 13〜14ms) | ◎(mean 1.2ms、予算の 1/30 以下) | ◯(mean 23〜30ms、予算に対する余裕が最も小さい) |
| (3) 保守性・表現力 | △(型なし、jq 依存のまま) | ◯(型・エラー伝播が構造的) | ◯(型・標準ライブラリが充実) |
| (4) `--selftest`→テスト移行 | △〜◯(bats、実装コストあり) | ◎(cargo test に自然に 1:1) | ◎(deno test に 1:1、ただし机上調査の API 誤りに実装で気づいた) |
| 既存資産との整合 | ◯(変更ゼロ) | ◎(herdr・telepath・rust-repo-governance と地続き) | △(ADR-0002 の literal 思想とは親和的だが nix 面で新規開拓) |

## 参照

- [ADR-0024](adr/0024-hook-cli-scripts-target-rust.md) — 本調査から下した決定。
- グリルセッションの計画: `/home/tarotene/.claude/plans/nix-warm-frog.md`
  (ローカルパス、リポジトリ外)。
