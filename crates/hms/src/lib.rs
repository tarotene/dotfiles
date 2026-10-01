//! hms — home-manager switch、正準の適用手順(docs/operations.md)。bash 版
//! `scripts/hms.sh` の移植(ADR-0024 / #389 Stage 4e、#414)。
//!
//! 使い方: `hms [flake-ref] [--public-only]`
//!
//! ```text
//! hms          pushed main(github:tarotene/dotfiles)を適用。private wrapper
//!              flake が登録されていればそちらを適用(ADR-0034)
//! hms .        現在の checkout/worktree を適用(push 前の検証)。wrapper 登録
//!              ホストでは wrapper 経由で `dotfiles` をこの checkout に
//!              override して適用する(ADR-0034 Amendment 2026-09-27)
//! hms <path>   任意のローカル checkout(`hms .` と同じ経路)
//! hms . --public-only
//!              wrapper ホストでも checkout 単体を適用する
//! ```
//!
//! シェル状態(cd・export)を呼び出し元に残す処理は無い(`~/.local/bin/hms` に
//! 配備された独立の実行ファイルで、zsh 関数ではない)ので、全体を Rust に移せる。
//! bash 版の `--selftest` は無い(純関数の単体テスト + スタブ PATH の統合テストに移した)。
//!
//! 1 コマンド = 適用 runbook 全体:
//!
//! 1. home-manager switch --flake <ref>#<host> -b backup
//! 2. systemctl --user daemon-reload
//! 3. 生成された fcitx5 autostart unit を restart — switch で ExecStart の store
//!    path は動くが、daemon-reload だけでは generated unit は再起動されず旧バイナリが
//!    動き続ける
//! 4. restart 後に unit が active で MainPID が生きていることを検証
//!
//! 既定 ref はリモート main(どのローカル checkout のブランチ・dirty 状態にも依存
//! しない)。worktree の適用は明示の `hms .` だけ。
//!
//! nix は github: 形式の flake ref の解決を tarball-ttl(既定 1h)キャッシュする。
//! マージ直後に `hms` が 1 時間前の main を黙って適用して "Done." を出す(#48)ので、
//! 非ローカル ref は switch 前に `--refresh` し、実際に適用する revision を表示して
//! 古い適用が痕跡なしで終わらないようにする。
//!
//! `hms .` / `hms <path>` が wrapper 登録ホストで PUBLIC checkout 単体を適用すると、
//! wrapper が足す private value module(bleep 自身の denylist config=orgs.txt/
//! repos.txt を含む、ADR-0034 Amendment 2026-09-24)を無警告で落とし、安全機構を
//! 切ってしまう。実際に起きた(2026-09-27、直前の `hms` が再生成した bleep config を
//! 手動 `hms .` が落とし、欠けた orgs.txt を見たエージェントが古い backup から手で
//! 復元した)ため、`local_apply_plan` が既定で wrapper 経由にする:
//!
//! ```text
//! home-manager switch --flake <private-hub-ref>#<host> \
//!   --override-input dotfiles path:<abs-path> --no-write-lock-file -b backup
//! ```
//!
//! `--public-only` で従来の checkout 単体適用に戻せる。
//!
//! wrapper の flake.lock が `dotfiles` を古い rev に pin している問題(#48 と同形)は
//! 1 層下にもある: #48 の refresh は wrapper 自身の ref しか再解決せず、lock 内の
//! `dotfiles` input は触らない。適用対象 flake の lock が `dotfiles` input を持つ
//! (wrapper、リモートでもローカルでも)なら、lock を信用せず pushed main の解決済み
//! revision で override する(ADR-0034 Amendment 2026-09-25)。
//!
//! 上の 2 経路はどちらも `--override-input` を渡す。nix 自身の `--help` どおりこれは
//! `--no-write-lock-file` を含意し、毎回 `warning: not writing modified lock file of
//! flake '<ref>':` と input の差分を出す — override が効いている痕跡で異常ではなく、
//! この警告だけを消す nix のフラグは無い。`lock_warning_expected` がこの場合を検出し、
//! nix の出力の前にその旨を言う。

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

