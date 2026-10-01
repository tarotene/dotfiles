//! github-app-registry-check — 所有 App の Manifest 宣言
//! (`config/github-app-manifests/<name>.json`、ADR-590 D2)と、
//! `github-app-snapshot`(ADR-590 D3)が最後に観測した実体の登録との
//! account レベルの drift 検査(ADR-436 Amendment 2026-09-30)。
//! 決定論的で LLM も secret も使わない — snapshot ファイルを読むだけ。
//!
//! github-audit のドメインにしないのは意図的: あちらのデータモデルは
//! リポジトリ単位(repo x domain -> verdict)で、account レベルの App 登録には
//! 紐づくリポジトリが無い。github-audit の関数は使っていない(bash 版も
//! source していなかった)ので、このクレートは github-audit に依存しない。
//!
//! bash 版(scripts/github-app-registry-check)からの移植(#414、ADR-0024
//! Stage 4e)。出力・終了コードはバイト単位で一致させる。jq を使わなくなったため
//! `GITHUB_APP_REGISTRY_CHECK_JQ_BIN` は無くなった。

use hook_io::jqfmt::J;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str =
    "usage: github-app-registry-check [--manifests-dir DIR] [--snapshot FILE] [--json]
       github-app-registry-check --selftest
";

const SELF: &str = "github-app-registry-check";

struct Finding {
    app: String,
    drifted: bool,
    missing: Vec<&'static str>,
}

impl Finding {
    fn to_j(&self) -> J {
        J::obj(vec![
            ("app", J::str(self.app.clone())),
            (
                "verdict",
                J::str(if self.drifted { "drifted" } else { "ok" }),
            ),
            (
                "missing",
                J::Arr(self.missing.iter().map(|m| J::str(*m)).collect()),
            ),
        ])
    }
}

/// jq の `a // b`(a が null / false なら b)。
fn alt(v: Option<&Value>, default: Value) -> Value {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => default,
        Some(v) => v.clone(),
    }
}

/// `jq -r '.name'` を `$(...)` で受けた値(文字列はそのまま、null は "null"、
/// それ以外は JSON 表記。末尾改行は落ちる)。読めない・壊れた JSON は ""
/// (bash 版は `$(...)` の中の jq 失敗を errexit が拾わず空文字になる)。
fn manifest_name(manifest: Option<&Value>) -> String {
    let s = match manifest.map(|m| m.get("name")) {
        Some(Some(Value::String(s))) => s.clone(),
        Some(Some(other)) => J::from(other).compact(),
        Some(None) => "null".to_string(),
        None => String::new(),
    };
    s.trim_end_matches('\n').to_string()
}

/// `($manifest.default_events // []) | sort` と `($live.events // []) | sort`
/// の比較。同じ全順序で並べれば等しさは並び順に依らないので、順序は
/// compact 表記のバイト順で足りる。
fn sorted_events(v: Value) -> Value {
    match v {
        Value::Array(mut a) => {
            a.sort_by_key(|x| J::from(x).compact());
            Value::Array(a)
        }
        other => other,
    }
}

fn judge_app(manifest: Option<&Value>, snapshot_apps: &[Value]) -> Finding {
    let name = manifest_name(manifest);
    let live = snapshot_apps
        .iter()
        .find(|a| a.get("app_name").and_then(Value::as_str) == Some(name.as_str()));
    let Some(live) = live else {
        return Finding {
            app: name,
            drifted: true,
            missing: vec!["app-registry-missing-in-snapshot"],
        };
    };
    let null = Value::Null;
    let manifest = manifest.unwrap_or(&null);
    let mut missing = Vec::new();
    let empty_obj = || Value::Object(serde_json::Map::new());
    if alt(manifest.get("default_permissions"), empty_obj())
        != alt(live.get("permissions"), empty_obj())
    {
        missing.push("app-registry-permissions-drift");
    }
    if sorted_events(alt(manifest.get("default_events"), Value::Array(vec![])))
        != sorted_events(alt(live.get("events"), Value::Array(vec![])))
    {
        missing.push("app-registry-events-drift");
    }
    Finding {
        app: name,
        drifted: !missing.is_empty(),
        missing,
    }
}

/// bash の glob `"$dir"/*.json`(ロケールの照合順ではなくバイト順で並べる)。
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

fn run_check(manifests_dir: &Path, snapshot_file: &Path) -> Vec<Finding> {
    let snapshot_exists = snapshot_file.is_file();
    let snapshot_apps: Vec<Value> = if snapshot_exists {
        std::fs::read(snapshot_file)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .map(|v| match alt(v.get("apps"), Value::Array(vec![])) {
                Value::Array(a) => a,
                _ => Vec::new(),
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if !manifests_dir.is_dir() {
        return Vec::new();
    }
    manifest_files(manifests_dir)
        .iter()
        .map(|file| {
            let manifest = std::fs::read(file)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            if !snapshot_exists {
                Finding {
                    app: manifest_name(manifest.as_ref()),
                    drifted: true,
                    missing: vec!["app-snapshot-missing"],
                }
            } else {
                // 壊れた manifest は名前が "" になり snapshot に無い扱いになる
                // (bash 版と同じ縮退)。
                judge_app(manifest.as_ref(), &snapshot_apps)
            }
        })
        .collect()
}

fn render_report(findings: &[Finding]) -> String {
    let mut out = String::new();
    for f in findings {
        out.push_str(&format!(
            "app={} verdict={}",
            f.app,
            if f.drifted { "drifted" } else { "ok" }
        ));
        if !f.missing.is_empty() {
            out.push_str(" missing=");
            out.push_str(&f.missing.join(","));
        }
        out.push('\n');
    }
    let ok = findings.iter().filter(|f| !f.drifted).count();
    let drifted = findings.len() - ok;
    out.push_str(&format!(
        "total: {} app(s), ok={ok} drifted={drifted}\n",
        findings.len()
    ));
    out
}

fn default_dir(xdg_var: &str, home_rel: &str, tail: &str) -> PathBuf {
    let base = match std::env::var(xdg_var) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(home_rel),
    };
    base.join(tail)
}

fn main() -> ExitCode {
    let mut manifests_dir = match std::env::var("GITHUB_APP_REGISTRY_CHECK_MANIFESTS_DIR") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => default_dir(
            "XDG_CONFIG_HOME",
            ".config",
            "github-app-snapshot/manifests",
        ),
    };
    let mut snapshot_file = match std::env::var("GITHUB_APP_REGISTRY_CHECK_SNAPSHOT_FILE") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => default_dir(
            "XDG_STATE_HOME",
            ".local/state",
            "github-audit/app-snapshot.json",
        ),
    };
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--manifests-dir" | "--snapshot" => {
                // bash 版は `$2` が無いと `set -u` の unbound variable で終了コード 1。
                let Some(v) = args.next() else {
                    eprintln!("{SELF}: {a}: missing value");
                    return ExitCode::from(1);
                };
                if a == "--manifests-dir" {
                    manifests_dir = PathBuf::from(v);
                } else {
                    snapshot_file = PathBuf::from(v);
                }
            }
            "--json" => json = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let findings = run_check(&manifests_dir, &snapshot_file);
    if json {
        println!(
            "{}",
            J::Arr(findings.iter().map(Finding::to_j).collect()).compact()
        );
    } else {
        print!("{}", render_report(&findings));
    }
    // any_drift: drift が 1 件でもあれば 1。
    if findings.iter().any(|f| f.drifted) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
