//! 実際の実行ファイルを起動して、失敗が通常のセッションに倒れないことと、exec の渡し方を確かめる。

use std::path::PathBuf;
use std::process::Command;

fn modes_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/agents/modes")
}

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_agent-mode"));
    // 実環境の設定・逃げ道を読ませない。
    c.env_remove("DOTFILES_AGENT_MODE_INJECTION");
    c
}

#[test]
fn check_prints_the_shipped_mode() {
    let out = bin()
        .args(["check", "refactor", "--dir"])
        .arg(modes_dir())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["mode"], "refactor");
    assert_eq!(v["env"]["DOTFILES_AGENT_MODE"], "refactor");
}

#[test]
fn an_unknown_mode_exits_2_and_does_not_start_the_program() {
    let marker = tempfile::tempdir().unwrap().keep().join("ran");
    let out = bin()
        .args(["exec", "typo", "--dir"])
        .arg(modes_dir())
        .args(["--", "touch"])
        .arg(&marker)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        !marker.exists(),
        "モードが読めないのに、プログラムが起動された"
    );
}

#[test]
fn exec_runs_the_program_with_the_mode_flags_first_and_the_mode_environment() {
    use std::os::unix::fs::PermissionsExt;
    // `claude` の代わりの台本: 受け取った引数を 1 行ずつと、環境変数を出す。
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("stub");
    std::fs::write(
        &stub,
        "#!/bin/sh\nfor a in \"$@\"; do echo \"ARG:$a\"; done\nenv\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

    let out = bin()
        .args(["exec", "refactor", "--dir"])
        .arg(modes_dir())
        .arg("--")
        .arg(&stub)
        .args(["-p", "hello"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    let argv: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("ARG:"))
        .collect();
    assert_eq!(argv[0], "--settings", "{argv:?}");
    assert!(argv[1].contains("Read(**/docs/adr/**)"), "{argv:?}");
    assert_eq!(argv[2], "--append-system-prompt-file", "{argv:?}");
    assert!(argv[3].ends_with("refactor.md"), "{argv:?}");
    assert_eq!(
        &argv[4..],
        ["-p", "hello"],
        "利用者の引数はモードのフラグの後ろ"
    );
    assert!(
        text.lines().any(|l| l == "DOTFILES_AGENT_MODE=refactor"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l == "DOTFILES_AGENT_MODE_INJECTION=off"),
        "{text}"
    );
}

#[test]
fn inject_gate_does_not_run_the_hook_while_injection_is_off() {
    let off = bin()
        .env("DOTFILES_AGENT_MODE_INJECTION", "off")
        .args(["inject-gate", "--", "/usr/bin/false"])
        .status()
        .unwrap();
    assert_eq!(
        off.code(),
        Some(0),
        "止めている間は、元の hook を実行しない"
    );
    let on = bin()
        .args(["inject-gate", "--", "/usr/bin/false"])
        .status()
        .unwrap();
    assert_eq!(
        on.code(),
        Some(1),
        "止めていないときは、元の hook の終了コードがそのまま出る"
    );
}
