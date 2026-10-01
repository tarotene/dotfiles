//! owned App の登録 + install 先の snapshot(ADR-590 D2/D3)。
//!
//! HTTP は curl コマンドに閉じる(bash 版と同じ。テストは `*_CURL_BIN` で
//! スタブに差し替える)。秘密(PEM・JWT・installation token)はログにも
//! snapshot にも出さない — snapshot に載るのは permissions/events/repo 名だけ。

use crate::config::{Config, GITHUB_API};
use crate::jwt::build_jwt;
use github_audit::jq::{j_get, parse_ordered};
use hook_io::jqfmt::J;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `curl ... 2>/dev/null || printf '{}'`: 成功時は本文、失敗時は curl の stdout に
/// `{}` を足したもの。
fn curl(cfg: &Config, args: &[&str]) -> String {
    let out = Command::new(&cfg.curl_bin)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            if !o.status.success() {
                s.push_str("{}");
            }
            s
        }
        Err(_) => "{}".to_string(),
    }
}

const ACCEPT: &str = "Accept: application/vnd.github+json";

fn http_get(cfg: &Config, url: &str, bearer: &str) -> String {
    let auth = format!("Authorization: Bearer {bearer}");
    curl(cfg, &["-sS", "-H", &auth, "-H", ACCEPT, url])
}

fn http_post_empty_body(cfg: &Config, url: &str, bearer: &str) -> String {
    let auth = format!("Authorization: Bearer {bearer}");
    curl(cfg, &["-sS", "-X", "POST", "-H", &auth, "-H", ACCEPT, url])
}

/// POST /app-manifests/{code}/conversions は資格情報を取らず、GitHub は空の
/// `Authorization: Bearer ` ヘッダを 401 Bad credentials で弾く — ヘッダは空で
/// なく無くす必要がある。
pub fn http_post_unauthenticated(cfg: &Config, url: &str) -> String {
    curl(cfg, &["-sS", "-X", "POST", "-H", ACCEPT, url])
}

/// jq の `a // b`(null / false / 無しなら b)。
fn alt(v: Option<&J>, default: J) -> J {
    match v {
        None | Some(J::Null) | Some(J::Bool(false)) => default,
        Some(v) => v.clone(),
    }
}

/// `jq -r` で 1 値を出し `$(...)` で受けた文字列(文字列はそのまま、他は
/// JSON 表記。末尾の改行は落ちる)。
pub fn raw(v: &J) -> String {
    let s = match v {
        J::Str(s) => s.clone(),
        other => other.compact(),
    };
    s.trim_end_matches('\n').to_string()
}

/// 本文を JSON として読む。壊れていれば Null(bash 版は jq が失敗して
/// 空になるだけの縮退)。
fn parse_body(body: &str) -> J {
    parse_ordered(body).unwrap_or(J::Null)
}

/// `{slug, app_name, permissions, events}`
struct Registration {
    slug: J,
    app_name: J,
    permissions: J,
    events: J,
}

fn fetch_app_registration(cfg: &Config, id: &str, pem_file: &Path) -> Result<Registration, String> {
    let jwt = build_jwt(cfg, id, pem_file)?;
    let body = parse_body(&http_get(cfg, &format!("{GITHUB_API}/app"), &jwt));
    Ok(Registration {
        slug: alt(j_get(&body, "slug"), J::str("")),
        app_name: alt(j_get(&body, "name"), J::str("")),
        permissions: alt(j_get(&body, "permissions"), J::Obj(vec![])),
        events: alt(j_get(&body, "events"), J::Arr(vec![])),
    })
}

/// installation の repo 一覧("owner/repo" の配列)。token が取れなければ空。
fn fetch_installation_repos(cfg: &Config, installation_id: &str, jwt: &str) -> J {
    let token_body = parse_body(&http_post_empty_body(
        cfg,
        &format!("{GITHUB_API}/app/installations/{installation_id}/access_tokens"),
        jwt,
    ));
    let token = match j_get(&token_body, "token") {
        None | Some(J::Null) | Some(J::Bool(false)) => String::new(),
        Some(v) => raw(v),
    };
    if token.is_empty() {
        return J::Arr(vec![]);
    }
    let mut repos = Vec::new();
    let mut page = 1;
    loop {
        let body = parse_body(&http_get(
            cfg,
            &format!("{GITHUB_API}/installation/repositories?per_page=100&page={page}"),
            &token,
        ));
        let batch: Vec<J> = match j_get(&body, "repositories") {
            Some(J::Arr(items)) => items
                .iter()
                .filter(|r| matches!(r, J::Obj(_)))
                .map(|r| j_get(r, "full_name").cloned().unwrap_or(J::Null))
                .collect(),
            _ => Vec::new(),
        };
        let n = batch.len();
        repos.extend(batch);
        if n < 100 {
            break;
        }
        page += 1;
    }
    J::Arr(repos)
}