pub const DEFAULT_REF: &str = "github:tarotene/dotfiles";
pub const FCITX5_UNIT: &str = "app-fcitx5@autostart.service";

// ---------------------------------------------------------------------------
// 純関数(I/O なし。bash 版 selftest の 16 ケースは tests モジュール)
// ---------------------------------------------------------------------------

/// `local_apply_plan` の結果。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ApplyPlan {
    /// wrapper 経由で `dotfiles` をローカルパスに override して適用する。
    Route,
    /// 与えられた ref をそのまま適用する(従来の挙動)。
    AsIs,
}

/// ローカルパスの適用を、そのパス単体ではなく登録済み wrapper flake 経由にするか。
///
/// - `wrapper_ref`: resolve_default_ref の結果
/// - `default_ref`: DEFAULT_REF("wrapper 未登録"の検出用)
/// - `ref_is_local`: 適用する ref がディスク上のパスか
/// - `ref_is_wrapper`: 適用する ref 自身が wrapper flake か(flake.lock が `dotfiles` input
///   を pin している=既存の wrapper-lock override 経路が扱う)
/// - `public_only`: `--public-only` が渡されたか
pub fn local_apply_plan(
    wrapper_ref: &str,
    default_ref: &str,
    ref_is_local: bool,
    ref_is_wrapper: bool,
    public_only: bool,
) -> ApplyPlan {
    if public_only {
        return ApplyPlan::AsIs;
    }
    if wrapper_ref == default_ref {
        return ApplyPlan::AsIs; // このホストに wrapper は登録されていない
    }
    if !ref_is_local {
        return ApplyPlan::AsIs; // リモート ref は既に wrapper 経由(か明示指定)
    }
    if ref_is_wrapper {
        return ApplyPlan::AsIs; // 明示的な wrapper checkout — 既存の lock-override 経路が扱う
    }
    ApplyPlan::Route
}

/// これから走らせる `home-manager switch` が `--override-input` を持ち、nix が
/// "not writing modified lock file" 警告を出すはずか。
pub fn lock_warning_expected<S: AsRef<str>>(extra_opts: &[S]) -> bool {
    extra_opts.iter().any(|a| a.as_ref() == "--override-input")
}

/// jq の `-r '… // empty'`: null / false / 欠如は空文字、文字列はそのまま、
/// それ以外は JSON 表記。
fn jq_r_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `nix flake metadata --json` の `.revision // empty`。
pub fn metadata_revision(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .map(|v| jq_r_or_empty(v.get("revision")))
        .unwrap_or_default()
}

/// `nix flake metadata --json` の文書から、ルート flake の `dotfiles` input が
/// 素の(follows でない)input — つまり適用対象が ADR-0034 の private wrapper flake —
/// のとき、その input が現在 pin されている locked rev を返す。`dotfiles` input が
/// 無い、または `follows` チェーン(ノード名の文字列でなく JSON 配列)のときは空文字:
/// どちらも wrapper 自身の直接 pin ではないので override する対象が無い。
pub fn wrapper_locked_dotfiles_rev(json: &str) -> String {
    let Ok(doc) = serde_json::from_str::<Value>(json) else {
        return String::new();
    };
    let nodes = &doc["locks"]["nodes"];
    match nodes["root"]["inputs"].get("dotfiles") {
        Some(Value::String(node)) => jq_r_or_empty(nodes[node.as_str()]["locked"].get("rev")),
        _ => String::new(),
    }
}

/// `dotfiles` input を wrapper 自身の lock ではなく pushed main の解決済み revision
/// に pin する `home-manager switch` 引数。revision が空なら何も返さない
/// (呼び出し側は revision を解決できたときだけこれに来る)。
pub fn dotfiles_override_opts(main_rev: &str) -> Vec<String> {
    if main_rev.is_empty() {
        return Vec::new();
    }
    vec![
        "--override-input".into(),
        "dotfiles".into(),
        format!("github:tarotene/dotfiles/{main_rev}"),
        "--no-write-lock-file".into(),
    ]
}

