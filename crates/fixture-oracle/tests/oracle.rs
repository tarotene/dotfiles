//! `tests/cases/<bin>/*.toml` を bash 版の実体に向けて走らせる(#391)。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// 実行ビットの無いスクリプト(配備側が `bash '<path>'` / `sh '<path>'` で
/// 起動しているもの、#413)は、shebang の interpreter で起動する薄い wrapper を
/// 一時ディレクトリに作って登録する。
fn runnable(bin: &str, script: PathBuf) -> PathBuf {
    let mode = std::fs::metadata(&script)
        .map(|m| m.permissions().mode())
        .unwrap_or(0);
    if mode & 0o111 != 0 {
        return script;
    }
    let first = std::fs::read_to_string(&script)
        .ok()
        .and_then(|s| s.lines().next().map(str::to_string))
        .unwrap_or_default();
    let interp = first
        .strip_prefix("#!")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("/bin/sh")
        .to_string();
    let dir = std::env::temp_dir().join(format!("fixture-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("wrapper dir");
    let wrapper = dir.join(bin);
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nexec {interp} '{}' \"$@\"\n", script.display()),
    )
    .expect("write wrapper");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("chmod wrapper");
    wrapper
}

#[test]
fn bash_oracles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let t = trycmd::TestCases::new();
    for (bin, rel) in fixture_oracle::BASH_ORACLES {
        t.register_bin(*bin, runnable(bin, root.join(rel)));
        t.case(format!("tests/cases/{bin}/*.toml"));
    }
}