/// `[{id, account, repository_selection, repositories}]`。応答が配列でない
/// (API エラーのオブジェクトなど)ときは空 — bash 版は jq の失敗が連鎖して
/// 壊れた JSON を書き出していた(意図的に変えた箇所)。
fn fetch_app_installations(cfg: &Config, id: &str, pem_file: &Path) -> Result<J, String> {
    let jwt = build_jwt(cfg, id, pem_file)?;
    let body = parse_body(&http_get(
        cfg,
        &format!("{GITHUB_API}/app/installations"),
        &jwt,
    ));
    let J::Arr(items) = body else {
        return Ok(J::Arr(vec![]));
    };
    let mut out = Vec::new();
    for inst in items {
        let Some(J::Num(inst_id)) = j_get(&inst, "id").cloned() else {
            continue;
        };
        let account = j_get(&inst, "account")
            .and_then(|a| j_get(a, "login"))
            .map(|v| alt(Some(v), J::str("")))
            .unwrap_or_else(|| J::str(""));
        let selection = alt(j_get(&inst, "repository_selection"), J::str(""));
        let repos = fetch_installation_repos(cfg, &inst_id, &jwt);
        out.push(J::obj(vec![
            ("id", J::Num(inst_id)),
            ("account", J::str(raw(&account))),
            ("repository_selection", J::str(raw(&selection))),
            ("repositories", repos),
        ]));
    }
    Ok(J::Arr(out))
}

/// bash の glob `"$dir"/*.json`(バイト順。ドットファイルは含まない)。
fn manifest_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && p.file_name()
                    .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
                && p.is_file()
        })
        .collect();
    files.sort();
    files
}

/// `tr '[:lower:]-' '[:upper:]_'`
pub fn name_upper(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c == '-' {
                '_'
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect()
}

/// 0600 の一時ファイルに PEM を置く。drop 時に中身を潰してから unlink する
/// (bash 版の `openssl rand -out "$pem_file" 1` + `rm -f`、best-effort)。
struct PemFile(tempfile::NamedTempFile);

impl PemFile {
    fn new(pem: &str) -> std::io::Result<PemFile> {
        let mut f = tempfile::NamedTempFile::new()?; // mode 0600
        f.write_all(pem.as_bytes())?;
        f.flush()?;
        Ok(PemFile(f))
    }
}

impl Drop for PemFile {
    fn drop(&mut self) {
        let len = self.0.as_file().metadata().map(|m| m.len()).unwrap_or(0) as usize;
        let _ = self.0.as_file_mut().set_len(0);
        let _ = self.0.as_file_mut().write_all(&vec![0u8; len]);
    }
}

/// `{name, id, slug, app_name, permissions, events, installations}` の配列。
/// 環境に `GITHUB_APP_<NAME>_ID/_PEM` が無い App は警告して飛ばす。
pub fn snapshot_apps(cfg: &Config) -> Result<J, String> {
    let mut out = Vec::new();
    if !cfg.manifests_dir.is_dir() {
        return Ok(J::Arr(out));
    }
    for manifest in manifest_files(&cfg.manifests_dir) {
        let name = manifest
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let upper = name_upper(&name);
        let id = std::env::var(format!("GITHUB_APP_{upper}_ID")).unwrap_or_default();
        let pem = std::env::var(format!("GITHUB_APP_{upper}_PEM")).unwrap_or_default();
        if id.is_empty() || pem.is_empty() {
            eprintln!(
                "github-app-snapshot: {name} has no GITHUB_APP_{upper}_ID/_PEM in the environment — skipping (store them in the github-apps Secrets Manager project)"
            );
            continue;
        }
        // jq の `$id | tonumber`。数でなければ bash 版は壊れた出力を書いていた。
        let id_num: i64 = id
            .trim()
            .parse()
            .map_err(|_| format!("GITHUB_APP_{upper}_ID is not a number"))?;
        let pem_file = PemFile::new(&pem).map_err(|e| format!("cannot stage the PEM: {e}"))?;
        let reg = fetch_app_registration(cfg, &id, pem_file.0.path());
        let installs = fetch_app_installations(cfg, &id, pem_file.0.path());
        drop(pem_file);
        let (reg, installs) = (reg?, installs?);
        out.push(J::obj(vec![
            ("name", J::str(name)),
            ("id", J::Num(id_num.to_string())),
            ("slug", reg.slug),
            ("app_name", reg.app_name),
            ("permissions", reg.permissions),
            ("events", reg.events),
            ("installations", installs),
        ]));
    }
    Ok(J::Arr(out))
}

/// `date --iso-8601=seconds`(ローカル時刻 + オフセット。bash 版と同じ書式に
/// するため date に任せる)。
fn iso_now() -> String {
    Command::new("date")
        .arg("--iso-8601=seconds")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default()
}

/// snapshot を `out_path` に原子的に書く(同じディレクトリの 0600 一時ファイル
/// → rename。bash 版の `mktemp` + `mv`)。
pub fn do_snapshot(cfg: &Config, out_path: &Path) -> Result<(), String> {
    let apps = snapshot_apps(cfg)?;
    let dir = out_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let doc = J::obj(vec![
        ("schema", J::Num("1".into())),
        ("generated_at", J::str(iso_now())),
        ("apps", apps),
    ]);
    let mut tmp = tempfile::Builder::new()
        .prefix("app-snapshot.json.")
        .rand_bytes(6)
        .tempfile_in(dir)
        .map_err(|e| format!("mktemp in {}: {e}", dir.display()))?;
    writeln!(tmp, "{}", doc.pretty()).map_err(|e| format!("write: {e}"))?;
    tmp.persist(out_path)
        .map_err(|e| format!("mv to {}: {}", out_path.display(), e.error))?;
    println!("github-app-snapshot: wrote {}", out_path.display());
    Ok(())
}
