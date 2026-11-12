//! 実バイナリを、一時の `XDG_STATE_HOME` に置いた gate-events.jsonl で動かす。
//! 実機のログには触れない。

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

fn write_events(state_home: &Path, events: &[(&str, &str)]) {
    let path = state_home.join("claude/gate-events.jsonl");
    for (gate, decision) in events {
        agent_session::gate_event::record(&path, gate, decision).unwrap();
    }
}

/// 戻り値は (stdout, 終了コード)。`threshold` が `Some` なら環境変数で渡す。
fn run_hook(state_home: &Path, threshold: Option<&str>) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gate-failopen-advisory"));
    c.env("XDG_STATE_HOME", state_home)
        // 実機の ja-polish の設定(課金の検討の知らせが有効かの印)に触れないよう、設定の場所も隔離する。
        .env("XDG_CONFIG_HOME", state_home.join("config"))
        .env("HOME", state_home)
        .env_remove("GATE_FAILOPEN_THRESHOLD")
        .env_remove("JA_POLISH_TRANSIENT_ADVISORY_PERCENT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(t) = threshold {
        c.env("GATE_FAILOPEN_THRESHOLD", t);
    }
    let out = c.output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

fn context(stdout: &str) -> String {
    let v: Value = serde_json::from_str(stdout).expect("stdout is JSON");
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_string()
}

const GATE: &str = "copilot-plan-review";

#[test]
fn no_log_is_silent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
}

#[test]
fn below_threshold_is_silent() {
    let d = tempfile::tempdir().unwrap();
    write_events(d.path(), &[(GATE, "fail-open"), (GATE, "fail-open")]);
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
}

/// 受け入れ条件: 連続 fail-open が閾値(既定 3)に届いたとき、次の SessionStart に警告が出る。
#[test]
fn at_threshold_warns() {
    let d = tempfile::tempdir().unwrap();
    write_events(
        d.path(),
        &[
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "fail-open"),
        ],
    );
    let (out, code) = run_hook(d.path(), None);
    assert_eq!(code, 0);
    let ctx = context(&out);
    assert!(ctx.starts_with("[gate-failopen-advisory]"), "{ctx}");
    assert!(ctx.contains(GATE), "{ctx}");
    assert!(ctx.contains("3 回連続"), "{ctx}");
    assert!(ctx.contains("docs/agents/copilot-plan-review.md"), "{ctx}");
}

/// 受け入れ条件: レビューが 1 回成立すると、連続回数がリセットされる。
#[test]
fn a_pass_resets_the_count() {
    let d = tempfile::tempdir().unwrap();
    write_events(
        d.path(),
        &[
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "pass"),
        ],
    );
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
    // pass のあとに再び閾値まで溜まれば、また鳴る。
    write_events(
        d.path(),
        &[
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "fail-open"),
        ],
    );
    assert!(run_hook(d.path(), None).0.contains("3 回連続"));
}

#[test]
fn other_gates_do_not_count() {
    let d = tempfile::tempdir().unwrap();
    write_events(
        d.path(),
        &[
            ("other-gate", "fail-open"),
            ("other-gate", "fail-open"),
            ("other-gate", "fail-open"),
            ("new-tool-guard", "deny"),
        ],
    );
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
}

#[test]
fn threshold_is_overridable_and_garbage_falls_back_to_default() {
    let d = tempfile::tempdir().unwrap();
    write_events(d.path(), &[(GATE, "fail-open"), (GATE, "fail-open")]);
    assert!(
        run_hook(d.path(), Some("2")).0.contains("2 回連続"),
        "閾値 2 なら 2 回で鳴る"
    );
    assert_eq!(
        run_hook(d.path(), Some("abc")),
        (String::new(), 0),
        "数値でなければ既定の 3(2 回では鳴らない)"
    );
    assert_eq!(
        run_hook(d.path(), Some("0")),
        (String::new(), 0),
        "0 は無効として既定の 3 に戻す(常に鳴らない)"
    );
}

/// ja-polish の記録(`transient` の印つきの素通しを `transient` 件、書き直しを `others` 件)を置く。
fn write_ja_polish_records(state_home: &Path, transient: usize, others: usize) {
    use ja_polish::records::{append, Entry};
    let path = state_home.join("claude/ja-polish/records.jsonl");
    let mut entries = Vec::new();
    for i in 0..transient {
        entries.push(Entry::skipped(format!("t{i}"), "m", "混雑").with_transient(true));
    }
    for i in 0..others {
        entries.push(Entry::polished(format!("p{i}"), "m"));
    }
    append(&path, &entries).unwrap();
}

fn enable_ja_polish(state_home: &Path) {
    let p = state_home.join("config/dotfiles/ja-polish.toml");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, "model = \"m\"\n").unwrap();
}

/// 受け入れ条件(段 4): ja-polish が有効な機体で、直近 7 日の素通しのうち、混雑・時間切れによるものが
/// 5% を超えたら、次の SessionStart に、課金の検討を知らせる。
#[test]
fn ja_polish_billing_advisory_fires_above_the_rate() {
    let d = tempfile::tempdir().unwrap();
    enable_ja_polish(d.path());
    write_ja_polish_records(d.path(), 2, 18);
    let (out, code) = run_hook(d.path(), None);
    assert_eq!(code, 0);
    let ctx = context(&out);
    assert!(ctx.starts_with("[gate-failopen-advisory]"), "{ctx}");
    assert!(ctx.contains("20 段落のうち 2 段落(10%)"), "{ctx}");
    assert!(ctx.contains("有料枠を検討"), "{ctx}");
}

#[test]
fn ja_polish_billing_advisory_is_silent_below_the_rate_or_without_samples_or_when_disabled() {
    // 率が閾値以下(1/20 = 5%、超えていない)。
    let d = tempfile::tempdir().unwrap();
    enable_ja_polish(d.path());
    write_ja_polish_records(d.path(), 1, 19);
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));

    // 件数が下限(20)に満たない。
    let few = tempfile::tempdir().unwrap();
    enable_ja_polish(few.path());
    write_ja_polish_records(few.path(), 3, 6);
    assert_eq!(run_hook(few.path(), None), (String::new(), 0));

    // 設定ファイルが無い機体(ja-polish を有効にしていない)では、記録があっても黙る。
    let off = tempfile::tempdir().unwrap();
    write_ja_polish_records(off.path(), 10, 10);
    assert_eq!(run_hook(off.path(), None), (String::new(), 0));
}

#[test]
fn ja_polish_billing_threshold_is_overridable_by_environment() {
    let d = tempfile::tempdir().unwrap();
    enable_ja_polish(d.path());
    write_ja_polish_records(d.path(), 1, 19); // 5%
    let mut c = Command::new(env!("CARGO_BIN_EXE_gate-failopen-advisory"));
    c.env("XDG_STATE_HOME", d.path())
        .env("XDG_CONFIG_HOME", d.path().join("config"))
        .env("HOME", d.path())
        .env("JA_POLISH_TRANSIENT_ADVISORY_PERCENT", "4")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = c.output().unwrap();
    let ctx = context(&String::from_utf8(out.stdout).unwrap());
    assert!(ctx.contains("閾値 4%"), "{ctx}");
}
