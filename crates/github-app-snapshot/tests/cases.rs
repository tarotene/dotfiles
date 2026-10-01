//! bash 版 `scripts/github-app-snapshot --selftest` の写し(#414)。
//!
//! selftest の各検査(`bash#N`)を、バイナリの入出力に言い換えて 1 対 1 で固定する
//! (JWT の 5 検査 bash#1〜#5 は src/jwt.rs の単体テスト)。外部コマンド
//! (curl / bws / secret-tool)は `GITHUB_APP_SNAPSHOT_*_BIN` で差し替えるスタブで、
//! 実際の資格情報・ネットワークには触れない。スタブと openssl の実行に bash と
//! openssl が要る(bash 版 selftest と同じ前提)。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_github-app-snapshot"));
    for k in [
        "GITHUB_APP_SNAPSHOT_CONFIG_DIR",
        "GITHUB_APP_SNAPSHOT_STATE_DIR",
        "GITHUB_APP_SNAPSHOT_MANIFESTS_DIR",
        "GITHUB_APP_SNAPSHOT_BWS_BIN",
        "GITHUB_APP_SNAPSHOT_SECRET_TOOL_BIN",
        "GITHUB_APP_SNAPSHOT_CURL_BIN",
        "GITHUB_APP_SNAPSHOT_OPENSSL_BIN",
        "GITHUB_APP_RELEASER_ID",
        "GITHUB_APP_RELEASER_PEM",
        "XDG_RUNTIME_DIR",
    ] {
        c.env_remove(k);
    }
    c
}

fn out(c: &mut Command) -> Out {
    let o = c.output().unwrap();
    Out {
        code: o.status.code().unwrap(),
        stdout: String::from_utf8(o.stdout).unwrap(),
        stderr: String::from_utf8(o.stderr).unwrap(),
    }
}

fn write_exec(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    let mut p = fs::metadata(path).unwrap().permissions();
    p.set_mode(0o755);
    fs::set_permissions(path, p).unwrap();
}

const MANIFEST: &str = r#"{"name": "tarotene-releaser", "url": "https://example.invalid", "hook_attributes": {"url": "https://example.invalid", "active": false}}"#;

struct Fx {
    tmp: TempDir,
}

impl Fx {
    fn new() -> Fx {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("manifests")).unwrap();
        fs::create_dir_all(tmp.path().join("state")).unwrap();
        fs::write(
            tmp.path().join("manifests/releaser.json"),
            format!("{MANIFEST}\n"),
        )
        .unwrap();
        Fx { tmp }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }
}

/// bash#6 の curl スタブ(URL ごとに固定の応答)。
const FAKE_CURL: &str = r#"#!/usr/bin/env bash
set -euo pipefail
url="${*: -1}"
case "$url" in
  */app) printf '{"slug":"tarotene-releaser","name":"tarotene-releaser","permissions":{"contents":"write","issues":"write","pull_requests":"write","metadata":"read"},"events":[]}' ;;
  */app/installations) printf '[{"id":111,"account":{"login":"tarotene"},"repository_selection":"selected"}]' ;;
  */app/installations/111/access_tokens) printf '{"token":"fake-install-token"}' ;;
  */installation/repositories\?per_page=100\&page=1) printf '{"repositories":[{"full_name":"tarotene/telepath"},{"full_name":"tarotene/bleep"}]}' ;;
  */installation/repositories\?per_page=100\&page=2) printf '{"repositories":[]}' ;;
  *) printf '{}' ;;
esac
"#;

fn genrsa(path: &Path) -> String {
    let s = Command::new("openssl")
        .args(["genrsa", "-out"])
        .arg(path)
        .arg("2048")
        .output()
        .unwrap();
    assert!(s.status.success());
    fs::read_to_string(path).unwrap()
}

fn snapshot_cmd(fx: &Fx, curl: &Path, out_path: &Path) -> Command {
    let mut c = bin();
    c.args(["__run", "snapshot", "--out"])
        .arg(out_path)
        .env("GITHUB_APP_SNAPSHOT_MANIFESTS_DIR", fx.p("manifests"))
        .env("GITHUB_APP_SNAPSHOT_CURL_BIN", curl);
    c
}

