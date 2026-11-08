//! `issue-inbox --sweep` の外から見える振る舞い(ADR-0000)。
//!
//! 一時ディレクトリの vault と状態ディレクトリを環境変数で宣言し、実際のバイナリを起動する。
//! 掃除の中身(14 日の境界・再発での復帰・裁定の手順)は `vault.rs` の単体テストが担い、ここは
//! 引数の扱い・dry-run が書かないこと・二度目が何も書かないことだけを確かめる。

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn days_ago(n: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    issue_inbox::vault::jst_date(now - n * 86_400)
}

fn note(vault: &Path, name: &str, status: &str, last: &str) {
    fs::create_dir_all(vault.join("inbox")).unwrap();
    fs::write(
        vault.join(format!("inbox/{name}.md")),
        format!(
            "---\ntitle: \"{name}\"\nrepo: \"o/r\"\nstatus: \"{status}\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"{last}\"\n---\n- {last} d\n"
        ),
    )
    .unwrap();
}

fn status(vault: &Path, name: &str) -> String {
    let text = fs::read_to_string(vault.join(format!("inbox/{name}.md"))).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix("status: "))
        .unwrap()
        .trim_matches('"')
        .to_string()
}

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("vault");
        fs::create_dir_all(vault.join(".obsidian")).unwrap();
        fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
        Env { tmp }
    }
    fn vault(&self) -> std::path::PathBuf {
        self.tmp.path().join("vault")
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_issue-inbox"))
            .args(args)
            .env("HOME", "/nonexistent-home")
            .env("OBSIDIAN_VAULT", self.vault())
            .env("WRAPUP_STATE_DIR", self.tmp.path().join("state"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_STATE_HOME")
            .output()
            .unwrap()
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_dry_run_counts_and_writes_nothing_then_the_sweep_lapses_and_repeats_harmlessly() {
    let e = Env::new();
    note(&e.vault(), "recent", "open", &days_ago(13));
    note(&e.vault(), "stale", "open", &days_ago(14));
    note(&e.vault(), "closed", "fixed", &days_ago(30));

    let dry = e.run(&["--sweep", "--dry-run"]);
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert!(
        stdout(&dry).contains("open 2 件のうち、1 件が失効の対象"),
        "{}",
        stdout(&dry)
    );
    assert_eq!(status(&e.vault(), "stale"), "open");

    let real = e.run(&["--sweep"]);
    assert!(real.status.success());
    assert!(
        stdout(&real).contains("1 件が失効した"),
        "{}",
        stdout(&real)
    );
    assert_eq!(status(&e.vault(), "stale"), "lapsed");
    assert_eq!(status(&e.vault(), "recent"), "open");
    assert_eq!(status(&e.vault(), "closed"), "fixed");

    // 二度目は何も書かない(冪等)。
    let before = fs::read(e.vault().join("inbox/stale.md")).unwrap();
    let again = e.run(&["--sweep"]);
    assert!(
        stdout(&again).contains("0 件が失効した"),
        "{}",
        stdout(&again)
    );
    assert_eq!(fs::read(e.vault().join("inbox/stale.md")).unwrap(), before);
}

#[test]
fn as_of_estimates_a_later_day_but_only_as_a_dry_run() {
    let e = Env::new();
    note(&e.vault(), "n", "open", &days_ago(10));
    let later = e.run(&["--sweep", "--dry-run", "--as-of", &days_ago(0)]);
    assert!(
        stdout(&later).contains("0 件が失効の対象"),
        "{}",
        stdout(&later)
    );
    // 10 日たった気づきは、4 日後に 14 日になる。日付は暦の文字列なので、ここは固定の日付で確かめる。
    note(&e.vault(), "fixed-date", "open", "2026-10-25");
    let est = e.run(&["--sweep", "--dry-run", "--as-of", "2026-11-08"]);
    assert!(
        stdout(&est).contains("1 件が失効の対象"),
        "{}",
        stdout(&est)
    );
    assert_eq!(status(&e.vault(), "fixed-date"), "open");

    for args in [
        vec!["--sweep", "--as-of", "2026-11-08"],
        vec!["--sweep", "--dry-run", "--as-of"],
        vec!["--sweep", "--dry-run", "--as-of", "nonsense"],
        vec!["--sweep", "--bogus"],
    ] {
        let o = e.run(&args);
        assert_eq!(o.status.code(), Some(1), "{args:?}");
    }
    assert_eq!(status(&e.vault(), "fixed-date"), "open");
}

#[test]
fn a_machine_without_a_vault_has_nothing_to_sweep() {
    let tmp = tempfile::tempdir().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_issue-inbox"))
        .arg("--sweep")
        .env("HOME", tmp.path())
        .env_remove("OBSIDIAN_VAULT")
        .env_remove("XDG_CONFIG_HOME")
        .env("WRAPUP_STATE_DIR", tmp.path().join("state"))
        .output()
        .unwrap();
    assert!(o.status.success());
    assert!(stdout(&o).contains("対象なし"), "{}", stdout(&o));
}
