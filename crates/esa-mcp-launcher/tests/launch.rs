//! 旧 `esa-mcp-launcher --selftest` の 5 ケース(欠如・復号失敗・空トークン・
//! npx 不在・正常系のトークン受け渡し)。gpg / npx は PATH 上のスタブで、実際の
//! 資格情報・ネットワークには触れない。`ESA_MCP_LAUNCHER_UNDER_TEST` に bash 版を
//! 渡すと同じケースを bash 版に流せる。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn under_test() -> PathBuf {
    std::env::var_os("ESA_MCP_LAUNCHER_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_esa-mcp-launcher")))
}

fn write_exec(p: &Path, body: &str) {
    fs::write(p, body).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
}

const GPG_STUB: &str = "#!/usr/bin/env bash\n[ \"${GPG_STUB_FAIL:-0}\" = 1 ] && exit 2\nf=\"\"\nfor a in \"$@\"; do f=\"$a\"; done\ncat \"$f\"\n";
// 引数を 1 行ずつ + トークンを出す(受け渡しと素通し引数の両方を見る)。
const NPX_STUB: &str = "#!/usr/bin/env bash\nprintf '%s' \"${ESA_ACCESS_TOKEN:-}\"\nfor a in \"$@\"; do printf '|%s' \"$a\"; done\n";

struct T {
    dir: tempfile::TempDir,
}

impl T {
    fn new() -> Self {
        let t = T {
            dir: tempfile::tempdir().unwrap(),
        };
        let bin = t.dir.path().join("bin");
        let no_npx = t.dir.path().join("bin-no-npx");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&no_npx).unwrap();
        write_exec(&bin.join("gpg"), GPG_STUB);
        write_exec(&bin.join("npx"), NPX_STUB);
        write_exec(&no_npx.join("gpg"), GPG_STUB);
        t
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn run(
        &self,
        token_file: &Path,
        bin_dir: &str,
        extra_env: &[(&str, &str)],
        args: &[&str],
    ) -> Output {
        let path = format!("{}:/usr/bin:/bin", self.p(bin_dir).display());
        let mut cmd = Command::new(under_test());
        cmd.args(args)
            .env("ESA_TOKEN_FILE", token_file)
            .env("PATH", path)
            .env_remove("GPG_STUB_FAIL");
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// 1) トークンファイル欠如 → exit 1 + プロビジョニング誘導
#[test]
fn missing_token_file() {
    let t = T::new();
    let o = t.run(&t.p("absent.gpg"), "bin", &[], &[]);
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(err.contains("トークンファイルが無い"), "{err}");
    assert!(err.contains("docs/setup.md"), "ヒント行が出る: {err}");
    assert!(err.contains(t.p("absent.gpg").to_str().unwrap()));
}

/// 2) 復号失敗 → exit 1
#[test]
fn decrypt_failure() {
    let t = T::new();
    fs::write(t.p("token.gpg"), "sekrit-token").unwrap();
    let o = t.run(&t.p("token.gpg"), "bin", &[("GPG_STUB_FAIL", "1")], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("復号に失敗した"));
}

/// 3) 空トークン → exit 1(トークン内容は診断に出さない)
#[test]
fn empty_token() {
    let t = T::new();
    fs::write(t.p("empty.gpg"), "").unwrap();
    let o = t.run(&t.p("empty.gpg"), "bin", &[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("復号結果が空だった"));
}

/// 4) npx 不在 → exit 1
#[test]
fn missing_npx() {
    let t = T::new();
    fs::write(t.p("token.gpg"), "sekrit-token").unwrap();
    let o = t.run(&t.p("token.gpg"), "bin-no-npx", &[], &[]);
    // 実機の /usr/bin に npx があるホストではこのケースは成立しない(bash 版も同じ)。
    if !Path::new("/usr/bin/npx").exists() && !Path::new("/bin/npx").exists() {
        assert_eq!(o.status.code(), Some(1));
        assert!(text(&o.stderr).contains("npx が見つからない"));
    }
}

/// 5) 正常系 → exec された(スタブ)npx が ESA_ACCESS_TOKEN と固定引数を受け取る。
/// 末尾改行は落ちる(`$(...)` と同じ)。
#[test]
fn token_handoff_and_args() {
    let t = T::new();
    fs::write(t.p("token.gpg"), "sekrit-token\n\n").unwrap();
    let o = t.run(&t.p("token.gpg"), "bin", &[], &["--extra", "x y"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        text(&o.stdout),
        "sekrit-token|-y|@esaio/esa-mcp-server|--extra|x y"
    );
}

/// 診断にトークンを出さない(復号成功後の npx 不在でも)。
#[test]
fn token_never_in_diagnostics() {
    let t = T::new();
    fs::write(t.p("token.gpg"), "sekrit-token").unwrap();
    let o = t.run(&t.p("token.gpg"), "bin-no-npx", &[], &[]);
    assert!(!text(&o.stderr).contains("sekrit-token"));
    assert!(!text(&o.stdout).contains("sekrit-token"));
}

/// ESA_TOKEN_FILE 未設定時は XDG_CONFIG_HOME/esa/token.gpg を見る。
#[test]
fn default_token_path_uses_xdg() {
    let t = T::new();
    let xdg = t.p("xdg");
    fs::create_dir_all(xdg.join("esa")).unwrap();
    fs::write(xdg.join("esa/token.gpg"), "from-xdg").unwrap();
    let path = format!("{}:/usr/bin:/bin", t.p("bin").display());
    let o = Command::new(under_test())
        .env_remove("ESA_TOKEN_FILE")
        .env("XDG_CONFIG_HOME", &xdg)
        .env("PATH", path)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o.stdout).starts_with("from-xdg|"));
}