/// bash#6〜#10: do_snapshot(スタブ curl)。出力 JSON の形、install 先の
/// ページング終了、PEM が出力に出ないこと。
#[test]
fn snapshot_assembles_apps_and_installations() {
    let fx = Fx::new();
    let pem = genrsa(&fx.p("test.pem"));
    let curl = fx.p("curl");
    write_exec(&curl, FAKE_CURL);
    let target = fx.p("state/app-snapshot.json");
    let o = out(snapshot_cmd(&fx, &curl, &target)
        .env("GITHUB_APP_RELEASER_ID", "999")
        .env("GITHUB_APP_RELEASER_PEM", &pem));
    // bash#6: 書き出しを報告する
    assert_eq!(
        o.stdout,
        format!("github-app-snapshot: wrote {}\n", target.display())
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let text = fs::read_to_string(&target).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    // bash#7: schema
    assert_eq!(v["schema"], 1);
    // bash#8: apps[0]
    assert_eq!(v["apps"][0]["name"], "releaser");
    assert_eq!(v["apps"][0]["id"], 999);
    assert_eq!(v["apps"][0]["permissions"]["contents"], "write");
    // bash#9: installations[0].repositories(ページング終了と形)
    assert_eq!(
        v["apps"][0]["installations"][0]["repositories"],
        serde_json::json!(["tarotene/telepath", "tarotene/bleep"])
    );
    // bash#10: PEM は決して出力に載らない
    assert!(!text.contains("BEGIN"));
    // jq の整形・キー順(bash 版の `jq -n` と同じ): permissions は入力順のまま
    assert!(text.starts_with("{\n  \"schema\": 1,\n  \"generated_at\": \""));
    let perms = text.find("\"contents\"").unwrap();
    assert!(perms < text.find("\"metadata\"").unwrap());
    assert!(text.ends_with("}\n"));
    // 0600 の一時ファイルを rename するので、出力も 0600
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

/// bash#11〜#12: App の secret が環境に無ければ警告して飛ばし、致命にしない。
#[test]
fn missing_app_secret_is_skipped_not_fatal() {
    let fx = Fx::new();
    let curl = fx.p("curl");
    write_exec(&curl, FAKE_CURL);
    let target = fx.p("state/app-snapshot-empty.json");
    let o = out(&mut snapshot_cmd(&fx, &curl, &target));
    assert!(
        o.stderr.contains("no GITHUB_APP_RELEASER_ID"),
        "{}",
        o.stderr
    );
    assert_eq!(o.code, 0);
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(v["apps"], serde_json::json!([]));
    // 空配列は jq と同じく `[]`
    assert!(fs::read_to_string(&target)
        .unwrap()
        .contains("\"apps\": []\n"));
}

/// bash#13〜#16: manifest_form が実際に submit できるページを作る(manifest 属性は
/// 二重引用符で、元の manifest JSON に復号でき、script の要素 id は引用符付き)。
#[test]
fn manifest_form_page_submits() {
    let fx = Fx::new();
    let form = fx.p("form.html");
    let o = out(bin()
        .args(["manifest-form", "releaser", "--out"])
        .arg(&form)
        .env("GITHUB_APP_SNAPSHOT_MANIFESTS_DIR", fx.p("manifests")));
    assert_eq!(o.code, 0, "{}", o.stderr);
    let html = fs::read_to_string(&form).unwrap();
    // bash#13
    assert!(html.contains("method=\"post\""));
    // bash#14
    assert!(html.contains("document.getElementById('f').submit()"));
    // bash#15: round-trip
    let start =
        html.find("name=\"manifest\" value=\"").unwrap() + "name=\"manifest\" value=\"".len();
    let end = start + html[start..].find('"').unwrap();
    let decoded = html[start..end]
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&decoded).unwrap(),
        serde_json::from_str::<serde_json::Value>(MANIFEST).unwrap()
    );
    // bash#16: 引用符付きの action と 16 文字の state
    let marker = "action=\"https://github.com/settings/apps/new?state=";
    let s = html.find(marker).unwrap() + marker.len();
    let state = &html[s..s + 16];
    assert!(state.bytes().all(|b| b.is_ascii_alphanumeric()));
    assert_eq!(&html[s + 16..s + 17], "\"");
    assert!(o.stdout.contains(&format!("(state={state})")));
    assert!(html.ends_with("</html>\n"));
}

