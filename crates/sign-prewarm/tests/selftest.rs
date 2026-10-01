//! `config/claude/hooks/sign-prewarm.sh --selftest` の全ケースを、gpg スタブと
//! 隔離した git config で再現する統合テスト(#413、docs/rust-migration.md の段 1-2)。
//!
//! テスト対象は既定で cargo bin(Rust 版)。`SIGN_PREWARM_ORACLE` に bash 版の
//! パスを入れると同じケースを bash 版に向けて走らせる(移植前の緑確認用)。
//! bash selftest は本体を `source` して `run_prewarm` を呼んでいたが、ここでは
//! hook として直接起動する(`GPG_BIN` を env で差し替えるのは同じ)。
//!
//! card/stub の合成入力(`gpg --list-secret-keys --with-colons --with-fingerprint`
//! の実機出力を雛形にした field 15 の 3 値)は bash selftest のものをそのまま使う
//! — `crates/gpg-subkey` の on-disk/card-backed フィルタの回帰テストも同じ手法を参照している。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GPG_STUB: &str = r#"#!/usr/bin/env bash
args="$*"
case "$args" in
  *--list-secret-keys*)
    if [[ -n "${SIGN_PREWARM_STUB_LIST_FILE:-}" && -f "$SIGN_PREWARM_STUB_LIST_FILE" ]]; then
      cat "$SIGN_PREWARM_STUB_LIST_FILE"
    fi
    exit 0
    ;;
  *--decrypt*)
    case "$args" in
      *--pinentry-mode\ error*)
        [[ "${SIGN_PREWARM_STUB_DECRYPT_WARM:-0}" == "1" ]] && exit 0
        exit 1
        ;;
      *--pinentry-mode\ ask*)
        if [[ -n "${SIGN_PREWARM_STUB_DECRYPT_CALLS_FILE:-}" ]]; then
          echo 1 >>"$SIGN_PREWARM_STUB_DECRYPT_CALLS_FILE"
        fi
        exit "${SIGN_PREWARM_STUB_DECRYPT_WARMUP_RC:-0}"
        ;;
    esac
    exit 0
    ;;
  *--pinentry-mode\ error*)
    [[ "${SIGN_PREWARM_STUB_WARM:-0}" == "1" ]] && exit 0
    exit 1
    ;;
  *--pinentry-mode\ ask*)
    if [[ -n "${SIGN_PREWARM_STUB_CALLS_FILE:-}" ]]; then
      echo 1 >>"$SIGN_PREWARM_STUB_CALLS_FILE"
    fi
    exit "${SIGN_PREWARM_STUB_WARMUP_RC:-0}"
    ;;
  *)
    exit 0
    ;;
esac
"#;

// 実機(gpg --list-secret-keys --with-colons --with-fingerprint)から取った行。
// field 15 は sec/ssb 行 19 フィールド中の 15 番目。
const LIST_ONDISK: &str = "\
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:u:255:22:8608A3F925E329CC:1783930808:1815466808:::::s:::+::ed25519::
fpr:::::::::57B25182FB450B06570860488608A3F925E329CC:
";
const LIST_CARD: &str = "\
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:u:255:22:4DB3C00BA34556B0:1753115911::::::a:::D2760001240100000006246379980000::ed25519::
fpr:::::::::CARDCARDCARDCARDCARDCARDCARDCARDCARDCARD:
";
const LIST_STUB: &str = "\
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:r:255:22:282AE4C1E0CAC57A:1753371700:1784907700:::::s:::#::ed25519::
fpr:::::::::STUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUB:
";

const ONDISK_KEY: &str = "57B25182FB450B06570860488608A3F925E329CC";

