//! app-window — ファイルを Chrome の専用窓(`--app=file://…`)で開く。`plan-view`(プランの HTML)と
//! `show-image`(画像)が共有する単一の正本(`docs/agents/agent-libraries.md`)。
//!
//! 設計と根拠: `docs/agents/plan-view.md`「ブラウザは非同期で起動しなければならない」「なぜ `--app` の専用窓なのか」。
//!
//! - 窓は毎回新規にする。通常のタブにすると既存の窓にタブが積み上がる。
//! - **切り離して(待たずに)起動する。** 同期にすると、既存の Chrome が無いホストで Chrome 本体が呼び出し側を
//!   掴んだまま居座り、PreToolUse hook なら承認ダイアログが出ない。Linux は `setsid`、darwin は `open -a`
//!   (LaunchServices に渡してすぐ戻る)。
//! - 表示できるかの判定は、バイナリ(darwin では `.app` バンドル)と画面の**存在**だけを見る(ADR-0005)。

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

/// 窓の開き方の設定。呼び出し側(plan-view・show-image)が、自分の環境変数から組み立てる。
#[derive(Debug, Clone)]
pub struct AppWindow {
    /// Linux のブラウザの実行ファイル(名前か絶対パス)。
    pub browser: String,
    /// `--window-size` の値(例: `1000,900`)。
    pub window_size: String,
    /// darwin の Chrome の `.app` バンドルのパス。
    pub darwin_chrome_app: String,
    /// `Darwin` か `Linux` を入れると、実機に依らずその分岐を通る(試験用)。空なら実機の OS。
    pub uname_override: String,
}

impl AppWindow {
    pub fn is_darwin(&self) -> bool {
        match self.uname_override.as_str() {
            "Darwin" => true,
            "Linux" => false,
            _ => std::env::consts::OS == "macos",
        }
    }

    /// darwin のネイティブ GUI セッションは DISPLAY の概念を持たないので常に真(#230)。
    pub fn has_display(&self) -> bool {
        if self.is_darwin() {
            return true;
        }
        ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
    }

    /// ADR-0005: バイナリ(darwin では .app バンドル)の**存在**でゲートする。
    pub fn browser_available(&self) -> bool {
        if self.is_darwin() {
            Path::new(&self.darwin_chrome_app).is_dir()
        } else {
            process_control::command_exists(&self.browser)
        }
    }

    /// 窓を開く 1 本のコマンド(先頭が実行ファイル)。`have_setsid` は Linux で切り離しに使う `setsid` の有無。
    pub fn argv(&self, file: &Path, have_setsid: bool) -> Vec<OsString> {
        let app = OsString::from(format!("--app={}", file_url(file)));
        let size = OsString::from(format!("--window-size={}", self.window_size));
        let mut v: Vec<OsString> = if self.is_darwin() {
            vec![
                "open".into(),
                "-a".into(),
                (&self.darwin_chrome_app).into(),
                "--args".into(),
            ]
        } else if have_setsid {
            vec!["setsid".into(), (&self.browser).into()]
        } else {
            vec![(&self.browser).into()]
        };
        v.push(app);
        v.push(size);
        v
    }

    /// `file` を専用窓で開き、待たずに戻る。起動できたかは問わない(呼び出し側は、先に
    /// [`browser_available`](Self::browser_available) と [`has_display`](Self::has_display) で見る)。
    pub fn open_file(&self, file: &Path) {
        let argv = self.argv(file, process_control::command_exists("setsid"));
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // 子は待たない(bash の `&` + `disown`)。
        let _ = cmd.spawn();
    }
}

/// `file://` の URL。パスのバイトのうち、予約されていない文字(`A-Za-z0-9-._~`)と `/` だけをそのまま書き、
/// ほかは `%XX` にする。`#`・`?`・`%` や空白・日本語を含むパスが、URL の別の部分と読まれない。
fn file_url(file: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut url = String::from("file://");
    for &b in file.as_os_str().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                url.push(b as char)
            }
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(uname: &str) -> AppWindow {
        AppWindow {
            browser: "google-chrome".into(),
            window_size: "1000,900".into(),
            darwin_chrome_app: "/Applications/Google Chrome.app".into(),
            uname_override: uname.into(),
        }
    }

    fn strings(v: Vec<OsString>) -> Vec<String> {
        v.into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn linux_detaches_with_setsid() {
        let argv = strings(window("Linux").argv(Path::new("/tmp/a b.png"), true));
        assert_eq!(
            argv,
            [
                "setsid",
                "google-chrome",
                "--app=file:///tmp/a%20b.png",
                "--window-size=1000,900"
            ]
        );
    }

    #[test]
    fn url_escapes_what_a_url_would_read_as_another_part() {
        assert_eq!(
            file_url(Path::new("/tmp/a#b?c%d.png")),
            "file:///tmp/a%23b%3Fc%25d.png"
        );
        assert_eq!(
            file_url(Path::new("/tmp/図.png")),
            "file:///tmp/%E5%9B%B3.png"
        );
        assert_eq!(file_url(Path::new("/a-b_c.d~e/f")), "file:///a-b_c.d~e/f");
    }

    #[test]
    fn linux_without_setsid_runs_the_browser_directly() {
        let argv = strings(window("Linux").argv(Path::new("/tmp/a.png"), false));
        assert_eq!(argv[0], "google-chrome");
        assert_eq!(argv.len(), 3);
    }

    #[test]
    fn darwin_hands_over_to_launch_services() {
        let argv = strings(window("Darwin").argv(Path::new("/tmp/a.png"), true));
        assert_eq!(
            argv,
            [
                "open",
                "-a",
                "/Applications/Google Chrome.app",
                "--args",
                "--app=file:///tmp/a.png",
                "--window-size=1000,900"
            ]
        );
    }

    #[test]
    fn darwin_needs_the_app_bundle_not_a_binary_on_path() {
        let d = tempfile::tempdir().unwrap();
        let mut w = window("Darwin");
        w.darwin_chrome_app = d.path().join("Chrome.app").to_string_lossy().into_owned();
        assert!(!w.browser_available());
        std::fs::create_dir(d.path().join("Chrome.app")).unwrap();
        assert!(w.browser_available());
    }

    #[test]
    fn linux_gates_on_the_binary_existing() {
        let mut w = window("Linux");
        w.browser = "definitely-not-a-real-browser".into();
        assert!(!w.browser_available());
    }

    #[test]
    fn darwin_always_has_a_display() {
        assert!(window("Darwin").has_display());
    }
}
