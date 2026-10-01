//! Manifest flow(ADR-590 D2: registration の正本は
//! `config/github-app-manifests/*.json`)。

use crate::api::{http_post_unauthenticated, raw};
use crate::config::{Config, GITHUB_API};
use github_audit::jq::{j_get, parse_ordered};
use hook_io::jqfmt::J;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// jq の `@html`(`<>&'"` を実体参照に)。
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// `head -c16 /dev/urandom | base64 | tr -dc a-zA-Z0-9 | head -c16`。bash 版は
/// base64 の `+/=` を捨てるので稀に 16 文字に満たなかった(selftest が 16 文字を
/// 要求する)。ここでは英数字だけを必ず 16 文字集める(意図的な差)。
fn random_state() -> String {
    const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::new();
    let mut urandom = std::fs::File::open("/dev/urandom").ok();
    let mut buf = [0u8; 64];
    while out.len() < 16 {
        let ok = urandom
            .as_mut()
            .map(|f| f.read_exact(&mut buf).is_ok())
            .unwrap_or(false);
        if !ok {
            // /dev/urandom が読めない環境では pid と時刻で代用(state は CSRF
            // 対策の使い捨て値で、秘密ではない)。
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            return format!("{:016x}", t ^ u128::from(std::process::id()))[..16].to_string();
        }
        for b in buf {
            // 248 = 62 * 4。剰余の偏りを避ける。
            if b < 248 {
                out.push(ALNUM[(b % 62) as usize] as char);
                if out.len() == 16 {
                    break;
                }
            }
        }
    }
    out
}

/// `manifest-form <name> [--out FILE]`
pub fn manifest_form(cfg: &Config, name: &str, rest: &[String]) -> Result<(), String> {
    let out = if rest.first().map(String::as_str) == Some("--out") {
        match rest.get(1) {
            Some(p) => Some(PathBuf::from(p)),
            // bash 版は `$2` が無いと set -u の unbound variable で終了コード 1。
            None => return Err("--out: missing value".to_string()),
        }
    } else {
        None
    };
    let manifest_file = cfg.manifests_dir.join(format!("{name}.json"));
    if !manifest_file.is_file() {
        return Err(format!("no manifest at {}", manifest_file.display()));
    }
    let bytes =
        std::fs::read(&manifest_file).map_err(|e| format!("{}: {e}", manifest_file.display()))?;
    // `$(cat file)`: 末尾の改行は落ちる。不正な UTF-8 は jq の --arg と同じく U+FFFD。
    let manifest_json = String::from_utf8_lossy(&bytes);
    let manifest_json = manifest_json.trim_end_matches('\n');
    let state = random_state();
    let out = match out {
        Some(p) => p,
        None => {
            let (_, path) = tempfile::Builder::new()
                .prefix("tmp.")
                .suffix(".html")
                .rand_bytes(10)
                .tempfile()
                .map_err(|e| format!("mktemp: {e}"))?
                .keep()
                .map_err(|e| format!("mktemp: {e}"))?;
            path
        }
    };
    // 値の属性は二重引用符で囲む(manifest JSON は空白を含み、引用符無しの
    // 属性値は最初の空白で終わる)。script 内の要素 id も引用符で囲む
    // (囲まないと `getElementById(f)` の ReferenceError で auto-submit が
    // 黙って止まる — 実際に起きた)。
    let html = format!(
        "<!doctype html><html><body><form id=\"f\" action=\"https://github.com/settings/apps/new?state={state}\" method=\"post\">\
         <input type=\"hidden\" name=\"manifest\" value=\"{}\"></form>\
         <script>document.getElementById('f').submit()</script></body></html>\n",
        html_escape(manifest_json)
    );
    std::fs::write(&out, html).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "github-app-snapshot: wrote {} (state={state}) — open it in a browser, click \"Create GitHub App\", then copy the `code` query parameter from the redirect URL",
        out.display()
    );
    Ok(())
}

/// `convert <name> <code>`: manifest の code を App 登録に交換する。
pub fn convert_manifest(cfg: &Config, name: &str, code: &str) -> Result<(), String> {
    // 認証不要のエンドポイント(GitHub Docs, "Registering a GitHub App from a
    // manifest"、取得 2026-09-29): Authorization ヘッダは付けない。
    let body_text = http_post_unauthenticated(
        cfg,
        &format!("{GITHUB_API}/app-manifests/{code}/conversions"),
    );
    let body = parse_ordered(&body_text).unwrap_or(J::Null);
    let field = |k: &str| match j_get(&body, k) {
        None | Some(J::Null) | Some(J::Bool(false)) => String::new(),
        Some(v) => raw(v),
    };
    let (id, slug, client_id) = (field("id"), field("slug"), field("client_id"));
    if id.is_empty() {
        return Err(format!("manifest conversion failed: {body_text}"));
    }
    let upper = crate::api::name_upper(name);
    let runtime = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(v) if !v.is_empty() => v,
        _ => "/tmp".to_string(),
    };
    let pem_file = format!("{runtime}/github-app-snapshot-{name}.pem");
    // umask 077 相当: 新規作成は 0600(既存ファイルのモードは変えない)。
    let pem = match j_get(&body, "pem") {
        Some(J::Str(s)) => s.clone(),
        Some(other) => other.compact(),
        None => "null".to_string(),
    };
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&pem_file)
            .map_err(|e| format!("{pem_file}: {e}"))?;
        writeln!(f, "{pem}").map_err(|e| format!("{pem_file}: {e}"))?;
    }
    print!(
        "github-app-snapshot: converted \"{name}\" — id={id} slug={slug} client_id={client_id}\n\
         \n\
         Store these three secrets in the Bitwarden Secrets Manager \"github-apps\" project:\n  \
         GITHUB_APP_{upper}_ID        = {id}\n  \
         GITHUB_APP_{upper}_PEM       = <contents of {pem_file}>\n  \
         GITHUB_APP_{upper}_CLIENT_ID = {client_id}\n\
         \n\
         Then discard the local PEM copy:\n  \
         shred -u {pem_file}\n"
    );
    Ok(())
}