struct Fx {
    dir: tempfile::TempDir,
    n: std::cell::Cell<u32>,
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// run_case の引数(bash selftest の `run_case <cwd> <sign> <key> <fmt> [token]`)。
struct Case<'a> {
    cwd: &'a Path,
    sign: &'a str,
    key: &'a str,
    fmt: &'a str,
    token: Option<&'a Path>,
    env: Vec<(&'a str, String)>,
}

impl<'a> Case<'a> {
    fn new(cwd: &'a Path, sign: &'a str, key: &'a str, fmt: &'a str) -> Self {
        Case {
            cwd,
            sign,
            key,
            fmt,
            token: None,
            env: Vec::new(),
        }
    }
    fn env(mut self, k: &'a str, v: impl Into<String>) -> Self {
        self.env.push((k, v.into()));
        self
    }
    fn token(mut self, t: &'a Path) -> Self {
        self.token = Some(t);
        self
    }
}

fn target() -> Command {
    match std::env::var_os("SIGN_PREWARM_ORACLE") {
        Some(script) => {
            let mut c = Command::new("bash");
            c.arg(script);
            c
        }
        None => Command::new(env!("CARGO_BIN_EXE_sign-prewarm")),
    }
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("gpg-stub");
        std::fs::write(&stub, GPG_STUB).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fx = Fx {
            dir,
            n: std::cell::Cell::new(0),
        };
        for (n, body) in [
            ("list-ondisk.txt", LIST_ONDISK),
            ("list-card.txt", LIST_CARD),
            ("list-stub.txt", LIST_STUB),
        ] {
            std::fs::write(fx.p(n), body).unwrap();
        }
        let norepo = fx.p("norepo");
        std::fs::create_dir_all(norepo).unwrap();
        fx
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn list(&self, which: &str) -> String {
        self.p(&format!("list-{which}.txt"))
            .to_string_lossy()
            .into_owned()
    }

    fn run(&self, c: Case) -> Out {
        let n = self.n.get();
        self.n.set(n + 1);
        let home = self.p(&format!("home_{n}"));
        std::fs::create_dir_all(&home).unwrap();
        let gc = home.join("gitconfig");
        let gcs = gc.to_str().unwrap();
        git_cfg(gcs, "commit.gpgsign", c.sign);
        if !c.key.is_empty() {
            git_cfg(gcs, "user.signingkey", c.key);
        }
        if !c.fmt.is_empty() {
            git_cfg(gcs, "gpg.format", c.fmt);
        }
        // ESA_TOKEN_FILE は常に明示する — 既定では必ず存在しないパスにする。
        let token = c
            .token
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join(".config/esa/token.gpg"));
        let mut cmd = target();
        for (k, _) in std::env::vars() {
            if k.starts_with("SIGN_PREWARM_STUB_") || k == "XDG_CONFIG_HOME" {
                cmd.env_remove(k);
            }
        }
        cmd.current_dir(c.cwd)
            .env("GIT_CONFIG_GLOBAL", &gc)
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("HOME", &home)
            .env("GPG_BIN", self.p("bin/gpg-stub"))
            .env("ESA_TOKEN_FILE", &token)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.p("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        for (k, v) in &c.env {
            cmd.env(k, v);
        }
        let o = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        let tmp = self.dir.path().to_string_lossy().into_owned();
        Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8(o.stdout).unwrap().replace(&tmp, "<TMP>"),
            stderr: String::from_utf8(o.stderr).unwrap().replace(&tmp, "<TMP>"),
        }
    }

    /// 呼び出し記録ファイルの行数(無ければ 0 — bash の `cat … || true` が空)。
    fn calls(&self, name: &str) -> usize {
        std::fs::read_to_string(self.p(name))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    fn calls_file(&self, name: &str) -> String {
        self.p(name).to_string_lossy().into_owned()
    }
}

fn git_cfg(file: &str, key: &str, val: &str) {
    let _ = Command::new("git")
        .args(["config", "-f", file, key, val])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// stdout / stderr を `tests/expected/<name>.{stdout,stderr}` と完全一致させる
/// (一時ディレクトリは `<TMP>` に置換済み)。`SIGN_PREWARM_BLESS=1` で生成する。
fn snap(name: &str, out: &Out) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected");
    for (ext, got) in [("stdout", &out.stdout), ("stderr", &out.stderr)] {
        let f = dir.join(format!("{name}.{ext}"));
        if std::env::var_os("SIGN_PREWARM_BLESS").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&f, got).unwrap();
        } else {
            let want = std::fs::read_to_string(&f)
                .unwrap_or_else(|_| panic!("snapshot {} が無い", f.display()));
            assert_eq!(got, &want, "{name}.{ext}");
        }
    }
}

#[test]
fn out_of_scope_is_silent() {
    let fx = Fx::new();
    let norepo = fx.p("norepo");

    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk")),
    );
    assert_eq!(
        o.code, 0,
        "commit.gpgsign 未設定(グローバルのみ true): exit 0"
    );
    assert_eq!(o.stdout, "", "commit.gpgsign 未設定: stdout 空");

    let o = fx.run(
        Case::new(&norepo, "false", ONDISK_KEY, "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk")),
    );
    assert_eq!(o.code, 0, "commit.gpgsign=false: exit 0");
    assert_eq!(o.stdout, "", "commit.gpgsign=false: stdout 空");
    assert_eq!(o.stderr, "", "commit.gpgsign=false: stderr 空");

    let o = fx.run(
        Case::new(&norepo, "true", "", "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk")),
    );
    assert_eq!(o.code, 0, "user.signingkey 空: exit 0");
    assert_eq!(o.stdout, "", "user.signingkey 空: stdout 空");

    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "ssh")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk")),
    );
    assert_eq!(o.code, 0, "gpg.format=ssh: exit 0");
    assert_eq!(o.stdout, "", "gpg.format=ssh: stdout 空");
}

