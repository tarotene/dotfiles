//! `pr-checks-wait`(赤 1 件で即 NG、required 全緑で即 OK の待ち)の統合テスト。
//! gh はスタブ(tests/common)。`gh pr checks --watch` を呼ぶとスタブが exit 97 で落ちる。

mod common;

use common::*;

fn with_rules<'a>(fx: &Fx, rules: &str, extra: &[(&'a str, &'a str)]) -> Vec<(String, String)> {
    let mut v = vec![(
        "PR_GATE_STUB_RULES_FILE".to_string(),
        fx.write("rules.json", rules),
    )];
    v.extend(
        extra
            .iter()
            .map(|(k, val)| (k.to_string(), val.to_string())),
    );
    v
}

fn run(fx: &Fx, args: &[&str], envs: &[(String, String)]) -> Run {
    let e: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    fx.run_wait(args, &e)
}

fn checks_file(fx: &Fx, body: &str) -> (String, String) {
    (
        "PR_GATE_STUB_CHECKS_FILE".to_string(),
        fx.write("checks.json", body),
    )
}

#[test]
fn ok_as_soon_as_required_are_green_even_if_another_is_pending() {
    let fx = Fx::new();
    let mut envs = with_rules(&fx, RULES_2, &[]);
    envs.push(checks_file(&fx, CHECKS_REQ_PASS_OTHER_PENDING));
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(r.stdout.contains("OK: PR #37"), "{}", r.stdout);
    assert!(r.stdout.contains("required でないチェックは待っていません"));
    assert!(r.stdout.contains("altair"));
}

#[test]
fn ng_as_soon_as_any_check_is_red_even_if_required_are_pending() {
    let fx = Fx::new();
    let mut envs = with_rules(&fx, RULES_2, &[]);
    envs.push(checks_file(&fx, CHECKS_OTHER_FAIL_REQ_PENDING));
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 1, "{}", r.all());
    assert!(r.stdout.contains("NG: PR #37"));
    assert!(r.stdout.contains("失敗: lint"));
    assert!(r.stdout.contains("gh run view 4 --log-failed --job 41"));
    assert!(r.stdout.contains("まだ実行中")); // required は待たずに終える
    assert!(r.stdout.contains("job-a"));
}

#[test]
fn waits_until_required_turn_green() {
    // 1 回目は required が pending、2 回目で全緑。取り直しながら待って OK になる。
    let fx = Fx::new();
    let seq = fx.dir().join("seq");
    let seq = seq.display().to_string();
    std::fs::write(format!("{seq}.0"), CHECKS_PARTIAL_PENDING).unwrap();
    std::fs::write(format!("{seq}.1"), CHECKS_REQ_PASS_OTHER_PENDING).unwrap();
    let log = fx.write("gh.log", "");
    let envs = with_rules(
        &fx,
        RULES_2,
        &[
            ("PR_GATE_STUB_CHECKS_SEQ", seq.as_str()),
            ("PR_GATE_STUB_LOG", log.as_str()),
        ],
    );
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 0, "{}", r.all());
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.matches("pr checks").count() >= 2,
        "取り直していない: {calls}"
    );
    assert!(!calls.contains("--watch"), "--watch は呼ばない: {calls}");
}

#[test]
fn partial_set_that_is_green_is_not_ok() {
    // cli/cli#9973: 出現した部分集合(job-a)が緑でも、job-b が未出現なら OK にしない。
    let fx = Fx::new();
    let mut envs = with_rules(&fx, RULES_2, &[]);
    envs.push(checks_file(&fx, CHECKS_1OF2));
    let r = run(&fx, &["37", "--timeout", "0"], &envs);
    assert_eq!(r.code, 2, "{}", r.all());
    assert!(r.stdout.contains("UNKNOWN"));
    assert!(r.stdout.contains("job-b"));
}

#[test]
fn pending_at_timeout_is_exit_8_and_names_required_only() {
    let fx = Fx::new();
    let mut envs = with_rules(&fx, RULES_2, &[]);
    envs.push(checks_file(&fx, CHECKS_PARTIAL_PENDING));
    let r = run(&fx, &["37", "--timeout", "0"], &envs);
    assert_eq!(r.code, 8, "{}", r.all());
    assert!(r.stdout.contains("PENDING: PR #37"));
    assert!(r.stdout.contains("job-b"));
}

#[test]
fn gh_exit_8_with_json_is_still_read() {
    // pending があると gh は exit 8 を返しうる。標準出力が配列なら終了コードに関わらず読む。
    let fx = Fx::new();
    let mut envs = with_rules(&fx, RULES_2, &[("PR_GATE_STUB_CHECKS_RC", "8")]);
    envs.push(checks_file(&fx, CHECKS_REQ_PASS_OTHER_PENDING));
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 0, "{}", r.all());
}

#[test]
fn rules_api_failure_is_unknown_not_green() {
    // #135: required の取得失敗を「required 無し」と読んで緑にしない。
    let fx = Fx::new();
    let mut envs = vec![("PR_GATE_STUB_RULES_FAIL".to_string(), "1".to_string())];
    envs.push(checks_file(&fx, CHECKS_2PASS));
    let r = run(&fx, &["37"], &envs);
    assert_eq!(r.code, 2, "{}", r.all());
    assert!(r.stdout.contains("取得に失敗"));
}

#[test]
fn stacked_pr_uses_the_default_branch_required() {
    let fx = Fx::new();
    fx.set_origin_head("main", None);
    let default_rules = fx.write("default-rules.json", RULES_2);
    let mut envs = with_rules(
        &fx,
        "[]",
        &[
            ("PR_GATE_STUB_BASE", "feature-x"),
            ("PR_GATE_STUB_RULES_DEFAULT_FILE", default_rules.as_str()),
        ],
    );
    envs.push(checks_file(&fx, CHECKS_REQ_PASS_OTHER_PENDING));
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(r.stdout.contains("既定ブランチ main の required"));
}

#[test]
fn no_rules_anywhere_falls_back_to_quiesce_and_still_fails_fast() {
    let fx = Fx::new();
    let mut envs = with_rules(&fx, "[]", &[]);
    envs.push(checks_file(&fx, CHECKS_QUIESCE_FAIL));
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 1, "{}", r.all());
    let mut envs = with_rules(&fx, "[]", &[]);
    envs.push(checks_file(&fx, CHECKS_QUIESCE_PASS));
    let r = run(&fx, &["37", "--timeout", "30"], &envs);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(r.stdout.contains("ruleset の対象外"));
}

#[test]
fn no_checks_at_all_is_unknown() {
    let fx = Fx::new();
    let envs = with_rules(&fx, "[]", &[]);
    let r = run(&fx, &["37", "--timeout", "0"], &envs);
    assert_eq!(r.code, 2, "{}", r.all());
    assert!(r.stdout.contains("1 件も報告されていません"));
}

#[test]
fn pr_defaults_to_the_current_branch() {
    let fx = Fx::new();
    let mut envs = with_rules(&fx, RULES_2, &[("PR_GATE_STUB_PR_NUM", "55")]);
    envs.push(checks_file(&fx, CHECKS_2PASS));
    let r = run(&fx, &["--timeout", "30"], &envs);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(r.stdout.contains("PR #55"));
}

#[test]
fn usage_errors_exit_2_and_help_exits_0() {
    let fx = Fx::new();
    let r = run(&fx, &["--watch"], &[]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("usage:"));
    let r = run(&fx, &["--help"], &[]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("usage:"));
}
