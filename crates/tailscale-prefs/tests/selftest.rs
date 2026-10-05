//! `scripts/tailscale-prefs --selftest` の全 9 ケースを移した統合テスト
//! (#414、docs/rust-migration.md の段 1-2)。bash の selftest は純粋核
//! `build_set_args` 6 ケース + `apply` の warn-only 縮退 3 ケースで、ここでも
//! 同じ順に並べる。`apply` 側は PATH を stub だけにして実バイナリを起動する
//! (実 tailscaled には触れない)。末尾に bash selftest が持たなかった
//! 正常系(`tailscale set` まで到達する)を 1 ケース足してある。

use std::path::Path;
use std::process::{Command, Output};

use tailscale_prefs::build_set_args;

fn write(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn apply(path_dir: &Path, prefs: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tailscale-prefs"))
        .args(["apply", "--prefs-file"])
        .arg(prefs)
        .env("PATH", path_dir)
        .output()
        .unwrap()
}

fn stub_tailscale(dir: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("tailscale");
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const VALID: &str =
    "exit_node=mullvad-nrt-1\nexit_node_allow_lan_access=true\nshields_up=false\nssh=true\n";

#[test]
fn valid_prefs_build_four_flags() {
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "valid", VALID);
    assert_eq!(
        build_set_args(&f).unwrap(),
        [
            "--exit-node=mullvad-nrt-1",
            "--exit-node-allow-lan-access=true",
            "--shields-up=false",
            "--ssh=true"
        ]
    );
}

#[test]
fn ssh_false_emits_disabling_flag() {
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "p", "ssh=false\n");
    assert_eq!(build_set_args(&f).unwrap(), ["--ssh=false"]);
}

#[test]
fn empty_exit_node_still_emits_clearing_flag() {
    let d = tempfile::tempdir().unwrap();
    let f = write(
        d.path(),
        "p",
        "exit_node=\nexit_node_allow_lan_access=true\nshields_up=true\n",
    );
    assert_eq!(build_set_args(&f).unwrap()[0], "--exit-node=");
}

#[test]
fn comments_and_blank_lines_are_skipped() {
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "p", "# a comment\nshields_up=true\n\n");
    assert_eq!(build_set_args(&f).unwrap(), ["--shields-up=true"]);
}

#[test]
fn unknown_key_fails_hard_and_names_the_key() {
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "p", "exit_node=x\nsome_future_key=y\n");
    let e = build_set_args(&f).unwrap_err();
    assert!(e.message.contains("unknown key: some_future_key"), "{e:?}");
    // bash 版は失敗前に出た行も stdout に残る(apply が warn に渡す)
    assert_eq!(e.partial, ["--exit-node=x"]);
}

#[test]
fn malformed_line_fails_hard() {
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "p", "this is not a kv line\n");
    assert!(build_set_args(&f).is_err());
}

#[test]
fn missing_file_fails_hard() {
    let d = tempfile::tempdir().unwrap();
    assert!(build_set_args(&d.path().join("does-not-exist")).is_err());
}

#[test]
fn last_line_without_newline_is_read() {
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "p", "shields_up=true");
    assert_eq!(build_set_args(&f).unwrap(), ["--shields-up=true"]);
}

#[test]
fn apply_missing_prefs_file_warns_and_exits_zero() {
    let d = tempfile::tempdir().unwrap();
    let o = apply(d.path(), &d.path().join("does-not-exist"));
    assert!(o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("no Tailscale prefs declared"), "{err}");
}

#[test]
fn apply_tailscale_absent_warns_and_exits_zero() {
    let d = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let f = write(d.path(), "valid", VALID);
    let o = apply(empty.path(), &f);
    assert!(o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("tailscale not installed"), "{err}");
}

#[test]
fn apply_not_logged_in_warns_and_exits_zero() {
    let d = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    // `status` だけ理解し(失敗する)、他は大声で拒否する stub — `tailscale set` を
    // 呼ぶ回帰が起きれば exit 99 で検出できる。
    stub_tailscale(
        bin.path(),
        "#!/bin/sh\ncase \"$1\" in status) exit 1 ;; *) echo \"unexpected tailscale invocation: $*\" >&2; exit 99 ;; esac\n",
    );
    let f = write(d.path(), "valid", VALID);
    let o = apply(bin.path(), &f);
    assert!(o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("not logged in"), "{err}");
    assert!(!err.contains("unexpected tailscale invocation"), "{err}");
}

#[test]
fn apply_happy_path_runs_tailscale_set() {
    let d = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let log = d.path().join("calls");
    stub_tailscale(
        bin.path(),
        &format!(
            "#!/bin/sh\ncase \"$1\" in status) exit 0 ;; set) shift; echo \"$@\" >> {}; exit 0 ;; esac\n",
            log.display()
        ),
    );
    let f = write(d.path(), "valid", VALID);
    let o = apply(bin.path(), &f);
    assert!(o.status.success());
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.contains("Applying Tailscale prefs: --exit-node=mullvad-nrt-1"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().trim(),
        "--exit-node=mullvad-nrt-1 --exit-node-allow-lan-access=true --shields-up=false --ssh=true"
    );
}

#[test]
fn apply_malformed_prefs_warns_and_exits_zero() {
    let d = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    stub_tailscale(
        bin.path(),
        "#!/bin/sh\ncase \"$1\" in status) exit 0 ;; *) exit 99 ;; esac\n",
    );
    let f = write(d.path(), "p", "exit_node=x\nbogus=1\n");
    let o = apply(bin.path(), &f);
    assert!(o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("unknown key: bogus"), "{err}");
    assert!(
        err.contains("declared Tailscale prefs are malformed"),
        "{err}"
    );
}