#[test]
fn field15_three_values() {
    let fx = Fx::new();
    let norepo = fx.p("norepo");

    let o = fx.run(
        Case::new(
            &norepo,
            "true",
            "CARDCARDCARDCARDCARDCARDCARDCARDCARDCARD",
            "openpgp",
        )
        .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("card"))
        .env("SIGN_PREWARM_STUB_WARM", "0")
        .env(
            "SIGN_PREWARM_STUB_CALLS_FILE",
            fx.calls_file("calls-card.txt"),
        ),
    );
    assert_eq!(o.code, 0, "card-backed(token S/N): exit 0");
    assert_eq!(
        fx.calls("calls-card.txt"),
        0,
        "card-backed: 本番 gpg を呼ばない"
    );

    let o = fx.run(
        Case::new(
            &norepo,
            "true",
            "STUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUB",
            "openpgp",
        )
        .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("stub"))
        .env("SIGN_PREWARM_STUB_WARM", "0")
        .env(
            "SIGN_PREWARM_STUB_CALLS_FILE",
            fx.calls_file("calls-stub.txt"),
        ),
    );
    assert_eq!(o.code, 0, "simple stub(#): exit 0");
    assert_eq!(
        fx.calls("calls-stub.txt"),
        0,
        "simple stub: 本番 gpg を呼ばない"
    );

    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_CALLS_FILE",
                fx.calls_file("calls-ondisk.txt"),
            ),
    );
    assert_eq!(o.code, 0, "オンディスク(+) かつ cold: exit 0");
    assert_eq!(
        fx.calls("calls-ondisk.txt"),
        1,
        "オンディスク かつ cold: 本番 gpg を 1 回呼ぶ"
    );

    // selftest 外: 指紋の前方一致(長い指紋の先頭だけを signingkey に書いた場合)も
    // オンディスク扱いになる(awk の index(fpr, want) == 1)。
    let o = fx.run(
        Case::new(&norepo, "true", "57B25182FB450B06", "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_CALLS_FILE",
                fx.calls_file("calls-prefix.txt"),
            ),
    );
    assert_eq!(o.code, 0);
    assert_eq!(fx.calls("calls-prefix.txt"), 1, "指紋の前方一致");

    // selftest 外: 末尾 16 桁の key id は指紋の前方一致にならないので対象外。
    let o = fx.run(
        Case::new(&norepo, "true", "8608A3F925E329CC", "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_CALLS_FILE",
                fx.calls_file("calls-keyid.txt"),
            ),
    );
    assert_eq!(o.code, 0);
    assert_eq!(fx.calls("calls-keyid.txt"), 0, "末尾 key id は一致しない");
}

#[test]
fn warmth() {
    let fx = Fx::new();
    let norepo = fx.p("norepo");

    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "1")
            .env(
                "SIGN_PREWARM_STUB_CALLS_FILE",
                fx.calls_file("calls-warm.txt"),
            ),
    );
    assert_eq!(o.code, 0, "既に warm: exit 0");
    assert_eq!(
        fx.calls("calls-warm.txt"),
        0,
        "既に warm: 本番 gpg を呼ばない"
    );
    assert_eq!(o.stdout, "", "既に warm: stdout 空");
    assert_eq!(o.stderr, "", "既に warm: stderr 空");

    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "0")
            .env("SIGN_PREWARM_STUB_WARMUP_RC", "1"),
    );
    assert_eq!(o.code, 0, "cold かつ本番失敗: exit 0");
    assert_eq!(o.stdout, "", "cold かつ本番失敗: stdout 空");
    assert!(!o.stderr.is_empty(), "cold かつ本番失敗: stderr 非空");
    snap("sign-warmup-failed", &o);
}