/// 今回の適用が、直前の generation が既に deploy していた wrapper の private value
/// module を無警告で落とすものか(#567)。true なら中止。
///
/// - `had_marker_prev_gen`: 直前の generation の home-files が
///   `.config/dotfiles/private-hub` を deploy していたか(=wrapper 経由で適用済み、
///   home/modules/private-hub.nix)
/// - `wrapper_ref`: 今回の resolve_default_ref の結果
/// - `default_ref`: DEFAULT_REF(「今は wrapper 未登録」の検出用)
/// - `public_only`: `--public-only` が渡されたか
pub fn private_hub_downgrade_abort(
    had_marker_prev_gen: bool,
    wrapper_ref: &str,
    default_ref: &str,
    public_only: bool,
) -> bool {
    if public_only {
        return false; // 明示的な opt-out — 呼び出し側は分かってやっている
    }
    if wrapper_ref != default_ref {
        return false; // 今回も wrapper 登録済み — 降格ではない
    }
    had_marker_prev_gen // 前世代で登録済み、今回未登録 → 無警告の降格
}

// ---------------------------------------------------------------------------
// 環境の解決
// ---------------------------------------------------------------------------

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

/// マーカーファイルの 1 行目から空白をすべて除いたもの(`head -n1 | tr -d '[:space:]'`)。
/// マーカーが読めない・空なら None。
fn read_marker(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(config_home().join("dotfiles").join(name)).ok()?;
    let first = text.lines().next().unwrap_or("");
    let v: String = first.chars().filter(|c| !c.is_whitespace()).collect();
    (!v.is_empty()).then_some(v)
}

/// 既定の flake ref を解決する: マーカーファイルが先、DEFAULT_REF がフォールバック
/// (ADR-0034。resolve_host・docs/claude/writing-style.md の style-hub マーカーと同じ
/// 間接参照)。マーカーにより 1 ホストの `hms`(ref 無し)を、この repo のホスト
/// module の上に private value module を重ねる private wrapper flake へ向けられる —
/// この repo のソースがその flake を名指しすることなく。home-manager で宣言すると
/// 同じ絶対パスが管理下(store symlink)のファイルに戻ってしまい間接参照が無意味に
/// なるので、このマーカーは手置きで home-manager 管理にしない。
fn resolve_default_ref() -> String {
    read_marker("private-hub").unwrap_or_else(|| DEFAULT_REF.to_string())
}

fn hostname() -> String {
    Command::new("hostname")
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|s| s.trim_end().to_string())
        })
        .unwrap_or_default()
}

/// 論理ホスト名を解決する: マーカーファイルが先、`hostname` がフォールバック
/// (ADR-0019)。マーカーでホストが星の codename("altair"、"vega")を持てる。解決が
/// OS の hostname に依存することはない。Linux の rename runbook
/// (docs/cutover-runbook.md)は OS hostname も同じ codename にするので、マーカー未配置
/// でも `hostname` フォールバックが正しく解決する。各 codename ホストの module は同じ
/// switch で `xdg.configFile` によりマーカーを宣言し、以後は home-manager 管理。
fn resolve_host() -> String {
    read_marker("host").unwrap_or_else(hostname)
}

fn command_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| {
            p.is_file()
                && std::fs::metadata(p)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        })
}

