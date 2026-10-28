//! owned App の登録 + install 先の snapshot(ADR-590 D2/D3)。
//!
//! HTTP は curl コマンドに閉じる(bash 版と同じ。テストは `*_CURL_BIN` で
//! スタブに差し替える)。秘密(PEM・JWT・installation token)はログにも
//! snapshot にも出さない — snapshot に載るのは permissions/events/repo 名だけ。

use crate::config::{Config, GITHUB_API};
use crate::jwt::build_jwt;
use github_audit::jq::{j_get, parse_ordered};
use jq_json_compat::jqfmt::J;
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

// Keep this third-party user token out of argv and diagnostics. The token is
// passed through stdin as curl configuration, never written to the snapshot.
fn user_installation_page(cfg: &Config, id: &str, token: &str, page: usize) -> Result<J, ()> {
    if token.is_empty()
        || token
            .bytes()
            .any(|b| b.is_ascii_control() || b == b'"' || b == b'\\')
    {
        return Err(());
    }
    let url = format!("{GITHUB_API}/user/installations/{id}/repositories?per_page=100&page={page}");
    let mut child = Command::new(&cfg.curl_bin)
        .args(["--silent", "--show-error", "--fail", "--config", "-", &url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let mut stdin = child.stdin.take().ok_or(())?;
    write!(
        stdin,
        "header = \"Authorization: Bearer {token}\"\nheader = \"{ACCEPT}\"\n"
    )
    .map_err(|_| ())?;
    drop(stdin);
    let out = child.wait_with_output().map_err(|_| ())?;
    if !out.status.success() {
        return Err(());
    }
    parse_ordered(std::str::from_utf8(&out.stdout).map_err(|_| ())?).ok_or(())
}

fn third_party_snapshot(cfg: &Config) -> J {
    let id = match std::fs::read_to_string(&cfg.claude_installation_id_file) {
        Ok(id) => id.trim().to_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => {
            return J::obj(vec![
                ("status", J::str("error")),
                ("repositories", J::Arr(vec![])),
            ])
        }
    };
    let token = std::env::var("GITHUB_PAT_READ_USER").unwrap_or_default();
    let result = if id.is_empty() && token.is_empty() {
        ("unobserved", Vec::new())
    } else if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) || token.is_empty() {
        ("error", Vec::new())
    } else {
        match fetch_user_installation(cfg, &id, &token) {
            Ok(repos) => ("observed", repos),
            Err(()) => ("error", Vec::new()),
        }
    };
    J::obj(vec![
        ("status", J::str(result.0)),
        ("repositories", J::Arr(result.1)),
    ])
}

fn fetch_user_installation(cfg: &Config, id: &str, token: &str) -> Result<Vec<J>, ()> {
    let mut repos = Vec::new();
    let mut total = None;
    for page in 1..=10000 {
        let body = user_installation_page(cfg, id, token, page)?;
        let count = match j_get(&body, "total_count") {
            Some(J::Num(n)) => n.parse::<usize>().map_err(|_| ())?,
            _ => return Err(()),
        };
        if total.is_some_and(|old| old != count) {
            return Err(());
        }
        total = Some(count);
        let Some(J::Arr(batch)) = j_get(&body, "repositories") else {
            return Err(());
        };
        if batch.len() > 100 {
            return Err(());
        }
        for repo in batch {
            let Some(J::Str(name)) = j_get(repo, "full_name") else {
                return Err(());
            };
            if name.is_empty() || repos.iter().any(|r| matches!(r, J::Str(s) if s == name)) {
                return Err(());
            }
            repos.push(J::str(name));
        }
        if repos.len() > count {
            return Err(());
        }
        if batch.len() < 100 {
            return if repos.len() == count {
                Ok(repos)
            } else {
                Err(())
            };
        }
    }
    Err(())
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
    if !matches!(j_get(&body, "slug"), Some(J::Str(s)) if !s.is_empty())
        || !matches!(j_get(&body, "name"), Some(J::Str(s)) if !s.is_empty())
    {
        return Err("app registration unavailable".into());
    }
    Ok(Registration {
        slug: alt(j_get(&body, "slug"), J::str("")),
        app_name: alt(j_get(&body, "name"), J::str("")),
        permissions: alt(j_get(&body, "permissions"), J::Obj(vec![])),
        events: alt(j_get(&body, "events"), J::Arr(vec![])),
    })
}

/// Read all pages; a failed or partial response is never an empty installation.
fn fetch_installation_repos(cfg: &Config, installation_id: &str, jwt: &str) -> Result<J, String> {
    let body = parse_body(&http_post_empty_body(
        cfg,
        &format!("{GITHUB_API}/app/installations/{installation_id}/access_tokens"),
        jwt,
    ));
    let token = match j_get(&body, "token") {
        Some(J::Str(s)) if !s.is_empty() => s,
        _ => return Err("installation token unavailable".into()),
    };
    let mut repos = Vec::new();
    let mut total = None;
    for page in 1..=10000 {
        let body = parse_body(&http_get(
            cfg,
            &format!("{GITHUB_API}/installation/repositories?per_page=100&page={page}"),
            token,
        ));
        let count = match j_get(&body, "total_count") {
            Some(J::Num(n)) => n.parse::<usize>().map_err(|_| "invalid repository count")?,
            _ => return Err("repository listing unavailable".into()),
        };
        if total.is_some_and(|previous| previous != count) {
            return Err("repository listing changed during pagination".into());
        }
        total = Some(count);
        let Some(J::Arr(batch)) = j_get(&body, "repositories") else {
            return Err("repository listing unavailable".into());
        };
        if batch.len() > 100 {
            return Err("invalid repository page".into());
        }
        for entry in batch {
            let Some(J::Str(name)) = j_get(entry, "full_name") else {
                return Err("invalid repository entry".into());
            };
            if name.is_empty() || repos.iter().any(|r| matches!(r, J::Str(s) if s == name)) {
                return Err("duplicate or empty repository".into());
            }
            repos.push(J::str(name));
        }
        if repos.len() > count {
            return Err("invalid repository count".into());
        }
        if batch.len() < 100 {
            return if repos.len() == count {
                Ok(J::Arr(repos))
            } else {
                Err("incomplete repository listing".into())
            };
        }
    }
    Err("repository page limit exceeded".into())
}

/// All installation pages and all their repository pages, or an error.
fn fetch_app_installations(cfg: &Config, id: &str, pem_file: &Path) -> Result<J, String> {
    let jwt = build_jwt(cfg, id, pem_file)?;
    let mut out = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for page in 1..=10000 {
        let body = parse_body(&http_get(
            cfg,
            &format!("{GITHUB_API}/app/installations?per_page=100&page={page}"),
            &jwt,
        ));
        let J::Arr(items) = body else {
            return Err("installation listing unavailable".into());
        };
        let n = items.len();
        if n > 100 {
            return Err("invalid installation page".into());
        }
        for inst in items {
            let Some(J::Num(inst_id)) = j_get(&inst, "id").cloned() else {
                return Err("invalid installation id".into());
            };
            if !ids.insert(inst_id.clone()) {
                return Err("duplicate installation".into());
            }
            let Some(J::Str(account)) = j_get(&inst, "account").and_then(|a| j_get(a, "login"))
            else {
                return Err("invalid installation account".into());
            };
            let Some(J::Str(selection)) = j_get(&inst, "repository_selection") else {
                return Err("invalid installation selection".into());
            };
            let repos = fetch_installation_repos(cfg, &inst_id, &jwt)?;
            out.push(J::obj(vec![
                ("id", J::Num(inst_id)),
                ("account", J::str(account)),
                ("repository_selection", J::str(selection)),
                ("repositories", repos),
            ]));
        }
        if n < 100 {
            return Ok(J::Arr(out));
        }
    }
    Err("installation page limit exceeded".into())
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
            ("complete", J::Bool(true)),
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
        ("claude_installation", third_party_snapshot(cfg)),
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
