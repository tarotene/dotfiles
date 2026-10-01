//! `scripts/dotfiles-doctor --selftest` の全 4 シナリオ(10 アサーション)を
//! 移した統合テスト(#414)。bash の selftest は `~/.local/bin` を外した安全な
//! PATH と、兄弟の無い隔離コピーで「writing-style-hub が本当に無い」状態を
//! 作っていた。ここでは PATH を stub だけの一時ディレクトリにし、バイナリを
//! 兄弟の無い一時ディレクトリへコピーして同じ状態を作る。

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

fn stub(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// バイナリを兄弟の無い一時ディレクトリへコピーして実行する。dotfiles-doctor は
/// `writing-style-hub` を PATH → 実行ファイルの隣の順に探すので、ワークスペース
/// 全体をビルドして target/ に `writing-style-hub` が並んでいても、「兄弟が無い」
/// 状態をテストが自分で作る(bash 版の「兄弟の無い隔離コピー」と同じ)。
fn run(home: &Path, path: &Path) -> Output {
    let iso = tempfile::tempdir().unwrap();
    let exe = iso.path().join("dotfiles-doctor");
    std::fs::copy(env!("CARGO_BIN_EXE_dotfiles-doctor"), &exe).unwrap();
    // コピー直後は、並列テストが fork した子が書き込み fd を継いでいる間
    // exec が ETXTBSY(26)になりうる。fd が閉じるまで短く再試行する。
    for _ in 0..50 {
        let r = Command::new(&exe)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("WRITING_STYLE_HUB")
            .env("HOME", home)
            .env("PATH", path)
            .output();
        match r {
            Ok(o) => return o,
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("{e}"),
        }
    }
    panic!("dotfiles-doctor を起動できませんでした(ETXTBSY が解消しない)");
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    std::fs::create_dir_all(home.join(".config/dotfiles")).unwrap();
    let bin = tempfile::tempdir().unwrap();
    stub(bin.path(), "hostname", "#!/bin/sh\necho teststar\n");
    (d, home, bin)
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn all_markers_unset_and_no_style_hub_binary_exits_zero() {
    let (_d, home, bin) = fixture();
    let o = run(&home, bin.path());
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("INFO host: unset"), "{out}");
    assert!(out.contains("(teststar)"), "{out}");
    assert!(out.contains("INFO private-hub: unset"), "{out}");
    assert!(
        out.contains("INFO style-hub: writing-style-hub バイナリが見つからない"),
        "{out}"
    );
}

#[test]
fn configured_host_and_private_hub_markers_are_ok() {
    let (_d, home, bin) = fixture();
    std::fs::write(home.join(".config/dotfiles/host"), "testhost\n").unwrap();
    std::fs::write(
        home.join(".config/dotfiles/private-hub"),
        "github:example/private\n",
    )
    .unwrap();
    let o = run(&home, bin.path());
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("OK   host: testhost"), "{out}");
    assert!(
        out.contains("OK   private-hub: github:example/private"),
        "{out}"
    );
}

#[test]
fn unresolved_style_hub_warns_and_exits_one() {
    let (_d, home, bin) = fixture();
    stub(
        bin.path(),
        "writing-style-hub",
        "#!/bin/sh\necho \"writing-style-hub: スタイルガイドのハブが未設定です。\" >&2\nexit 1\n",
    );
    let o = run(&home, bin.path());
    let out = text(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("WARN style-hub"), "{out}");
    // bash 版は行末に空白が 1 つ残る(tr が printf 自身の改行も空白にする)
    assert!(
        out.contains(
            "WARN style-hub: 未解決 — writing-style-hub: スタイルガイドのハブが未設定です。 \n"
        ),
        "{out:?}"
    );
}

#[test]
fn resolved_style_hub_is_ok_and_exits_zero() {
    let (_d, home, bin) = fixture();
    stub(
        bin.path(),
        "writing-style-hub",
        "#!/bin/sh\necho /path/to/hub\n",
    );
    let o = run(&home, bin.path());
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("OK   style-hub: /path/to/hub"), "{out}");
}