/// `readlink -f` 相当(失敗は空)。
fn readlink_f(p: &Path) -> String {
    std::fs::canonicalize(p)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 成功した子の stdout(末尾改行除去)。失敗・起動不能は None。stderr は捨てる。
fn capture(cmd: &mut Command) -> Option<String> {
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

fn status_ok(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 子を素通しで実行し終了コードを返す。起動できなければ(シェルの command not found と
/// 同じ)127。
fn run_inherit(cmd: &mut Command) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match cmd.status() {
        Ok(st) => st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)),
        Err(e) => {
            eprintln!("hms: {}: {e}", cmd.get_program().to_string_lossy());
            127
        }
    }
}

// ---------------------------------------------------------------------------
// 診断(warn-only)
// ---------------------------------------------------------------------------

/// herdr server の陳腐化チェック(warn-only、#200): switch は `herdr` の store path を
/// 差し替えるが、動いている `herdr server` は再起動しない — 誰かが外から kill して
/// 起動し直すまで旧バイナリがメモリに載り続ける(docs/operations.md「Restarting herdr
/// after a switch...」)。hms は通常 herdr の pane の中で動くので自分のホストプロセスは
/// 再起動できず、warn するだけにする(check_generation_consistency と同じ形)。
fn check_herdr_staleness() {
    let Some(herdr) = command_path("herdr") else {
        return;
    };
    let current_bin = readlink_f(&herdr);
    let running_pid = capture(Command::new("pgrep").args(["-x", "herdr"]))
        .and_then(|s| s.lines().next().map(str::to_string))
        .unwrap_or_default();
    if current_bin.is_empty() || running_pid.is_empty() {
        return;
    }
    let running_bin = readlink_f(Path::new(&format!("/proc/{running_pid}/exe")));
    if running_bin.is_empty() {
        return;
    }
    // 両方が nix store に解決されるときだけ比較する — それ以外(wrapper script、
    // 非 nix のインストール)は陳腐化のシグナルではない。
    if !(current_bin.starts_with("/nix/store/") && running_bin.starts_with("/nix/store/")) {
        return;
    }
    if current_bin != running_bin {
        eprintln!("Warning: the running herdr server is still on the old binary.");
        eprintln!("  current generation: {current_bin}");
        eprintln!("  running (pid {running_pid}): {running_bin}");
        eprintln!("  Any change to herdr's server-side behavior will not take effect");
        eprintln!("  until you kill and relaunch it from outside herdr (see");
        eprintln!("  docs/operations.md, 'Restarting herdr after a switch...').");
    }
}

/// home-manager は `nix-env --profile --set`(current generation を進める)を、
/// activation script の本体が走る前に実行する。activation が途中で失敗する
/// (checkLinkTargets 等、#62/#63)と、current generation は新しい store path を指すのに
/// gcroots/current-home 配下の実際の home-files symlink は旧 generation のまま —
/// 「generation は進んだが世界は古い」状態になり、hms は検出しない(#65)。両者が
/// 食い違うときに警告する(失敗にはしない)。
///
/// `switch_already_failed`: この呼び出しの時点で *今回自身の* switch が既に失敗して
/// いるか(失敗後の呼び出しが true を渡す)— そうでないと締めの文が、今まさに失敗した
/// switch について「この switch が解消する」と言ってしまう。
fn check_generation_consistency(switch_already_failed: bool) {
    let profile_link = home().join(".local/state/nix/profiles/home-manager");
    let current_home_link = home().join(".local/state/home-manager/gcroots/current-home");
    if !(profile_link.exists() && current_home_link.exists()) {
        return;
    }
    let profile_target = readlink_f(&profile_link);
    let current_home_target = readlink_f(&current_home_link);
    if profile_target != current_home_target {
        eprintln!("Warning: home-manager generation/reality mismatch detected.");
        eprintln!("  profile ({}): {profile_target}", profile_link.display());
        eprintln!(
            "  current-home ({}): {current_home_target}",
            current_home_link.display()
        );
        eprintln!("  A previous activation likely failed partway through, leaving the");
        eprintln!("  generation pointer ahead of what is actually applied.");
        if switch_already_failed {
            eprintln!("  This switch also failed, so the mismatch remains — fix the error");
            eprintln!("  above and rerun; a successful switch will then resolve it.");
        } else {
            eprintln!("  A successful switch (this one) will resolve it.");
        }
    }
}

// ---------------------------------------------------------------------------
// メインの流れ
// ---------------------------------------------------------------------------

fn print_help(r#ref: &str, wrapper_ref: &str) {
    println!("Usage: hms [flake-ref] [--public-only]");
    println!();
    println!("Apply the home-manager configuration for this host.");
    println!("  hms          apply pushed main ({})", r#ref);
    println!("  hms .        apply the current checkout/worktree (pre-push verification).");
    if wrapper_ref != DEFAULT_REF {
        println!("               Routed through {wrapper_ref} with dotfiles overridden to");
        println!("               this checkout — pass --public-only to apply it alone instead.");
    }
    println!("  hms <path>   apply an arbitrary local checkout (same routing as `hms .`)");
    println!("  hms . --public-only   apply the checkout alone, dropping private value modules");
}

/// `nix flake metadata --refresh --json <flake>` の revision(解決できなければ None)。
fn refreshed_revision(flake: &str) -> Option<(String, String)> {
    let json =
        capture(Command::new("nix").args(["flake", "metadata", "--refresh", "--json", flake]))?;
    let rev = metadata_revision(&json);
    (!rev.is_empty()).then_some((json, rev))
}

/// 引数(argv[0] を除く)を処理して終了コードを返す。
pub fn run(args: &[String]) -> i32 {
    let wrapper_ref = resolve_default_ref();
    let mut r#ref = wrapper_ref.clone();
    let mut public_only = false;

    for a in args {
        match a.as_str() {
            "--help" | "-h" => {
                print_help(&r#ref, &wrapper_ref);
                return 0;
            }
            "--public-only" => public_only = true,
            s if s.starts_with('-') => {
                eprintln!("Error: Unknown option: {s}");
                return 1;
            }
            s => r#ref = s.to_string(),
        }
    }

    let host = resolve_host();

    check_generation_consistency(false);

    // private-hub 降格 guard (#567): このホストの直前の generation が既に wrapper
    // 経由で適用済み(home-files に .config/dotfiles/private-hub が存在する)なのに、
    // 今回の resolve_default_ref() が DEFAULT_REF(未登録)に縮退していて、かつ
    // --public-only の明示指定も無いなら、wrapper が配る私的な value module
    // (bleep の denylist config 等)を無警告で撤去する適用になる。実際にこの手順で
    // 起きた事故(2026-09-29、#567)を機械的に検知して止める。
    let profile_link = home().join(".local/state/nix/profiles/home-manager");
    let had_marker_prev_gen = profile_link.exists()
        && PathBuf::from(readlink_f(&profile_link))
            .join("home-files/.config/dotfiles/private-hub")
            .exists();
    if private_hub_downgrade_abort(had_marker_prev_gen, &wrapper_ref, DEFAULT_REF, public_only) {
        eprintln!("Error: 直前の generation は private wrapper flake 経由で適用済みでしたが、");
        eprintln!("  今回は ~/.config/dotfiles/private-hub マーカーが見つからず、public");
        eprintln!("  単体({DEFAULT_REF})に縮退します。このまま進めると wrapper が配った");
        eprintln!("  私的な value module(bleep の denylist config 等)が無警告で撤去されます");
        eprintln!("  (#567、2026-09-29 に実際に発生)。");
        eprintln!("  意図的な public 単体適用なら --public-only を明示してください。");
        eprintln!("  wrapper へ戻すなら次でマーカーを復旧してから再実行してください:");
        eprintln!(
            "    mkdir -p ~/.config/dotfiles && printf '%s\\n' '<wrapper flake ref>' > ~/.config/dotfiles/private-hub"
        );
        return 1;
    }

    let mut extra_opts: Vec<String> = Vec::new();

    // リモートの flake ref(github:、git+ssh: …)は nix がキャッシュするもの。ローカル
    // パス(`.` や checkout ディレクトリ)は常に現在のツリーを読むので refresh は不要。
    let ref_is_local = Path::new(&r#ref).exists();
    let ref_meta_json: String;
    if !ref_is_local {
        println!("==> nix flake metadata --refresh {}", r#ref);
        match refreshed_revision(&r#ref) {
            Some((json, rev)) => {
                println!("==> applying revision {rev}");
                ref_meta_json = json;
            }
            None => {
                eprintln!(
                    "==> could not resolve a revision for {} (offline?); continuing with whatever switch resolves",
                    r#ref
                );
                ref_meta_json = String::new();
            }
        }
    } else {
        // `hms .` はセッション途中でほぼ必ず dirty なローカル checkout/worktree を適用する
        // — それが push 前検証の目的そのもの。nix は毎回 "warning: Git tree '<path>' has
        // uncommitted changes" を繰り返す。ここ(ローカルパス分岐)だけで抑え、nix.conf で
        // マシン全体には効かせない(#149)— 非ローカル ref ではこの警告は出ない。
        extra_opts = vec!["--option".into(), "warn-dirty".into(), "false".into()];
        ref_meta_json = capture(Command::new("nix").args(["flake", "metadata", "--json", &r#ref]))
            .unwrap_or_default();
    }

    let ref_is_wrapper =
        !ref_meta_json.is_empty() && !wrapper_locked_dotfiles_rev(&ref_meta_json).is_empty();
    let plan = local_apply_plan(
        &wrapper_ref,
        DEFAULT_REF,
        ref_is_local,
        ref_is_wrapper,
        public_only,
    );

    // ローカル適用の routing(ADR-0034 Amendment 2026-09-27): wrapper 登録ホストでの
    // ローカルパス適用は、そのパス単体を適用して private value module(bleep 自身の
    // denylist config を含む)をすべて落とす代わりに、`dotfiles` をそのパスへ override
    // した wrapper 経由にする。
    let mut apply_ref = r#ref.clone();
    if plan == ApplyPlan::Route {
        // `realpath -m`(存在しなくても解決。ここでは ref_is_local なので存在する)
        let abs = std::fs::canonicalize(&r#ref)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(&r#ref));
        apply_ref = wrapper_ref.clone();
        extra_opts.extend([
            "--override-input".into(),
            "dotfiles".into(),
            format!("path:{}", abs.display()),
            "--no-write-lock-file".into(),
        ]);
    }

    // wrapper-lock override(ADR-0034 Amendment 2026-09-25): 適用対象 flake が
    // `dotfiles` input を持つ(wrapper、リモート/ローカル)なら、wrapper の lock を
    // 信用せず pushed main の解決済み revision で pin する。上の routing とは排他 —
    // routing は意図的に `dotfiles` をローカルパスに pin 済みで、pushed main の
    // revision の出番は無い。
    if plan != ApplyPlan::Route && !ref_meta_json.is_empty() {
        let locked = wrapper_locked_dotfiles_rev(&ref_meta_json);
        if !locked.is_empty() {
            match refreshed_revision(DEFAULT_REF) {
                Some((_, main_rev)) => {
                    if main_rev == locked {
                        println!(
                            "==> dotfiles revision {main_rev} (wrapper's lock already at this revision)"
                        );
                    } else {
                        println!(
                            "==> dotfiles revision {main_rev} (overriding the wrapper's lock, which pins {locked})"
                        );
                    }
                    extra_opts.extend(dotfiles_override_opts(&main_rev));
                }
                None => eprintln!(
                    "==> could not resolve {DEFAULT_REF} (offline?); applying the wrapper's lock as-is (dotfiles {locked})"
                ),
            }
        }
    }

    // ファイル冒頭の `--override-input` の注記を参照: nix 自身の "not writing modified
    // lock file" 出力の前に、説明なしで残さず警告する。
    if lock_warning_expected(&extra_opts) {
        println!(
            "==> note: a nix \"warning: not writing modified lock file\" below is expected — dotfiles is overridden for this switch only; the wrapper's flake.lock is intentionally left untouched (ADR-0034)"
        );
    }

    println!(
        "==> home-manager switch --flake {apply_ref}#{host} -b backup {}",
        extra_opts.join(" ")
    );
    let rc = run_inherit(
        Command::new("home-manager")
            .args(["switch", "--flake"])
            .arg(format!("{apply_ref}#{host}"))
            .args(["-b", "backup"])
            .args(&extra_opts),
    );
    if rc != 0 {
        check_generation_consistency(true);
        return rc;
    }

    // Tailscale prefs の収束(ADR-471、下の check_herdr_staleness と同じ warn-only —
    // Tailscale 未インストール/未認証で switch を失敗させてはならない)。Linux と
    // darwin 共通なので、下の Linux 専用の systemd/fcitx5 後処理より前に走る。
    if command_path("tailscale-prefs").is_some() {
        let rc = run_inherit(Command::new("tailscale-prefs").arg("apply"));
        if rc != 0 {
            return rc;
        }
    }

    // systemd --user と fcitx5 unit は Linux 専用(ADR-0018)。darwin ホスト(altair 等)
    // にはどちらも無いので後処理全体が no-op。
    if std::env::consts::OS != "linux" {
        println!("==> non-Linux host; skipping systemctl daemon-reload and fcitx5 restart.");
        println!("Done.");
        return 0;
    }

    println!("==> systemctl --user daemon-reload");
    let rc = run_inherit(Command::new("systemctl").args(["--user", "daemon-reload"]));
    if rc != 0 {
        return rc;
    }

    check_herdr_staleness();

    // fcitx5 unit の後処理 — unit の無いホストではきれいにスキップ。
    if !status_ok(Command::new("systemctl").args(["--user", "cat", FCITX5_UNIT])) {
        println!("==> {FCITX5_UNIT} not present; skipping fcitx5 restart.");
        println!("Done.");
        return 0;
    }

    println!("==> systemctl --user restart {FCITX5_UNIT}");
    let rc = run_inherit(Command::new("systemctl").args(["--user", "restart", FCITX5_UNIT]));
    if rc != 0 {
        return rc;
    }

    // 検証: restart は同期的で daemon-reload の後に走るので、MainPID が生きている active な
    // unit は構造上、新 generation の ExecStart で動いている。MainPID が正本 — プロセス名
    // (pgrep)や exe path での照合はここでは不可能: nixpkgs は fcitx5 を wrap する
    // (bin/fcitx5 -> .fcitx5-wrapped -> 本体)ので comm は ".fcitx5-wrapped" になり、
    // /proc/<pid>/exe は unit の ExecStart が指す wrapper の先へ解決される。
    if !status_ok(Command::new("systemctl").args(["--user", "is-active", "--quiet", FCITX5_UNIT])) {
        eprintln!("Error: {FCITX5_UNIT} is not active after the restart.");
        // `systemctl status --no-pager UNIT >&2 || true`
        let _ = Command::new("systemctl")
            .args(["--user", "status", "--no-pager", FCITX5_UNIT])
            .stdout(std::io::stderr())
            .status();
        return 1;
    }

    let pid = match capture_strict(Command::new("systemctl").args([
        "--user",
        "show",
        "-p",
        "MainPID",
        "--value",
        FCITX5_UNIT,
    ])) {
        Ok(p) => p,
        Err(rc) => return rc,
    };
    if pid.is_empty() || pid == "0" {
        eprintln!("Error: {FCITX5_UNIT} is active but has no MainPID.");
        return 1;
    }

    let running_bin = readlink_f(Path::new(&format!("/proc/{pid}/exe")));
    println!(
        "==> fcitx5 running (pid {pid}) from {}",
        if running_bin.is_empty() {
            "<unknown>"
        } else {
            &running_bin
        }
    );
    println!("Done.");
    0
}

/// `x="$(cmd)"` を `set -e` 下で実行する形: 失敗したらその終了コードで中断する。
fn capture_strict(cmd: &mut Command) -> Result<String, i32> {
    use std::os::unix::process::ExitStatusExt;
    match cmd.stdin(Stdio::null()).stderr(Stdio::inherit()).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout)
            .trim_end_matches('\n')
            .to_string()),
        Ok(o) => Err(o
            .status
            .code()
            .unwrap_or_else(|| 128 + o.status.signal().unwrap_or(0))),
        Err(e) => {
            eprintln!("hms: {}: {e}", cmd.get_program().to_string_lossy());
            Err(127)
        }
    }
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let rc = run(&args);
    ExitCode::from(u8::try_from(rc.rem_euclid(256)).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRAPPER: &str = "git+https://example.invalid/wrapper";

    // wrapper_locked_dotfiles_rev(selftest 1-3)

    #[test]
    fn rev_1_dotfiles_input_present_is_a_wrapper_flake() {
        let json = r#"{"locks":{"nodes":{
            "root":{"inputs":{"dotfiles":"dotfiles","nixpkgs":"nixpkgs"}},
            "dotfiles":{"locked":{"rev":"abc123def456","type":"github"}}
        }}}"#;
        assert_eq!(wrapper_locked_dotfiles_rev(json), "abc123def456");
    }

    #[test]
    fn rev_2_no_dotfiles_input() {
        let json = r#"{"locks":{"nodes":{"root":{"inputs":{"nixpkgs":"nixpkgs"}}}}}"#;
        assert_eq!(wrapper_locked_dotfiles_rev(json), "");
    }

    #[test]
    fn rev_3_dotfiles_input_is_a_follows_array() {
        let json = r#"{"locks":{"nodes":{"root":{"inputs":{"dotfiles":["nixpkgs","dotfiles"]}}}}}"#;
        assert_eq!(wrapper_locked_dotfiles_rev(json), "");
    }

    #[test]
    fn rev_garbage_json_is_empty() {
        assert_eq!(wrapper_locked_dotfiles_rev("not json"), "");
        assert_eq!(wrapper_locked_dotfiles_rev(""), "");
    }

    // dotfiles_override_opts(selftest 4-5)

    #[test]
    fn opts_4_non_empty_revision_gives_4_args() {
        let o = dotfiles_override_opts("90db04baaa54c598a2b5ba847adbb9451d2bc798");
        assert_eq!(
            o,
            [
                "--override-input",
                "dotfiles",
                "github:tarotene/dotfiles/90db04baaa54c598a2b5ba847adbb9451d2bc798",
                "--no-write-lock-file"
            ]
        );
    }

    #[test]
    fn opts_5_empty_revision_gives_nothing() {
        assert!(dotfiles_override_opts("").is_empty());
    }

    // local_apply_plan(selftest 6-10)

    #[test]
    fn plan_6_wrapper_host_local_public_path_routes() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, true, false, false),
            ApplyPlan::Route
        );
    }

    #[test]
    fn plan_7_no_wrapper_registered_is_asis() {
        assert_eq!(
            local_apply_plan(DEFAULT_REF, DEFAULT_REF, true, false, false),
            ApplyPlan::AsIs
        );
    }

    #[test]
    fn plan_8_public_only_is_asis_even_on_wrapper_host() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, true, false, true),
            ApplyPlan::AsIs
        );
    }

    #[test]
    fn plan_9_ref_is_itself_the_wrapper_checkout_is_asis() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, true, true, false),
            ApplyPlan::AsIs
        );
    }

    #[test]
    fn plan_10_non_local_ref_on_wrapper_host_is_asis() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, false, false, false),
            ApplyPlan::AsIs
        );
    }

    // private_hub_downgrade_abort(selftest 11-14)

    #[test]
    fn guard_11_marker_in_prev_gen_but_unregistered_now_aborts() {
        assert!(private_hub_downgrade_abort(
            true,
            DEFAULT_REF,
            DEFAULT_REF,
            false
        ));
    }

    #[test]
    fn guard_12_marker_in_prev_gen_and_still_registered_is_ok() {
        assert!(!private_hub_downgrade_abort(
            true,
            WRAPPER,
            DEFAULT_REF,
            false
        ));
    }

    #[test]
    fn guard_13_no_marker_in_prev_gen_is_ok() {
        assert!(!private_hub_downgrade_abort(
            false,
            DEFAULT_REF,
            DEFAULT_REF,
            false
        ));
    }

    #[test]
    fn guard_14_explicit_public_only_is_ok() {
        assert!(!private_hub_downgrade_abort(
            true,
            DEFAULT_REF,
            DEFAULT_REF,
            true
        ));
    }

    // lock_warning_expected(selftest 15-16)

    #[test]
    fn lock_15_override_input_present() {
        assert!(lock_warning_expected(&[
            "--override-input",
            "dotfiles",
            "path:/x",
            "--no-write-lock-file"
        ]));
    }

    #[test]
    fn lock_16_warn_dirty_only_is_not() {
        assert!(!lock_warning_expected(&["--option", "warn-dirty", "false"]));
        assert!(!lock_warning_expected::<&str>(&[]));
    }

    #[test]
    fn metadata_revision_semantics() {
        assert_eq!(metadata_revision(r#"{"revision":"abc"}"#), "abc");
        assert_eq!(metadata_revision(r#"{"revision":null}"#), "");
        assert_eq!(metadata_revision(r#"{}"#), "");
        assert_eq!(metadata_revision("oops"), "");
    }
}