/// bash#17〜#21: convert は Authorization ヘッダを付けず(空ヘッダは 401)、PEM は
/// 0600 のファイルに置き、保管すべき secret 名(CLIENT_ID 含む、#615)を案内する。
#[test]
fn convert_sends_no_auth_and_writes_0600_pem() {
    let fx = Fx::new();
    let curl = fx.p("conv-curl");
    let args = fx.p("conv-args");
    write_exec(
        &curl,
        r#"#!/usr/bin/env bash
printf '%s\n' "$*" > "$CONV_ARGS"
printf '{"id":42,"slug":"tarotene-releaser","client_id":"Iv1.test","pem":"-----BEGIN TEST-----"}'
"#,
    );
    let o = out(bin()
        .args(["convert", "releaser", "testcode"])
        .env("CONV_ARGS", &args)
        .env("GITHUB_APP_SNAPSHOT_CURL_BIN", &curl)
        .env("XDG_RUNTIME_DIR", fx.tmp.path()));
    assert_eq!(o.code, 0, "{}", o.stderr);
    let a = fs::read_to_string(&args).unwrap();
    // bash#17
    assert!(!a.to_lowercase().contains("authorization"), "{a}");
    // bash#18
    assert!(a.contains("/app-manifests/testcode/conversions"), "{a}");
    // bash#19
    let pem = fx.p("github-app-snapshot-releaser.pem");
    assert_eq!(
        fs::metadata(&pem).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&pem).unwrap(), "-----BEGIN TEST-----\n");
    // bash#20
    assert!(o.stdout.contains("GITHUB_APP_RELEASER_ID"));
    // bash#21 (#615)
    assert!(o
        .stdout
        .contains("GITHUB_APP_RELEASER_CLIENT_ID = Iv1.test"));
    assert_eq!(
        o.stdout,
        format!(
            "github-app-snapshot: converted \"releaser\" — id=42 slug=tarotene-releaser client_id=Iv1.test\n\nStore these three secrets in the Bitwarden Secrets Manager \"github-apps\" project:\n  GITHUB_APP_RELEASER_ID        = 42\n  GITHUB_APP_RELEASER_PEM       = <contents of {p}>\n  GITHUB_APP_RELEASER_CLIENT_ID = Iv1.test\n\nThen discard the local PEM copy:\n  shred -u {p}\n",
            p = pem.display()
        )
    );
}

/// 変換に失敗したら(id が無い)終了コード 1 と本文つきのメッセージ。
#[test]
fn convert_failure_reports_body() {
    let fx = Fx::new();
    let curl = fx.p("bad-curl");
    write_exec(
        &curl,
        "#!/usr/bin/env bash\nprintf '{\"message\":\"Not Found\"}'\n",
    );
    let o = out(bin()
        .args(["convert", "releaser", "x"])
        .env("GITHUB_APP_SNAPSHOT_CURL_BIN", &curl)
        .env("XDG_RUNTIME_DIR", fx.tmp.path()));
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        "github-app-snapshot: manifest conversion failed: {\"message\":\"Not Found\"}\n"
    );
}

