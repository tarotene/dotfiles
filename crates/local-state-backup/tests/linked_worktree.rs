use std::{fs, process::Command};

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn restore_linked_worktree_and_common_dir_to_new_paths() {
    let Some(restic) = std::env::var_os("TEST_RESTIC_BIN") else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let source = root.join("source");
    let linked = root.join("linked");
    run(Command::new("git")
        .args(["init", "-q", "-b", "fixture-root"])
        .arg(&source));
    run(Command::new("git").arg("-C").arg(&source).args([
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--allow-empty",
        "-qm",
        "fixture history",
    ]));
    run(Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["worktree", "add", "-q", "-b", "fixture"])
        .arg(&linked));
    fs::write(linked.join("untracked"), b"pending\n").unwrap();
    let repo = root.join("store");
    let restic_command = || {
        let mut cmd = Command::new(&restic);
        cmd.env("RESTIC_REPOSITORY", &repo)
            .env("RESTIC_PASSWORD", "fixture")
            .env("XDG_CACHE_HOME", root);
        cmd
    };
    run(restic_command().arg("init"));
    run(restic_command()
        .arg("backup")
        .arg(source.join(".git"))
        .arg(&linked));
    let recovered = root.join("recovered");
    let common = recovered.join("source/.git");
    let worktree = recovered.join("linked");
    run(restic_command()
        .arg("restore")
        .arg(format!("latest:{}", source.join(".git").display()))
        .arg("--target")
        .arg(&common)
        .arg("--verify"));
    run(restic_command()
        .arg("restore")
        .arg(format!("latest:{}", linked.display()))
        .arg("--target")
        .arg(&worktree)
        .arg("--verify"));
    run(Command::new("git")
        .arg("-C")
        .arg(recovered.join("source"))
        .args(["worktree", "repair"])
        .arg(&worktree));
    let output = Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"fixture history\n");
    assert_eq!(fs::read(worktree.join("untracked")).unwrap(), b"pending\n");
    let output = Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        std::path::Path::new(std::str::from_utf8(&output.stdout).unwrap().trim()),
        common
    );
}