/// R1-A-2: local commit.gpgsign=false な repo から起動してもグローバル値で判定する。
#[test]
fn cwd_independent() {
    let fx = Fx::new();
    let repo = fx.p("nonsignrepo");
    std::fs::create_dir_all(&repo).unwrap();
    let ok = |args: &[&str]| {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success())
    };
    ok(&["init", "-q"]);
    ok(&["config", "commit.gpgsign", "false"]);

    let o = fx.run(
        Case::new(&repo, "true", ONDISK_KEY, "openpgp")
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_CALLS_FILE",
                fx.calls_file("calls-cwd.txt"),
            ),
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        fx.calls("calls-cwd.txt"),
        1,
        "local commit.gpgsign=false でもグローバル有効なら本番 gpg を 1 回呼ぶ"
    );
}

/// [E](esa MCP token.gpg)カバレッジ(#252)。
#[test]
fn esa_token() {
    let fx = Fx::new();
    let norepo = fx.p("norepo");
    let token = fx.p("token.gpg");
    std::fs::write(&token, "").unwrap();

    let o = fx.run(
        Case::new(&norepo, "false", "", "")
            .token(&token)
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_DECRYPT_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_DECRYPT_CALLS_FILE",
                fx.calls_file("calls-decrypt-cold.txt"),
            ),
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        fx.calls("calls-decrypt-cold.txt"),
        1,
        "token.gpg 存在 かつ cold: 本番 gpg(decrypt)を 1 回呼ぶ"
    );

    let o = fx.run(
        Case::new(&norepo, "false", "", "")
            .token(&token)
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_DECRYPT_WARM", "1")
            .env(
                "SIGN_PREWARM_STUB_DECRYPT_CALLS_FILE",
                fx.calls_file("calls-decrypt-warm.txt"),
            ),
    );
    assert_eq!(o.code, 0);
    assert_eq!(fx.calls("calls-decrypt-warm.txt"), 0);
    assert_eq!(o.stdout, "");
    assert_eq!(o.stderr, "");

    let o = fx.run(
        Case::new(&norepo, "false", "", "")
            .token(&token)
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_DECRYPT_WARM", "0")
            .env("SIGN_PREWARM_STUB_DECRYPT_WARMUP_RC", "1"),
    );
    assert_eq!(o.code, 0, "token.gpg 存在 かつ 本番失敗: exit 0");
    assert!(
        o.stderr.contains("<TMP>/token.gpg"),
        "stderr が token.gpg のパスに言及する"
    );
    snap("decrypt-warmup-failed", &o);

    let o = fx.run(Case::new(&norepo, "false", "", ""));
    assert_eq!(o.code, 0, "token.gpg 不在(既定): exit 0");
    assert_eq!(o.stdout, "");
    assert_eq!(o.stderr, "");
}

/// [S]/[E] 独立性の回帰(#252)。
#[test]
fn sign_and_decrypt_are_independent() {
    let fx = Fx::new();
    let norepo = fx.p("norepo");
    let token = fx.p("token.gpg");
    std::fs::write(&token, "").unwrap();

    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .token(&token)
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_CALLS_FILE",
                fx.calls_file("calls-both-sign.txt"),
            )
            .env("SIGN_PREWARM_STUB_DECRYPT_WARM", "0")
            .env(
                "SIGN_PREWARM_STUB_DECRYPT_CALLS_FILE",
                fx.calls_file("calls-both-decrypt.txt"),
            ),
    );
    assert_eq!(o.code, 0);
    assert_eq!(
        fx.calls("calls-both-sign.txt"),
        1,
        "[S] 側を 1 回呼ぶ(独立)"
    );
    assert_eq!(
        fx.calls("calls-both-decrypt.txt"),
        1,
        "[E] 側も 1 回呼ぶ(独立)"
    );

    // selftest 外: 両方失敗 → 対象ごとに 1 行ずつ、[S] → [E] の順。
    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .token(&token)
            .env("SIGN_PREWARM_STUB_LIST_FILE", fx.list("ondisk"))
            .env("SIGN_PREWARM_STUB_WARMUP_RC", "1")
            .env("SIGN_PREWARM_STUB_DECRYPT_WARMUP_RC", "1"),
    );
    assert_eq!(o.code, 0);
    snap("both-failed", &o);
}

/// selftest 外: GPG_BIN が解決できなければ完全沈黙(command -v ゲート)。
#[test]
fn missing_gpg_is_silent() {
    let fx = Fx::new();
    let norepo = fx.p("norepo");
    let o = fx.run(
        Case::new(&norepo, "true", ONDISK_KEY, "openpgp")
            .token(&fx.p("list-ondisk.txt"))
            .env("GPG_BIN", "/nonexistent/gpg"),
    );
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "");
    assert_eq!(o.stderr, "");
}
