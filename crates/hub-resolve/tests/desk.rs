//! desk-hub(机の正本の解決、`hub_resolve::resolve_desk`)の統合テスト。実バイナリを、
//! 隔離した HOME と環境変数つきで起動する。会社の機体を想定した「`desk-hub` だけがあり、
//! `performance-hub` は無い」場合と、従来の「`performance-hub` だけがある」場合を分けて確かめる。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

const DEFAULT_CANON: &str = "state/reviews/daily/desk.md";

fn run(home: &Path, args: &[&str], env: &[(&str, &Path)]) -> Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_desk-hub"));
    cmd.args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// `rel` に正本のファイルがあるハブを作る。
fn hub_with(root: &Path, name: &str, rel: &str) -> PathBuf {
    let hub = root.join(name);
    let canon = hub.join(rel);
    fs::create_dir_all(canon.parent().unwrap()).unwrap();
    fs::write(canon, "# desk\n").unwrap();
    hub
}

/// 従来のハブ(`performance-hub` が指す。存在確認は state/performances/README.md)。
fn performance_hub(root: &Path) -> PathBuf {
    let hub = hub_with(root, "perf-hub", DEFAULT_CANON);
    let readme = hub.join("state/performances/README.md");
    fs::create_dir_all(readme.parent().unwrap()).unwrap();
    fs::write(readme, "# readme\n").unwrap();
    hub
}

fn marker(home: &Path, name: &str) -> PathBuf {
    let p = home.join(".config/dotfiles").join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    p
}

fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    (dir, home)
}

#[test]
fn unset_machine_fails_with_both_ways_to_set_it() {
    let (_dir, home) = setup();
    let o = run(&home, &[], &[]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(err.contains("机の正本のハブが未設定"), "{err}");
    assert!(err.contains("export DESK_HUB="), "{err}");
    assert!(err.contains(&marker(&home, "desk-hub").display().to_string()));
    assert!(err.contains("performance-hub"), "{err}");
    assert_eq!(stdout(&o), "");
}

#[test]
fn desk_hub_marker_alone_resolves_the_hub_and_the_default_canon() {
    // 会社の機体: `desk-hub` だけがあり、`performance-hub` は無い。
    let (dir, home) = setup();
    let hub = hub_with(dir.path(), "company-hub", DEFAULT_CANON);
    fs::write(marker(&home, "desk-hub"), format!("{}\n", hub.display())).unwrap();

    let o = run(&home, &[], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), hub.display().to_string());

    let o = run(&home, &["--canon"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), hub.join(DEFAULT_CANON).display().to_string());
}

#[test]
fn second_marker_line_names_the_canon_relative_to_the_hub() {
    let (dir, home) = setup();
    let hub = hub_with(dir.path(), "company-hub", "workday/desk.md");
    fs::write(
        marker(&home, "desk-hub"),
        format!("{}\nworkday/desk.md\n", hub.display()),
    )
    .unwrap();

    let o = run(&home, &["--canon"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        hub.join("workday/desk.md").display().to_string()
    );
}

#[test]
fn desk_canon_env_overrides_the_second_marker_line() {
    let (dir, home) = setup();
    let hub = hub_with(dir.path(), "company-hub", "other/desk.md");
    fs::write(
        marker(&home, "desk-hub"),
        format!("{}\nworkday/desk.md\n", hub.display()),
    )
    .unwrap();

    let o = run(
        &home,
        &["--canon"],
        &[("DESK_CANON", Path::new("other/desk.md"))],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), hub.join("other/desk.md").display().to_string());
}

#[test]
fn without_desk_hub_the_performance_hub_still_holds_the_desk() {
    // 従来の個人の機体: `performance-hub` だけ。
    let (dir, home) = setup();
    let hub = performance_hub(dir.path());
    let o = run(&home, &["--canon"], &[("PERFORMANCE_HUB", hub.as_path())]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), hub.join(DEFAULT_CANON).display().to_string());
}

#[test]
fn desk_hub_wins_over_performance_hub() {
    let (dir, home) = setup();
    let perf = performance_hub(dir.path());
    let desk = hub_with(dir.path(), "company-hub", DEFAULT_CANON);
    fs::write(marker(&home, "desk-hub"), format!("{}\n", desk.display())).unwrap();

    let o = run(&home, &[], &[("PERFORMANCE_HUB", perf.as_path())]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), desk.display().to_string());
}

#[test]
fn a_hub_without_the_canon_file_fails_loudly() {
    let (dir, home) = setup();
    let hub = dir.path().join("empty-hub");
    fs::create_dir_all(&hub).unwrap();
    fs::write(marker(&home, "desk-hub"), format!("{}\n", hub.display())).unwrap();

    let o = run(&home, &[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("机の正本が読めません"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn a_hub_path_that_does_not_exist_fails_loudly() {
    let (dir, home) = setup();
    let gone = dir.path().join("does-not-exist");
    fs::write(marker(&home, "desk-hub"), format!("{}\n", gone.display())).unwrap();

    let o = run(&home, &[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("存在しません"), "{}", stderr(&o));
}

#[test]
fn the_canon_path_must_stay_inside_the_hub() {
    let (dir, home) = setup();
    let hub = hub_with(dir.path(), "company-hub", DEFAULT_CANON);
    for bad in ["../outside.md", "/etc/passwd", "./desk.md"] {
        fs::write(
            marker(&home, "desk-hub"),
            format!("{}\n{bad}\n", hub.display()),
        )
        .unwrap();
        let o = run(&home, &["--canon"], &[]);
        assert_eq!(o.status.code(), Some(1), "{bad}");
        assert!(stderr(&o).contains("不正"), "{bad}: {}", stderr(&o));
        assert_eq!(stdout(&o), "", "{bad}");
    }
}

#[test]
fn unknown_arguments_are_a_usage_error() {
    let (_dir, home) = setup();
    let o = run(&home, &["--bogus"], &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("使い方"), "{}", stderr(&o));
}
