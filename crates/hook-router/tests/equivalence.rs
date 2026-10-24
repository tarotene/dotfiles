//! ルータは、旧 bin と同じ理由で拒否する。
//!
//! 旧構成では、検査ごとに別の hook を Claude Code が並列に走らせていた。ルータへ移すときの
//! 新しいリスクは、規則の表の `Tools`(どのツールに当てるか)にある。判定そのものは、
//! `main` とルータが同じ `hook` 関数を呼ぶので、同じになる。ここでは、旧 bin の trycmd の
//! fixture(標準入力と、期待する標準出力)のうち、Claude 形(引数なし)で拒否を期待するものを
//! ルータにも流して、同じ理由が出ることを確かめる。MCP の経路(external-send-guard)と、
//! Bash の経路(attribution-guard)の両方を覆う。

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_hook-router");

struct Case {
    file: PathBuf,
    stdin: String,
    expected_reason: String,
    env_add: BTreeMap<String, String>,
    env_remove: Vec<String>,
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Claude 形(引数なし、または `--agent claude` だけ)で、拒否を期待する fixture。
fn deny_cases(crate_dir: &str) -> Vec<Case> {
    let dir = crates_dir().join(crate_dir).join("tests/cmd");
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    for file in files {
        let v: toml::Value = toml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let claude_shaped = match v.get("args").and_then(|a| a.as_array()) {
            None => true,
            Some(a) => {
                a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>() == ["--agent", "claude"]
            }
        };
        if !claude_shaped {
            continue;
        }
        let Some(stdout) = v.get("stdout").and_then(|s| s.as_str()) else {
            continue;
        };
        let Ok(expected) = serde_json::from_str::<serde_json::Value>(stdout) else {
            continue;
        };
        let spec = &expected["hookSpecificOutput"];
        if spec["permissionDecision"] != "deny" {
            continue;
        }
        let env = v.get("env");
        out.push(Case {
            file: file.clone(),
            stdin: v["stdin"].as_str().unwrap().to_string(),
            expected_reason: spec["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .to_string(),
            env_add: env
                .and_then(|e| e.get("add"))
                .and_then(|a| a.as_table())
                .map(|t| {
                    t.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            env_remove: env
                .and_then(|e| e.get("remove"))
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    out
}

fn router_reason(case: &Case) -> Option<String> {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(BIN);
    cmd.arg("pre")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"));
    for (k, v) in &case.env_add {
        cmd.env(k, v);
    }
    for k in &case.env_remove {
        cmd.env_remove(k);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(case.stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", case.file.display());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .map(String::from)
}

fn check_all(crate_dir: &str, at_least: usize) {
    let cases = deny_cases(crate_dir);
    assert!(
        cases.len() >= at_least,
        "{crate_dir}: 拒否を期待する fixture が {} 件(下限 {at_least})",
        cases.len()
    );
    for case in &cases {
        let reason = router_reason(case).unwrap_or_else(|| {
            panic!(
                "{}: 旧 bin は拒否するが、ルータは何も出さない(規則の表の Tools を確かめる)",
                case.file.display()
            )
        });
        assert!(
            reason.contains(&case.expected_reason),
            "{}: ルータの理由に、旧 bin の理由が含まれない\n  旧: {}\n  新: {reason}",
            case.file.display(),
            case.expected_reason
        );
    }
}

/// Bash の経路。複合の `Bash|mcp__.*` の matcher だった規則。
#[test]
fn attribution_guard_denies_with_the_same_reason_through_the_router() {
    check_all("attribution-guard", 3);
}

/// MCP の経路。matcher が `mcp__.*` だった規則(Gmail の送信系の MCP ツール)。
#[test]
fn external_send_guard_denies_with_the_same_reason_through_the_router() {
    check_all("external-send-guard", 14);
}