/// bash#22: `bws run` は結合した引数を shell に再パースするので、引数のクォートが
/// 壊れてはならず、注入された secret が子プロセスに届く。
#[test]
fn exec_preserves_quoting_through_bws_run() {
    let fx = Fx::new();
    let st = fx.p("q-secret-tool");
    let bws = fx.p("q-bws");
    write_exec(&st, "#!/usr/bin/env bash\nprintf 'tok'\n");
    write_exec(
        &bws,
        r#"#!/usr/bin/env bash
# usage: bws run --no-inherit-env -- <args...>  (joins args like the real one)
shift 3
export GITHUB_APP_RELEASER_ID=777
exec sh -c "$*"
"#,
    );
    let o = out(bin()
        .args([
            "exec",
            "--",
            "sh",
            "-c",
            "printf \"[%s|%s]\" \"$1\" \"$GITHUB_APP_RELEASER_ID\"",
            "_",
            "a b'c\nd",
        ])
        .env("GITHUB_APP_SNAPSHOT_BWS_BIN", &bws)
        .env("GITHUB_APP_SNAPSHOT_SECRET_TOOL_BIN", &st));
    assert_eq!(o.stdout, "[a b'c\nd|777]", "{}", o.stderr);
    assert_eq!(o.code, 0);
}

/// bash#22 の補足: BWS_ACCESS_TOKEN は keyring の値が env で渡り、argv に出ない。
#[test]
fn token_goes_via_env_not_argv() {
    let fx = Fx::new();
    let st = fx.p("st");
    let bws = fx.p("bws");
    write_exec(&st, "#!/usr/bin/env bash\nprintf 'sekrit-token\\n\\n'\n");
    write_exec(
        &bws,
        "#!/usr/bin/env bash\nprintf 'env=%s\\nargv=%s\\n' \"$BWS_ACCESS_TOKEN\" \"$*\"\nexit 7\n",
    );
    let o = out(bin()
        .args(["run", "--out", "x"])
        .env("GITHUB_APP_SNAPSHOT_BWS_BIN", &bws)
        .env("GITHUB_APP_SNAPSHOT_SECRET_TOOL_BIN", &st));
    // bws の終了コードがそのまま返る
    assert_eq!(o.code, 7);
    assert!(o
        .stdout
        .starts_with("env=sekrit-token\nargv=run --no-inherit-env -- "));
    let argv_line = o.stdout.lines().nth(1).unwrap();
    assert!(!argv_line.contains("sekrit-token"));
    assert!(
        argv_line.contains("'__run' 'snapshot' '--out' 'x' "),
        "{argv_line}"
    );
}

/// bash#23: keyring に token が無ければ、案内つきで終了コード 1。ネットワークには出ない。
#[test]
fn missing_keyring_token_fails_with_guidance() {
    let o = out(bin()
        .arg("run")
        .env("GITHUB_APP_SNAPSHOT_SECRET_TOOL_BIN", "false")
        .env("GITHUB_APP_SNAPSHOT_BWS_BIN", "false"));
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        "github-app-snapshot: Bitwarden machine token is missing from the login keyring\nrun: github-app-snapshot configure-token\n"
    );
}

/// 引数処理(selftest の外。bash 版 main の挙動)。
#[test]
fn usage_errors_exit_2() {
    for args in [
        vec![],
        vec!["bogus"],
        vec!["convert", "only-one"],
        vec!["manifest-form"],
        vec!["exec"],
        vec!["exec", "--"],
        vec!["exec", "x"],
        vec!["__run"],
    ] {
        let o = out(bin().args(&args));
        assert_eq!(o.code, 2, "{args:?}");
        assert!(o
            .stderr
            .starts_with("usage:\n  github-app-snapshot configure-token\n"));
    }
    let o = out(bin().args(["__run", "nope"]));
    assert_eq!(o.code, 2);
    assert_eq!(
        o.stderr,
        "github-app-snapshot: unknown inner operation: nope\n"
    );
}

#[test]
fn manifest_form_without_manifest_fails() {
    let fx = Fx::new();
    let o = out(bin()
        .args(["manifest-form", "nope"])
        .env("GITHUB_APP_SNAPSHOT_MANIFESTS_DIR", fx.p("manifests")));
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        format!(
            "github-app-snapshot: no manifest at {}\n",
            fx.p("manifests/nope.json").display()
        )
    );
}
