//! 起動ラッパーの振る舞い。rsync / npx / firefox は一時ディレクトリ上のスタブで、
//! 実際のブラウザ・プロファイル・ネットワークには触れない。

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write_exec(p: &Path, body: &str) {
    fs::write(p, body).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
}

// 引数を 1 行ずつ記録し、stdout に雑音を出す(MCP の stdout を汚さないことの確認)。
// PATH を絞るテストでも動くよう、/bin/sh の組み込みだけを使う。
const RSYNC_STUB: &str = "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RSYNC_LOG\"\necho RSYNC-NOISE\nexit \"${RSYNC_RC:-0}\"\n";
// 引数を `|` 区切りで stdout に出す。
const NPX_STUB: &str = "#!/bin/sh\nfor a in \"$@\"; do printf '|%s' \"$a\"; done\n";

struct T {
    dir: tempfile::TempDir,
}

impl T {
    fn new() -> Self {
        let t = T {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(t.p("bin")).unwrap();
        write_exec(&t.p("bin/rsync"), RSYNC_STUB);
        write_exec(&t.p("bin/npx"), NPX_STUB);
        write_exec(&t.p("bin/firefox"), "#!/bin/sh\n");
        // 普段の Firefox: installs.ini が 1 つの Default= を指す。
        fs::create_dir_all(t.p("ff/p.default")).unwrap();
        fs::write(t.p("ff/p.default/cookies.sqlite"), "x").unwrap();
        fs::write(
            t.p("ff/installs.ini"),
            "[ABC]\nDefault=p.default\nLocked=1\n",
        )
        .unwrap();
        t
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn clone_dir(&self) -> PathBuf {
        self.p("cache/clone")
    }

    fn rsync_log(&self) -> String {
        fs::read_to_string(self.p("rsync.log")).unwrap_or_default()
    }

    /// 既定の引数(rsync・複製・firefox-dir)に `extra` を足して起動する。
    fn run(&self, extra: &[&str], envs: &[(&str, &str)]) -> Output {
        self.run_with_path(
            &format!("{}:/usr/bin:/bin", self.p("bin").display()),
            extra,
            envs,
        )
    }

    fn run_with_path(&self, path: &str, extra: &[&str], envs: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_playwright-mcp-launcher"));
        cmd.arg("--rsync")
            .arg(self.p("bin/rsync"))
            .arg("--clone")
            .arg(self.clone_dir())
            .arg("--firefox-dir")
            .arg(self.p("ff"))
            .args(extra)
            .env("PATH", path)
            .env("HOME", self.p("home"))
            .env("RSYNC_LOG", self.p("rsync.log"))
            .env_remove("RSYNC_RC");
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// 正常系: installs.ini の Default= を同期元に rsync し、複製で npx を起動する。
/// stdout は npx の出力だけで、rsync の雑音を含まない。
#[test]
fn syncs_default_profile_then_launches_npx_on_clone() {
    let t = T::new();
    let o = t.run(&["--", "--output-dir", "/o"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));

    let out = text(&o.stdout);
    assert!(
        !out.contains("RSYNC-NOISE"),
        "rsync の出力は stdout に出さない: {out}"
    );
    assert!(
        out.starts_with("|@playwright/mcp@0.0.83|--browser|moz-firefox"),
        "{out}"
    );
    let clone = t.clone_dir().canonicalize().unwrap();
    assert!(
        out.contains(&format!("|--user-data-dir|{}", clone.display())),
        "{out}"
    );
    assert!(out.ends_with("|--output-dir|/o"), "素通し引数は最後: {out}");

    let log = t.rsync_log();
    let src = t.p("ff/p.default").canonicalize().unwrap();
    assert!(log.contains("--delete"), "{log}");
    assert!(log.contains("--exclude=/user.js"), "{log}");
    assert!(log.contains("--exclude=/lock"), "{log}");
    assert!(
        log.contains(&format!("{}/", src.display())),
        "同期元は末尾 /: {log}"
    );
    assert!(log.contains(&format!("{}/", clone.display())), "{log}");

    let mode = fs::metadata(&clone).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "複製は本人だけが読める");
}

/// Linux: PATH の firefox を実体まで解決して `--executable-path` に渡す。
#[cfg(target_os = "linux")]
#[test]
fn passes_resolved_firefox_executable() {
    let t = T::new();
    let o = t.run(&[], &[]);
    let out = text(&o.stdout);
    let exe = t.p("bin/firefox").canonicalize().unwrap();
    assert!(
        out.contains(&format!("|--executable-path|{}", exe.display())),
        "{out}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn missing_firefox_fails_before_npx() {
    let t = T::new();
    fs::create_dir_all(t.p("bin-nofox")).unwrap();
    write_exec(&t.p("bin-nofox/npx"), NPX_STUB);
    let o = t.run_with_path(&t.p("bin-nofox").display().to_string(), &[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("firefox が見つからない"),
        "{}",
        text(&o.stderr)
    );
    assert!(text(&o.stdout).is_empty(), "npx は起動しない");
}

#[test]
fn explicit_source_profile_overrides_installs_ini() {
    let t = T::new();
    fs::create_dir_all(t.p("elsewhere")).unwrap();
    let o = t.run(
        &["--source-profile", t.p("elsewhere").to_str().unwrap()],
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let src = t.p("elsewhere").canonicalize().unwrap();
    assert!(t.rsync_log().contains(&format!("{}/", src.display())));
}

#[test]
fn installs_ini_with_no_or_many_defaults_is_refused() {
    let t = T::new();
    fs::write(t.p("ff/installs.ini"), "[A]\nLocked=1\n").unwrap();
    let o = t.run(&[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("--source-profile"),
        "{}",
        text(&o.stderr)
    );
    assert!(t.rsync_log().is_empty(), "rsync は呼ばない");

    fs::write(t.p("ff/installs.ini"), "[A]\nDefault=a\n[B]\nDefault=b\n").unwrap();
    let o = t.run(&[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("2 個"), "{}", text(&o.stderr));
    assert!(t.rsync_log().is_empty());
}

/// 複製の lock が生きた process を指す間は同期を飛ばす。npx は起動する。
#[test]
fn skips_sync_while_clone_firefox_is_alive() {
    let t = T::new();
    fs::create_dir_all(t.clone_dir()).unwrap();
    symlink(
        format!("127.0.0.1:+{}", std::process::id()),
        t.clone_dir().join("lock"),
    )
    .unwrap();
    let o = t.run(&[], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains("使用中"), "{}", text(&o.stderr));
    assert!(t.rsync_log().is_empty(), "使用中の複製へ rsync しない");
    assert!(text(&o.stdout).contains("--user-data-dir"));
}

/// 死んだ process を指す古い lock は使用中に数えない。
#[test]
fn stale_clone_lock_does_not_block_sync() {
    let t = T::new();
    fs::create_dir_all(t.clone_dir()).unwrap();
    symlink(
        format!("127.0.0.1:+{}", i32::MAX),
        t.clone_dir().join("lock"),
    )
    .unwrap();
    let o = t.run(&[], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!t.rsync_log().is_empty());
}

/// rsync の 24(転送中にファイルが消えた)は成功、それ以外の失敗は npx を起動しない。
#[test]
fn rsync_exit_codes() {
    let t = T::new();
    let o = t.run(&[], &[("RSYNC_RC", "24")]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));

    let o = t.run(&[], &[("RSYNC_RC", "23")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("rsync が失敗した"),
        "{}",
        text(&o.stderr)
    );
    assert!(
        text(&o.stdout).is_empty(),
        "同期に失敗したら npx は起動しない"
    );
}

/// `rsync --delete` が普段のプロファイルや兄弟を消す配置は、rsync を呼ぶ前に拒否する。
#[test]
fn clone_overlapping_source_is_refused() {
    let t = T::new();
    let source = t.p("ff/p.default");
    for bad in [t.p("ff"), source.clone(), source.join("clone")] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_playwright-mcp-launcher"));
        let o = cmd
            .arg("--rsync")
            .arg(t.p("bin/rsync"))
            .arg("--clone")
            .arg(&bad)
            .arg("--source-profile")
            .arg(&source)
            .env("RSYNC_LOG", t.p("rsync.log"))
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(1),
            "{}: {}",
            bad.display(),
            text(&o.stderr)
        );
        assert!(
            text(&o.stderr).contains("重なっている"),
            "{}",
            text(&o.stderr)
        );
    }
    assert!(t.rsync_log().is_empty(), "rsync は呼ばない");
}

#[test]
fn sync_only_does_not_launch_npx() {
    let t = T::new();
    let o = t.run(&["--sync-only"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(text(&o.stdout).is_empty());
    assert!(!t.rsync_log().is_empty());
}

#[test]
fn usage_error_exits_2() {
    let o = Command::new(env!("CARGO_BIN_EXE_playwright-mcp-launcher"))
        .arg("--bogus")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("使い方"));
}
