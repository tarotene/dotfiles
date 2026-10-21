//! `--version`(#807): いま動いている github-audit が、どの home-manager generation
//! (= dotfiles のどのコミット)で入ったかを出す。古い版を使い続けていること
//! (ADR-792 より前の版が無料プランの非公開リポジトリを誤報した、2026-10-15)を、
//! 判定を信じる前に気づけるようにする。
//!
//! generation の rev は、home-manager が `~/.local/share/dotfiles/rev` に置く 1 行の
//! テキストを実行時に読む。コンパイル時に環境変数で埋め込むと、コミットごとに
//! Rust のワークスペース全体が再ビルドされる(`cleanCargoSource` が文書だけの
//! 変更で再ビルドさせない作りを壊す)ので、バイナリには埋め込まない。

use std::process::Command;

fn run(home: &std::path::Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_github-audit"))
        .arg("--version")
        .env("HOME", home)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn version_prints_the_generation_rev() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".local/share/dotfiles");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rev"), "abc1234\n").unwrap();
    let (code, stdout) = run(home.path());
    assert_eq!(code, 0);
    assert_eq!(stdout, "github-audit 0.1.0 (generation abc1234)\n");
}

#[test]
fn version_without_a_rev_file_says_unknown() {
    let home = tempfile::tempdir().unwrap();
    let (code, stdout) = run(home.path());
    assert_eq!(code, 0);
    assert_eq!(stdout, "github-audit 0.1.0 (generation unknown)\n");
}
