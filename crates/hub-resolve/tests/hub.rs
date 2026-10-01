//! 旧 `writing-style-hub --selftest` / `performance-hub --selftest`(各 5 グループ、
//! check 8 件)を 2 bin 分に写したもの。実バイナリを環境変数つきで起動する。
//! `WRITING_STYLE_HUB_UNDER_TEST` / `PERFORMANCE_HUB_UNDER_TEST` に bash 版の
//! パスを渡すと同じケースを bash 版に流せる。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Case {
    bin: &'static str,
    under_test_env: &'static str,
    exe: &'static str,
    env_var: &'static str,
    marker: &'static str,
    readme: &'static str,
}

const WSH: Case = Case {
    bin: "writing-style-hub",
    under_test_env: "WRITING_STYLE_HUB_UNDER_TEST",
    exe: env!("CARGO_BIN_EXE_writing-style-hub"),
    env_var: "WRITING_STYLE_HUB",
    marker: "style-hub",
    readme: "docs/style/README.md",
};

const PH: Case = Case {
    bin: "performance-hub",
    under_test_env: "PERFORMANCE_HUB_UNDER_TEST",
    exe: env!("CARGO_BIN_EXE_performance-hub"),
    env_var: "PERFORMANCE_HUB",
    marker: "performance-hub",
    readme: "state/performances/README.md",
};

impl Case {
    fn run(&self, home: &Path, env: &[(&str, &Path)]) -> Output {
        let exe = std::env::var_os(self.under_test_env)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(self.exe));
        let mut cmd = Command::new(exe);
        cmd.env("HOME", home)
            .env_remove("WRITING_STYLE_HUB")
            .env_remove("PERFORMANCE_HUB")
            .env_remove("XDG_CONFIG_HOME");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    fn marker_file(&self, home: &Path) -> PathBuf {
        let p = home.join(".config/dotfiles").join(self.marker);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    }

    fn good_hub(&self, root: &Path) -> PathBuf {
        let hub = root.join("hub-ok");
        let readme = hub.join(self.readme);
        fs::create_dir_all(readme.parent().unwrap()).unwrap();
        fs::write(readme, "# readme\n").unwrap();
        hub
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn check_all(c: &Case) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let marker = c.marker_file(&home);
    let hub_ok = c.good_hub(dir.path());

    // 1) マーカー未設定・環境変数も無し → 失敗 + 明示メッセージ(設定例を含む)
    let o = c.run(&home, &[]);
    assert_eq!(o.status.code(), Some(1), "{}: マーカー未設定は非0", c.bin);
    assert!(stderr(&o).contains("未設定"));
    assert!(stderr(&o).contains(&format!("export {}=", c.env_var)));
    assert!(stderr(&o).contains(&marker.display().to_string()));
    assert_eq!(stdout(&o), "");

    // 2) マーカーが正しいハブを指す → 成功してパスを stdout に出す(末尾改行なし)
    fs::write(&marker, format!("{}\n", hub_ok.display())).unwrap();
    let o = c.run(&home, &[]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), hub_ok.display().to_string());

    // 3) 環境変数がマーカーより優先される(マーカーが壊れていても環境変数が勝つ)
    fs::write(&marker, "/does/not/exist\n").unwrap();
    let via_env = dir.path().join("hub-ok/../hub-ok");
    let o = c.run(&home, &[(c.env_var, via_env.as_path())]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), via_env.display().to_string());

    // 4) マーカーが存在しないパスを指す → 失敗
    let gone = dir.path().join("does-not-exist");
    fs::write(&marker, format!("{}\n", gone.display())).unwrap();
    let o = c.run(&home, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("存在しません"));

    // 5) ハブは存在するが README が無い → 失敗
    let no_readme = dir.path().join("hub-no-readme");
    fs::create_dir_all(no_readme.join(Path::new(c.readme).parent().unwrap())).unwrap();
    fs::write(&marker, format!("{}\n", no_readme.display())).unwrap();
    let o = c.run(&home, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("が読めません"));
}

#[test]
fn writing_style_hub() {
    check_all(&WSH);
}

#[test]
fn performance_hub() {
    check_all(&PH);
}

/// XDG_CONFIG_HOME が優先され、空文字は未設定扱い(`${VAR:-default}`)。
#[test]
fn xdg_config_home_is_honoured_and_empty_means_unset() {
    for c in [&WSH, &PH] {
        let dir = tempfile::tempdir().unwrap();
        let hub_ok = c.good_hub(dir.path());
        let xdg = dir.path().join("xdg");
        fs::create_dir_all(xdg.join("dotfiles")).unwrap();
        fs::write(
            xdg.join("dotfiles").join(c.marker),
            format!("{}\n", hub_ok.display()),
        )
        .unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let o = c.run(&home, &[("XDG_CONFIG_HOME", xdg.as_path())]);
        assert_eq!(o.status.code(), Some(0), "{}", c.bin);
        assert_eq!(stdout(&o), hub_ok.display().to_string());
        // 環境変数が空文字なら未設定扱いでマーカーへ落ちる(ここでは未設定 → 失敗)
        let o = c.run(
            &home,
            &[
                ("XDG_CONFIG_HOME", Path::new("")),
                (c.env_var, Path::new("")),
            ],
        );
        assert_eq!(o.status.code(), Some(1));
    }
}
