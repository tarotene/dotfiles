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
    /// fixture の `stdout`(trycmd が `\` を `/` に正規化したあとの形)。
    expected_stdout: String,
    env_add: BTreeMap<String, String>,
    env_remove: Vec<String>,
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Claude 形(引数なし、または `--agent claude` だけ)の fixture のうち、`keep` が通すもの。
fn load_cases(crate_dir: &str, keep: impl Fn(&toml::Value) -> bool) -> Vec<Case> {
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
        if !claude_shaped || !keep(&v) {
            continue;
        }
        let stdout = v.get("stdout").and_then(|s| s.as_str()).unwrap_or_default();
        let expected_reason = serde_json::from_str::<serde_json::Value>(stdout)
            .ok()
            .and_then(|e| {
                e["hookSpecificOutput"]["permissionDecisionReason"]
                    .as_str()
                    .map(String::from)
            })
            .unwrap_or_default();
        let env = v.get("env");
        out.push(Case {
            file: file.clone(),
            stdin: v["stdin"].as_str().unwrap().to_string(),
            expected_reason,
            expected_stdout: stdout.to_string(),
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

/// 指定した判定(`deny` または `ask`)を期待する fixture。
fn decision_cases(crate_dir: &str, decision: &str) -> Vec<Case> {
    load_cases(crate_dir, |v| {
        let Some(stdout) = v.get("stdout").and_then(|s| s.as_str()) else {
            return false;
        };
        let Ok(expected) = serde_json::from_str::<serde_json::Value>(stdout) else {
            return false;
        };
        expected["hookSpecificOutput"]["permissionDecision"] == decision
    })
}

fn router_reason(case: &Case) -> Option<String> {
    router_output(case, false)?["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .map(String::from)
}

/// ルータの標準出力(JSON)。出力が無ければ `None`。`stub_gh` は、`gh` の存在だけを見る検査
/// (`plan-scope-gate`、ADR-0005)を、ホストの `gh` に頼らず動かすための、常に失敗するスタブ。
fn router_output(case: &Case, stub_gh: bool) -> Option<serde_json::Value> {
    let home = tempfile::tempdir().unwrap();
    let mut path = std::env::var_os("PATH").unwrap_or_default();
    if stub_gh {
        let bin = home.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut dirs = vec![bin];
        dirs.extend(std::env::split_paths(&path));
        path = std::env::join_paths(dirs).unwrap();
    }
    let mut cmd = Command::new(BIN);
    cmd.arg("pre")
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        // 例文に埋める取得日(旧 bin の fixture と同じ固定値)。
        .env("PLAN_PRECEDENT_GATE_TODAY", "2000-01-01");
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
    serde_json::from_slice(&out.stdout).ok()
}

fn check_decision(crate_dir: &str, decision: &str, at_least: usize) {
    let cases = decision_cases(crate_dir, decision);
    assert!(
        cases.len() >= at_least,
        "{crate_dir}: {decision} を期待する fixture が {} 件(下限 {at_least})",
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
    check_decision("attribution-guard", "deny", 3);
}

/// MCP の経路。matcher が `mcp__.*` だった規則(Gmail の送信系の MCP ツール)。
#[test]
fn external_send_guard_denies_with_the_same_reason_through_the_router() {
    check_decision("external-send-guard", "deny", 7);
}

/// 外部宛の Gmail 送信は、deny でなく ask(本人の許可の確認)。旧 bin と同じ理由が、ルータの
/// ask の応答にも含まれることを確かめる。
#[test]
fn external_send_guard_asks_with_the_same_reason_through_the_router() {
    check_decision("external-send-guard", "ask", 8);
}

/// ExitPlanMode の拒否 2 件。旧 bin の fixture のうち、Claude の PreToolUse で `ExitPlanMode` を
/// 呼ぶもの(標準出力が空のものも含む)を、ルータにも流す。ルータの出力を旧 bin と同じ書式に
/// 描き直して(`jqfmt::deny_for_event`、trycmd の `\` → `/` の正規化を同じに当てる)、fixture の
/// 標準出力と**全文一致**することを確かめる。判定は同じ `hook` 関数なので、ここで確かめるのは
/// 表の `Tools` と、判定の結果を出力へ運ぶ道(取りこぼしと、書式の差)である。
///
/// ルータは ExitPlanMode の 2 件を同時に走らせるので、もう片方は `skip_other`(その検査の
/// 環境変数のスキップ)で止める。そうしないと、片方の fixture の計画をもう片方が拒否する。
fn check_exit_plan_mode(crate_dir: &str, deny_at_least: usize, stub_gh: bool, skip_other: &str) {
    let mut cases = load_cases(crate_dir, |v| {
        let Some(stdin) = v
            .get("stdin")
            .and_then(|s| s.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        else {
            return false;
        };
        // 旧 bin は `tool_name` を読まない(matcher が絞っていた)ので、無い fixture もある。
        // ルータは `tool_name` で規則を選ぶので、実際の呼び出しと同じく `ExitPlanMode` を足して流す。
        (stdin["tool_name"].is_null() || stdin["tool_name"] == "ExitPlanMode")
            && stdin["hook_event_name"] == "PreToolUse"
    });
    for case in &mut cases {
        case.env_add.insert(skip_other.into(), "1".into());
        let mut stdin: serde_json::Value = serde_json::from_str(&case.stdin).unwrap();
        stdin["tool_name"] = "ExitPlanMode".into();
        case.stdin = stdin.to_string();
    }
    let mut denies = 0;
    for case in &cases {
        let out = router_output(case, stub_gh);
        let reason = out
            .as_ref()
            .and_then(|v| v["hookSpecificOutput"]["permissionDecisionReason"].as_str());
        assert!(
            out.as_ref()
                .is_none_or(|v| v.get("systemMessage").is_none()),
            "{}: 検査が素通りになった: {out:?}",
            case.file.display()
        );
        let got = reason
            .map(|r| jq_json_compat::jqfmt::deny_for_event("PreToolUse", r).replace('\\', "/"))
            .unwrap_or_default();
        assert_eq!(
            got.trim_end(),
            case.expected_stdout.trim_end(),
            "{}: ルータの出力が、旧 bin の標準出力と違う",
            case.file.display()
        );
        denies += usize::from(reason.is_some());
    }
    assert!(
        denies >= deny_at_least,
        "{crate_dir}: 拒否を期待する fixture が {denies} 件(下限 {deny_at_least})、対象の fixture は {} 件",
        cases.len()
    );
}

#[test]
fn plan_scope_gate_matches_the_old_bin_through_the_router() {
    check_exit_plan_mode("plan-scope-gate", 2, true, "SKIP_PLAN_PRECEDENT_GATE");
}

#[test]
fn plan_precedent_gate_matches_the_old_bin_through_the_router() {
    check_exit_plan_mode("plan-precedent-gate", 10, false, "SKIP_PLAN_SCOPE_GATE");
}

/// 旧 bin は PreToolUse の matcher `ExitPlanMode` だけで呼ばれていた。表の `Tools` は、ほかのツールには
/// 当てない(同じ入力を `Write` で送っても、2 件は拒否しない)。
#[test]
fn the_two_plan_gates_apply_to_exit_plan_mode_only() {
    let denying = load_cases("plan-scope-gate", |v| {
        v.get("stdin")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.contains("\"tool_name\": \"ExitPlanMode\""))
            && v.get("stdout")
                .and_then(|s| s.as_str())
                .is_some_and(|s| s.contains("\"deny\""))
    });
    assert!(!denying.is_empty());
    for case in denying {
        let mut other = case;
        other.stdin = other.stdin.replace("ExitPlanMode", "Write");
        let out = router_output(&other, true);
        assert!(
            out.is_none_or(|v| v["hookSpecificOutput"]["permissionDecision"].is_null()),
            "{}: ExitPlanMode 以外のツールで拒否した",
            other.file.display()
        );
    }
}
