//! `dotfiles-doctor` — このリポジトリが間接参照するホストローカルの marker
//! ファイルの状態を、実値を書かずに報告する(#368)。検出専用: marker を書かず、
//! ディレクトリも作らず、`~/.config/dotfiles/*` に触れない。
//! `scripts/dotfiles-doctor` の Rust 移植(ADR-0024、#414)。
//!
//! 設計と根拠: ADR-0034(実値は private wrapper flake、規則・スキーマだけが
//! public)D5「規則は public、実値は private」を守ったまま、3 つのマーカーが
//! 揃って未設置という状態(#368 の実測、2026-09-23: ~/.config/dotfiles/ 自体が
//! 不存在)を人間が発見できるようにする。style-hub だけを直す narrow fix では
//! host/private-hub の同時欠落を見逃すので、3 マーカーをまとめて検出する。
//!
//! 3 マーカーは意味が異なる — host と private-hub は「無ければ既定へ縮退する」
//! 任意設定、style-hub は「無ければ writing-style skill が使えない」必須設定:
//!
//! - `dotfiles/host`(ADR-0019): 無ければ `hostname` にフォールバックする。
//!   既存 3 ホストはこのマーカーを持たない設計そのものなので、未設置は INFO
//!   (正常な既定状態)。
//! - `dotfiles/private-hub`(ADR-0034): 無ければ `scripts/hms.sh` の
//!   DEFAULT_REF(github:tarotene/dotfiles)にフォールバックする。private
//!   wrapper flake を使わないホストでは未設置が正常。未設置は INFO。
//! - `dotfiles/style-hub`(#115、docs/claude/writing-style.md): フォール
//!   バック先は `$WRITING_STYLE_HUB` 環境変数のみで、どちらも無ければ
//!   writing-style skill が使用不能になる。未設置・解決不能は WARN。解決自体は
//!   `writing-style-hub` に委譲する(縮退・エラーメッセージのロジックを
//!   重複させない)。
//!
//! 終了コード: 0 = 全マーカーが OK か INFO(想定内の未設置含む)。
//!             1 = WARN が 1 件以上ある(style-hub が解決できない)。
//!
//! 差分: bash 版の `--selftest` は持たない(テストは `tests/*.rs`)。
//! `writing-style-hub` の兄弟探索は bash 版では `$0` の実体(scripts/)の隣だった
//! が、ここでは実行ファイル自身の隣(`current_exe`)。PATH 上に無く隣にも無い
//! 場合は INFO(ADR-0005: 未インストールとして扱う)。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_PRIVATE_HUB_REF: &str = "github:tarotene/dotfiles";

fn config_home() -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
    }
}

/// 先頭 1 行の全空白を除いたもの。読めない/空なら空文字(`head -n1 | tr -d '[:space:]'`)。
fn read_marker(path: &Path) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    let first = bytes.split(|&b| b == b'\n').next().unwrap_or(&[]);
    String::from_utf8_lossy(first)
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\x0b' | '\x0c'))
        .collect()
}

fn check_host(config: &Path) {
    let marker = config.join("dotfiles/host");
    let val = read_marker(&marker);
    if !val.is_empty() {
        println!("OK   host: {val} (marker {})", marker.display());
    } else {
        let host = Command::new("hostname")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim_end_matches('\n')
                    .to_string()
            })
            .unwrap_or_else(|| "?".to_string());
        println!(
            "INFO host: unset — falling back to `hostname` ({host}) — normal for hosts without a star-codename marker, ADR-0019"
        );
    }
}

fn check_private_hub(config: &Path) {
    let marker = config.join("dotfiles/private-hub");
    let val = read_marker(&marker);
    if !val.is_empty() {
        println!("OK   private-hub: {val} (marker {})", marker.display());
    } else {
        println!(
            "INFO private-hub: unset — falling back to {DEFAULT_PRIVATE_HUB_REF} (public-only apply) — normal for hosts with no private wrapper flake, ADR-0034"
        );
    }
}

fn find_style_hub_bin() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("writing-style-hub"))
            .find(|p| hook_io::proc::command_exists(&p.to_string_lossy()))
    });
    on_path.or_else(|| {
        let sibling = std::env::current_exe()
            .ok()
            .and_then(|e| e.canonicalize().ok())
            .and_then(|e| e.parent().map(|d| d.join("writing-style-hub")))?;
        hook_io::proc::command_exists(&sibling.to_string_lossy()).then_some(sibling)
    })
}

/// stdout/stderr を 1 本のパイプに合流させて走らせる(`"$bin" 2>&1`)。
/// 戻り値は (成功したか, 末尾改行を除いた出力)。起動できなければ失敗として
/// そのエラー文を出力扱いにする。
fn run_merged(bin: &Path) -> (bool, String) {
    let Ok((mut reader, writer)) = std::io::pipe() else {
        return (false, "pipe failed".to_string());
    };
    let mut cmd = Command::new(bin);
    cmd.stdin(Stdio::null());
    match writer.try_clone() {
        Ok(w) => cmd.stdout(w),
        Err(e) => return (false, e.to_string()),
    };
    cmd.stderr(writer);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, format!("{}: {e}", bin.display())),
    };
    // `cmd` が書き込み側 fd を握ったままだと EOF が来ないので先に落とす。
    drop(cmd);
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    let mut out = String::from_utf8_lossy(&buf).into_owned();
    while out.ends_with('\n') {
        out.pop();
    }
    (ok, out)
}

/// 戻り値 true = WARN(exit 1 の原因)。
fn check_style_hub() -> bool {
    let Some(bin) = find_style_hub_bin() else {
        println!(
            "INFO style-hub: writing-style-hub バイナリが見つからないため判定できません(ADR-0005: 未インストールとして扱う)"
        );
        return false;
    };
    let (ok, out) = run_merged(&bin);
    if ok {
        println!("OK   style-hub: {out} (writing-style-hub 経由で解決)");
        false
    } else {
        // bash: `printf 'WARN … — %s\n' "$out" | tr '\n' ' '; printf '\n'`
        // — printf 自身の末尾改行も空白になるので、行末に空白が 1 つ残る。
        println!("WARN style-hub: 未解決 — {} ", out.replace('\n', " "));
        true
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--selftest") {
        eprintln!("dotfiles-doctor: --selftest は `cargo test -p dotfiles-doctor` に移った(#414)");
        return;
    }
    let config = config_home();
    check_host(&config);
    check_private_hub(&config);
    let warned = check_style_hub();
    std::process::exit(i32::from(warned));
}
