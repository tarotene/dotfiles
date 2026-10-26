use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::Command;

#[test]
fn login_profile_and_interactive_zsh_resolve_launcher_and_leave_native_intact() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let native = home.join(".local/bin/codex");
    let launcher = home.join(".local/libexec/codex-launcher/codex");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&native, "#!/bin/sh\nexit 42\n").unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(env!("CARGO_BIN_EXE_codex-launcher"), &launcher).unwrap();
    let before = fs::read(&native).unwrap();
    for (shell, flags, source) in [
        ("sh", vec!["-c"], "config/shell/profile"),
        (
            "zsh",
            vec!["-f", "-i", "-c"],
            "config/zsh/modules/10-path.zsh",
        ),
    ] {
        let available = Command::new(shell).arg("--version").output();
        // zsh is provided by the dev shell in CI, but is optional outside it.
        if shell == "zsh" && available.is_err() {
            continue;
        }
        let o = Command::new(shell)
            .args(flags)
            .arg(". \"$1\"; command -v codex; codex --help")
            .arg("path-test")
            .arg(root.join(source))
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(42),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&o.stdout).trim(),
            launcher.to_str().unwrap()
        );
    }
    assert_eq!(fs::read(&native).unwrap(), before);
}
