//! `tailscale-prefs` — home-manager が宣言した identity ごとの Tailscale 設定
//! (ADR-471、`home/modules/tailscale.nix`)を `tailscale set` で収束させる。
//! `scripts/tailscale-prefs` の Rust 移植(ADR-0024、#414)。
//!
//! prefs ファイルは `key=value` 行の閉語彙で、`home/modules/tailscale.nix` が
//! `dotfiles.tailscale.*` option(identity が person-level の値、private wrapper
//! flake が `exit_node` の実値を宣言する)から生成する:
//!
//! ```text
//! exit_node=<mullvad-node-name-or-empty>
//! exit_node_allow_lan_access=true|false
//! shields_up=true|false
//! ssh=true|false
//! ```
//!
//! `exit_node` の空値は意図的(company identity は既定で exit node 無し。
//! カフェ Wi-Fi の間だけ手動で選ぶ — docs/operations.md の "Café Wi-Fi")で、
//! 以前に設定された exit node を消すために `--exit-node=` を出す。
//!
//! `apply` は `hms` が switch のたびに呼ぶ(herdr-staleness 検査と同じ
//! warn-only: Tailscale が未導入・未認証でも switch を失敗させない)。純粋で
//! テスト可能な核が [`build_set_args`] で、閉語彙の外のキーは hard fail する
//! — その失敗を `apply` が警告に落とす。
//!
//! 唯一の差: bash 版の `--selftest` は持たない(テストは `tests/*.rs`、
//! `cargo test -p tailscale-prefs`)。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const KNOWN_KEYS: [&str; 4] = [
    "exit_node",
    "exit_node_allow_lan_access",
    "shields_up",
    "ssh",
];

const USAGE: &str = "\
Usage: tailscale-prefs apply [--prefs-file <path>]
       tailscale-prefs --selftest

Apply the declared Tailscale prefs (exit_node, exit_node_allow_lan_access,
shields_up, ssh) via `tailscale set`. Warn-only: a missing tailscale binary, an
unauthenticated device, or a missing prefs file never fails the caller (hms).
";

/// `build_set_args` の失敗。bash 版は `$(build_set_args …)` の stdout だけを
/// 取り込み、失敗前に出た行(`partial`)を `warn "$out"` にそのまま渡して
/// いたので、同じ挙動を保つために途中までのフラグも返す。`message` は bash では
/// stderr に直接出ていた診断。
#[derive(Debug, PartialEq, Eq)]
pub struct BuildError {
    pub partial: Vec<String>,
    pub message: String,
}

/// prefs ファイルを読み、`tailscale set` のフラグを 1 つずつ返す。閉語彙の外の
/// キー・`key=value` でない行・ファイル不在は失敗(typo や古い宣言を黙って
/// 無視しない)。
pub fn build_set_args(prefs_file: &Path) -> Result<Vec<String>, BuildError> {
    let display = prefs_file.display();
    let bytes = std::fs::read(prefs_file).map_err(|_| BuildError {
        partial: Vec::new(),
        message: format!("build_set_args: no such file: {display}"),
    })?;
    let text = String::from_utf8_lossy(&bytes);
    // bash の `while IFS= read -r line || [[ -n $line ]]`: 末尾改行なしの最終行も
    // 読み、CR は取らない(`str::lines` は CR を落とすので使わない)。
    let mut pieces: Vec<&str> = text.split('\n').collect();
    if pieces.last() == Some(&"") {
        pieces.pop();
    }
    let mut out = Vec::new();
    for (i, line) in pieces.iter().enumerate() {
        let lineno = i + 1;
        if line.is_empty() || line.trim_start_matches(is_posix_space).starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(BuildError {
                partial: out,
                message: format!(
                    "build_set_args: {display}:{lineno}: not a key=value line: {line}"
                ),
            });
        };
        if !KNOWN_KEYS.contains(&key) {
            return Err(BuildError {
                partial: out,
                message: format!("build_set_args: {display}:{lineno}: unknown key: {key}"),
            });
        }
        out.push(match key {
            "exit_node" => format!("--exit-node={value}"),
            "exit_node_allow_lan_access" => format!("--exit-node-allow-lan-access={value}"),
            "shields_up" => format!("--shields-up={value}"),
            "ssh" => format!("--ssh={value}"),
            // KNOWN_KEYS 検査を通ったキーだけがここへ来る。語彙にキーを足して
            // この match を直し忘れたら、暗黙に別の flag へ流れず落ちる。
            other => unreachable!("key {other} is in KNOWN_KEYS but has no flag mapping"),
        });
    }
    Ok(out)
}

/// bash の `[[:space:]]`(C ロケールの ASCII 空白)。
fn is_posix_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

fn warn(msg: &str) {
    eprintln!("Warning: {msg}");
}

fn default_prefs_file() -> PathBuf {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
    };
    base.join("dotfiles").join("tailscale-prefs")
}

/// `tailscale-prefs apply`。どの縮退経路も exit 0(hms を落とさない)。
pub fn cmd_apply(args: &[String]) -> i32 {
    let mut prefs_file = default_prefs_file();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--prefs-file" => match it.next() {
                Some(v) => prefs_file = PathBuf::from(v),
                None => {
                    eprintln!("Error: --prefs-file requires a value");
                    return 1;
                }
            },
            other => {
                eprintln!("Error: unknown option: {other}");
                return 1;
            }
        }
    }

    if !prefs_file.is_file() {
        warn(&format!(
            "no Tailscale prefs declared at {} — skipping",
            prefs_file.display()
        ));
        return 0;
    }
    if !hook_io::proc::command_exists("tailscale") {
        warn("tailscale not installed (see scripts/install-packages.sh) — skipping");
        return 0;
    }
    let logged_in = Command::new("tailscale")
        .arg("status")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !logged_in {
        warn("tailscale is installed but not logged in — skipping (run: sudo tailscale up)");
        return 0;
    }

    let flags = match build_set_args(&prefs_file) {
        Ok(f) => f,
        Err(e) => {
            // bash 版は診断を stderr に直接出した上で、`$(…)` が拾った途中までの
            // stdout を `warn` に渡していた(空なら "Warning: " だけ)。
            eprintln!("{}", e.message);
            warn(&e.partial.join("\n"));
            warn("declared Tailscale prefs are malformed — skipping");
            return 0;
        }
    };
    if flags.is_empty() {
        warn(&format!(
            "no recognized prefs in {} — skipping",
            prefs_file.display()
        ));
        return 0;
    }

    println!("Applying Tailscale prefs: {}", flags.join(" "));
    let ok = Command::new("tailscale")
        .arg("set")
        .args(&flags)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        warn("tailscale set failed — skipping (host prefs unchanged)");
    }
    0
}

/// バイナリの入口。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("apply") => cmd_apply(&args[1..]),
        Some("--selftest") => {
            eprintln!(
                "tailscale-prefs: --selftest は `cargo test -p tailscale-prefs` に移った(#414)"
            );
            0
        }
        Some("-h") | Some("--help") => {
            print!("{USAGE}");
            0
        }
        _ => {
            eprint!("{USAGE}");
            1
        }
    }
}
