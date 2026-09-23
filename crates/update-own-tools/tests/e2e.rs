//! 一時 git リポジトリとダミー crate に対して、実バイナリを end-to-end で走らせる。
//! 対象リポジトリのチェックアウト(HEAD・作業ツリー)が変わらないこと
//! (ADR-0025 Verification)と、一時 worktree が後片付けされることを確かめる。

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

struct Fixture {
    _tmp: tempfile::TempDir,
    clone: std::path::PathBuf,
    home: std::path::PathBuf,
}

/// upstream(bare)→ clone の 2 段を作り、clone は作業中の状態(別ブランチ・未コミット変更)にする。
fn fixture(install: &str) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let up = tmp.path().join("upstream.git");
    let seed = tmp.path().join("seed");
    std::fs::create_dir_all(seed.join("crates/cli")).unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(
        seed.join("crates/cli/Cargo.toml"),
        "[package]\nname = \"dummy\"\n",
    )
    .unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "init"]);
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            "--bare",
            seed.to_str().unwrap(),
            up.to_str().unwrap(),
        ],
    );
    let clone = tmp.path().join("clone");
    git(
        tmp.path(),
        &["clone", "-q", up.to_str().unwrap(), clone.to_str().unwrap()],
    );
    git(&clone, &["switch", "-q", "-c", "wip"]);
    std::fs::write(clone.join("dirty.txt"), "uncommitted").unwrap();
    let home = tmp.path().join("home");
    let cfg = home.join(".config/update-own-tools");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("registry.toml"),
        format!(
            "[[tool]]\nname = \"dummy\"\nrepo = \"{}\"\ncrate = \"crates/cli\"\ninstall = {install}\n",
            clone.display()
        ),
    )
    .unwrap();
    Fixture {
        _tmp: tmp,
        clone,
        home,
    }
}

fn run(f: &Fixture, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_update-own-tools"))
        .args(args)
        .env("HOME", &f.home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .output()
        .unwrap()
}

#[test]
fn builds_in_detached_worktree_and_cleans_up() {
    // install の代わりに、crate ディレクトリの Cargo.toml を target へコピーする
    let f = fixture(r#"["cp", "{dir}/Cargo.toml", "{cache}.installed"]"#);
    let head_before = git(&f.clone, &["rev-parse", "HEAD"]);
    let out = run(&f, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let installed = f
        .home
        .join(".cache/update-own-tools/target/dummy.installed");
    assert!(std::fs::read_to_string(installed)
        .unwrap()
        .contains("dummy"));
    // 対象のチェックアウトは不変、一時 worktree は残らない
    assert_eq!(git(&f.clone, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(&f.clone, &["branch", "--show-current"]).trim(), "wip");
    assert!(f.clone.join("dirty.txt").exists());
    assert_eq!(
        git(&f.clone, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

#[test]
fn failed_install_still_removes_worktree() {
    let f = fixture(r#"["false"]"#);
    let out = run(&f, &[]);
    assert!(!out.status.success());
    assert_eq!(
        git(&f.clone, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

#[test]
fn dry_run_touches_nothing() {
    let f = fixture(r#"["cp", "{dir}/Cargo.toml", "{cache}.installed"]"#);
    let out = run(&f, &["--dry-run"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("== dummy\n"));
    assert!(stdout.contains("worktree add --quiet --detach"));
    assert!(!f.home.join(".cache").exists());
}

#[test]
fn unknown_name_and_missing_registry_fail() {
    let f = fixture(r#"["true"]"#);
    assert!(!run(&f, &["nope"]).status.success());
    std::fs::remove_file(f.home.join(".config/update-own-tools/registry.toml")).unwrap();
    let out = run(&f, &["--dry-run"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("docs/update-own-tools.md"));
}

#[test]
fn url_repo_is_mirrored_into_cache() {
    let f = fixture(r#"["cp", "{dir}/Cargo.toml", "{cache}.installed"]"#);
    // repo を clone のパスではなく upstream の file:// URL に差し替える
    let up = f.clone.parent().unwrap().join("upstream.git");
    let reg = f.home.join(".config/update-own-tools/registry.toml");
    let text = std::fs::read_to_string(&reg).unwrap().replace(
        &f.clone.display().to_string(),
        &format!("file://{}", up.display()),
    );
    std::fs::write(&reg, text).unwrap();
    for _ in 0..2 {
        // 2 回目は既存の bare ミラーを fetch するだけ
        let out = run(&f, &[]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let bare = f.home.join(".cache/update-own-tools/repos/dummy.git");
    assert_eq!(
        git(&bare, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    assert!(f
        .home
        .join(".cache/update-own-tools/target/dummy.installed")
        .exists());
}
