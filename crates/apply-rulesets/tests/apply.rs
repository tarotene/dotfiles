//! `scripts/apply-rulesets.sh --selftest` の全ケース(1 / 1c / 2 / 3 / 3b / 4 / 5 / 6 /
//! 7 / 8 / 9 / 10 / 10b / 11 の 14 ケース)を、PATH 上ではなく
//! `GITHUB_AUDIT_GH_BIN` で差し替えた `gh` スタブに対して再現する統合テスト
//! (#414)。スタブは bash selftest と同じ「パス→fixture」規約で、書込み系
//! (-X が GET 以外)は `STUB_LOG` に `METHOD PATH NAME` を 1 行残す。
//! bash 版が `--jq` をスタブ側で適用していたのに対し、Rust 版は `--jq` を使わず
//! 生の JSON を自分で読むので、スタブは fixture をそのまま返すだけでよい。
//! 末尾に bash selftest が持たなかった追加ケース(open PR 警告・出力の逐語)を足す。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GH_STUB: &str = r#"#!/bin/sh
dir="$(dirname "$0")"
fixtures="$dir/../fixtures"
if [ "$1" != api ]; then echo "unexpected gh invocation: $*" >&2; exit 1; fi
shift
method=GET
path=""
body=""
while [ $# -gt 0 ]; do
  case "$1" in
    -X | --method) method="$2"; shift 2 ;;
    -F | -H) shift 2 ;;
    --input) body="$(cat)"; shift 2 ;;
    *) if [ -z "$path" ]; then path="$1"; fi; shift ;;
  esac
done
if [ "$method" != GET ]; then
  name="$(printf '%s' "$body" | sed -n 's/.*"name": *"\([^"]*\)".*/\1/p' | head -1)"
  printf '%s %s %s\n' "$method" "$path" "$name" >>"$STUB_LOG"
  if [ -f "$dir/write-fail" ]; then exit 1; fi
  if [ "$method" = POST ]; then printf '{"id": 999}'; fi
  exit 0
fi
key="$(printf '%s' "$path" | tr '/' '_')"
if [ -f "$dir/fail-$key" ]; then exit 1; fi
if [ ! -f "$fixtures/$key.json" ]; then echo "no fixture for: $key" >&2; exit 1; fi
cat "$fixtures/$key.json"
"#;

const SECURITY: &str = r#"{"name": "Security", "target": "branch", "enforcement": "active", "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}, "rules": [{"type": "deletion"}]}"#;
const WORKFLOW: &str = r#"{"name": "Workflow", "target": "branch", "enforcement": "active", "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}, "rules": [{"type": "non_fast_forward"}]}"#;
const QUALITY: &str = r#"{"name": "Quality", "target": "branch", "enforcement": "active", "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}, "rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "test"}, {"context": "PR Title / PR title"}]}}]}"#;

const ALL_JOBS: &str = r#"[{"name":"test"},{"name":"PR Title / PR title"}]"#;

struct Fx {
    dir: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("bin")).unwrap();
        std::fs::create_dir_all(p.join("fixtures")).unwrap();
        std::fs::create_dir_all(p.join("decl")).unwrap();
        std::fs::write(p.join("bin/gh"), GH_STUB).unwrap();
        std::fs::set_permissions(p.join("bin/gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(p.join("decl/security.json"), SECURITY).unwrap();
        std::fs::write(p.join("decl/workflow.json"), WORKFLOW).unwrap();
        std::fs::write(p.join("decl/quality.json"), QUALITY).unwrap();
        std::fs::write(p.join("calls.log"), "").unwrap();
        let fx = Fx { dir };
        fx.fixture("repos_tarotene_x_rulesets", "[]");
        fx
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn decl(&self) -> PathBuf {
        self.path().join("decl")
    }

    fn fixture(&self, key: &str, body: &str) {
        std::fs::write(self.path().join(format!("fixtures/{key}.json")), body).unwrap();
    }

    fn rm_fixture(&self, key: &str) {
        let _ = std::fs::remove_file(self.path().join(format!("fixtures/{key}.json")));
    }

    /// 実測 job 名の fixture(runs 一覧 + 各 run の jobs)。
    fn jobs(&self, jobs: &str) {
        self.fixture(
            "repos_tarotene_x_actions_runs",
            r#"{"workflow_runs":[{"id":1}]}"#,
        );
        self.fixture(
            "repos_tarotene_x_actions_runs_1_jobs",
            &format!(r#"{{"jobs":{jobs}}}"#),
        );
    }

    fn pulls(&self, sha: Option<&str>) {
        self.fixture(
            "repos_tarotene_x_pulls",
            &match sha {
                Some(s) => format!(r#"[{{"head":{{"sha":"{s}"}}}}]"#),
                None => "[]".to_string(),
            },
        );
    }

    fn reset_log(&self) {
        std::fs::write(self.path().join("calls.log"), "").unwrap();
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path().join("calls.log")).unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_apply-rulesets"))
            .args(args)
            .env("GITHUB_AUDIT_GH_BIN", self.path().join("bin/gh"))
            .env("STUB_LOG", self.path().join("calls.log"))
            .output()
            .unwrap()
    }

    /// `tarotene/x --ref main --from-dir <decl>` を前置して走らせる。
    fn apply(&self, extra: &[&str]) -> Output {
        let decl = self.decl();
        let mut a = vec![
            "tarotene/x",
            "--ref",
            "main",
            "--from-dir",
            decl.to_str().unwrap(),
        ];
        a.extend_from_slice(extra);
        self.run(&a)
    }
}

fn rc(o: &Output) -> i32 {
    o.status.code().unwrap()
}
fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn b64_wrapped(data: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for chunk in data.as_bytes().chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, &b)| a | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    // gh は 60 桁ごとに改行を入れる(JSON 文字列中は `\n`)
    s.as_bytes()
        .chunks(60)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("\\n")
}

#[test]
fn t1_from_dir_all_contexts_reportable_posts_three_rulesets() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    let o = fx.apply(&["--verify-sha", "abc"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    let log = fx.log();
    assert!(
        log.contains("POST repos/tarotene/x/rulesets Security"),
        "{log}"
    );
    assert!(
        log.contains("POST repos/tarotene/x/rulesets Quality"),
        "{log}"
    );
    assert!(
        log.contains("POST repos/tarotene/x/rulesets Workflow"),
        "{log}"
    );
    let out = out(&o);
    assert!(
        out.contains("Applying declared rulesets to: tarotene/x (ref=main, reconcile=false)\n\n"),
        "{out}"
    );
    assert!(out.contains("  ✓  Created 'Security' (id=999)"), "{out}");
}

#[test]
fn t1c_remote_fetch_matches_from_dir() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    fx.fixture(
        "repos_tarotene_x_contents_.github_rulesets",
        r#"[{"name":"security.json"},{"name":"quality.json"},{"name":"workflow.json"},{"name":"README.md"}]"#,
    );
    for (n, body) in [
        ("security", SECURITY),
        ("quality", QUALITY),
        ("workflow", WORKFLOW),
    ] {
        fx.fixture(
            &format!("repos_tarotene_x_contents_.github_rulesets_{n}.json"),
            &format!(r#"{{"content":"{}"}}"#, b64_wrapped(body)),
        );
    }
    let o = fx.run(&["tarotene/x", "--ref", "main", "--verify-sha", "abc"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    let log = fx.log();
    assert!(
        log.contains("POST repos/tarotene/x/rulesets Quality"),
        "{log}"
    );
    assert!(
        log.contains("POST repos/tarotene/x/rulesets Security"),
        "{log}"
    );
}

#[test]
fn t2_existing_ruleset_is_skipped_without_reconcile_and_put_with_it() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    fx.fixture(
        "repos_tarotene_x_rulesets",
        r#"[{"id": 1, "name": "Quality", "target": "branch"}]"#,
    );
    let o = fx.apply(&["--verify-sha", "abc"]);
    assert!(
        !fx.log().contains("PUT repos/tarotene/x/rulesets/1"),
        "{}",
        fx.log()
    );
    assert!(
        out(&o).contains(
            "  ⚠   'Quality' already exists (id=1) — skipping (pass --reconcile to update)."
        ),
        "{}",
        out(&o)
    );
    fx.reset_log();
    let o = fx.apply(&["--verify-sha", "abc", "--reconcile"]);
    assert!(
        fx.log().contains("PUT repos/tarotene/x/rulesets/1 Quality"),
        "{}",
        fx.log()
    );
    assert!(
        out(&o).contains("  ✓  Reconciled 'Quality' (id=1)"),
        "{}",
        out(&o)
    );
}

#[test]
fn reconcile_dry_run_shows_the_required_context_diff() {
    // ADR-591 D8: live はまだ旧 context を必須にしている(#744)。
    let fx = Fx::new();
    fx.fixture(
        "repos_tarotene_x_rulesets",
        r#"[{"id": 1, "name": "Quality", "target": "branch"}]"#,
    );
    fx.fixture(
        "repos_tarotene_x_rulesets_1",
        r#"{"id": 1, "name": "Quality", "rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "test"}, {"context": "PR Title / PR title (old)"}]}}]}"#,
    );
    let o = fx.apply(&["--reconcile", "--dry-run", "--unverified-contexts"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    let s = out(&o);
    assert!(s.contains("DRY-RUN: would PUT 'Quality' (id=1)"), "{s}");
    assert!(s.contains("      - PR Title / PR title (old)"), "{s}");
    assert!(s.contains("      + PR Title / PR title"), "{s}");
    assert!(!s.contains("      - test"), "{s}");
    assert_eq!(fx.log(), "", "dry-run なのに書込みが発生した");
}

#[test]
fn reconcile_dry_run_says_when_nothing_differs_or_live_is_unreadable() {
    let fx = Fx::new();
    fx.fixture(
        "repos_tarotene_x_rulesets",
        r#"[{"id": 1, "name": "Quality", "target": "branch"}]"#,
    );
    fx.fixture("repos_tarotene_x_rulesets_1", QUALITY);
    let o = fx.apply(&["--reconcile", "--dry-run", "--unverified-contexts"]);
    assert!(
        out(&o).contains("required context: 宣言と live は同じ"),
        "{}",
        out(&o)
    );
    // live を読めないときは「差分なし」と言わない
    fx.rm_fixture("repos_tarotene_x_rulesets_1");
    let o = fx.apply(&["--reconcile", "--dry-run", "--unverified-contexts"]);
    assert!(
        out(&o).contains("live の ruleset を読めず、required context の差分は未確認"),
        "{}",
        out(&o)
    );
}

#[test]
fn t3_unreportable_context_exits_4_with_zero_writes_and_unverified_overrides() {
    let fx = Fx::new();
    fx.jobs(r#"[{"name":"unrelated"}]"#);
    let o = fx.apply(&["--verify-sha", "def"]);
    assert_eq!(rc(&o), 4, "{}", err(&o));
    assert_eq!(fx.log(), "", "書込みが発生した");
    let e = err(&o);
    assert!(e.contains("  以下の required context は def の実測 job 名に含まれません:\n    - PR Title / PR title\n    - test\n"), "{e}");
    assert!(e.contains("ERROR: refusing to apply"), "{e}");

    // 3b
    let o = fx.apply(&["--verify-sha", "def", "--unverified-contexts"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    assert!(
        err(&o).contains("WARNING: applying 2 unverified context(s)"),
        "{}",
        err(&o)
    );
}

#[test]
fn t4_no_declaration_exits_3_and_points_at_seed() {
    let fx = Fx::new();
    let empty = fx.path().join("empty-decl");
    let o = fx.run(&[
        "tarotene/x",
        "--ref",
        "main",
        "--from-dir",
        empty.to_str().unwrap(),
        "--verify-sha",
        "abc",
    ]);
    assert_eq!(rc(&o), 3);
    let e = err(&o);
    assert!(e.contains("has no .github/rulesets/ declaration (missing: security.json quality.json workflow.json)"), "{e}");
    assert!(e.to_lowercase().contains("seed"), "{e}");
}

#[test]
fn t5_dry_run_writes_nothing() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    let o = fx.apply(&["--verify-sha", "abc", "--dry-run"]);
    assert_eq!(rc(&o), 0);
    assert_eq!(fx.log(), "", "dry-run なのに書込みが発生した");
    assert!(
        out(&o).contains("  DRY-RUN: would POST ruleset 'Quality'"),
        "{}",
        out(&o)
    );
}

#[test]
fn t6_placeholder_left_over_exits_5() {
    let fx = Fx::new();
    let bad = fx.path().join("decl-bad");
    std::fs::create_dir_all(&bad).unwrap();
    for n in ["security", "quality", "workflow"] {
        std::fs::copy(
            fx.decl().join(format!("{n}.json")),
            bad.join(format!("{n}.json")),
        )
        .unwrap();
    }
    let q = std::fs::read_to_string(bad.join("quality.json")).unwrap();
    std::fs::write(
        bad.join("quality.json"),
        q.replace("\"test\"", "\"__CLI_CRATE__ CLI\""),
    )
    .unwrap();
    let o = fx.run(&[
        "tarotene/x",
        "--ref",
        "main",
        "--from-dir",
        bad.to_str().unwrap(),
        "--verify-sha",
        "abc",
    ]);
    assert_eq!(rc(&o), 5);
    assert!(
        err(&o).contains("ERROR: quality.json still has an unreplaced placeholder"),
        "{}",
        err(&o)
    );
    assert_eq!(fx.log(), "");
}

#[test]
fn t7_verify_sha_given_does_not_call_pulls() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    // pulls fixture が無い: 呼ばれたら gh スタブが exit 1 になり、PR 解決が空になる。
    // verify-sha 指定なら呼ばれないので exit 0 のまま。
    fx.rm_fixture("repos_tarotene_x_pulls");
    let o = fx.apply(&["--verify-sha", "abc"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
}

#[test]
fn t8_verify_sha_resolved_from_latest_pr() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    fx.pulls(Some("abc"));
    let o = fx.apply(&[]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    // 解決された sha で実測が走ったことを、報告不能な context で確かめる
    fx.jobs(r#"[{"name":"unrelated"}]"#);
    let o = fx.apply(&[]);
    assert_eq!(rc(&o), 4, "{}", err(&o));
    assert!(err(&o).contains("abc の実測 job 名"), "{}", err(&o));
}

#[test]
fn t8b_no_pr_and_no_sha_skips_verification_and_applies() {
    let fx = Fx::new();
    fx.pulls(None);
    let o = fx.apply(&[]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    assert!(
        err(&o).contains("検証対象コミットが無いため"),
        "{}",
        err(&o)
    );
    assert!(
        fx.log().contains("POST repos/tarotene/x/rulesets Quality"),
        "{}",
        fx.log()
    );
}

#[test]
fn t9_undeclared_ruleset_is_reported_not_deleted() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    fx.fixture(
        "repos_tarotene_x_rulesets",
        r#"[{"id": 1, "name": "Ephemeral Initial", "target": "branch"}]"#,
    );
    let o = fx.apply(&["--verify-sha", "abc", "--reconcile"]);
    assert!(!fx.log().contains("DELETE"), "{}", fx.log());
    let out = out(&o);
    assert!(
        out.contains("\nNOTE: 1 active branch ruleset(s) not in the declaration:\n  - Ephemeral Initial (id=1)\n  These are reported, not deleted.\n"),
        "{out}"
    );
}

#[test]
fn t10_delete_ruleset_deletes_when_undeclared_and_refuses_when_declared() {
    let fx = Fx::new();
    fx.fixture(
        "repos_tarotene_x_rulesets",
        r#"[{"id": 5, "name": "Review", "target": "branch"}]"#,
    );
    let o = fx.apply(&["--delete-ruleset", "Review"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    assert!(
        fx.log().contains("DELETE repos/tarotene/x/rulesets/5"),
        "{}",
        fx.log()
    );
    assert!(
        out(&o).contains("  ✓  Deleted 'Review' (id=5)"),
        "{}",
        out(&o)
    );

    // 10b: 宣言に有るときは拒否
    let review = fx.path().join("decl-review");
    std::fs::create_dir_all(&review).unwrap();
    for n in ["security", "quality", "workflow"] {
        std::fs::copy(
            fx.decl().join(format!("{n}.json")),
            review.join(format!("{n}.json")),
        )
        .unwrap();
    }
    std::fs::write(
        review.join("review.json"),
        WORKFLOW.replace("Workflow", "Review"),
    )
    .unwrap();
    fx.reset_log();
    let o = fx.run(&[
        "tarotene/x",
        "--ref",
        "main",
        "--from-dir",
        review.to_str().unwrap(),
        "--delete-ruleset",
        "Review",
    ]);
    assert_eq!(rc(&o), 1);
    assert!(
        err(&o).contains("'Review' is still declared in .github/rulesets/"),
        "{}",
        err(&o)
    );
    assert_eq!(fx.log(), "");
}

#[test]
fn t10c_delete_ruleset_variants() {
    let fx = Fx::new();
    // 該当 ruleset が無ければ何もしない
    let o = fx.apply(&["--delete-ruleset", "Review"]);
    assert_eq!(rc(&o), 0);
    assert!(
        out(&o).contains("'Review' is not an active branch ruleset — nothing to delete."),
        "{}",
        out(&o)
    );
    // --dry-run は DELETE しない
    fx.fixture(
        "repos_tarotene_x_rulesets",
        r#"[{"id": 5, "name": "Review", "target": "branch"}]"#,
    );
    let o = fx.apply(&["--delete-ruleset", "Review", "--dry-run"]);
    assert!(
        out(&o).contains("DRY-RUN: would DELETE 'Review' (id=5)"),
        "{}",
        out(&o)
    );
    assert_eq!(fx.log(), "");
}

#[test]
fn t11_open_prs_satisfying_contexts_get_no_warning() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    fx.fixture(
        "repos_tarotene_x_pulls",
        r#"[{"number":7,"title":"open pr","head":{"sha":"abc"}}]"#,
    );
    let o = fx.apply(&["--verify-sha", "abc"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    let all = format!("{}{}", out(&o), err(&o));
    assert!(
        !all.contains("PR #7"),
        "満たしている PR に警告が出た: {all}"
    );
}

#[test]
fn t11b_open_pr_missing_a_context_is_warned() {
    // bash selftest 11 の注記どおり、スタブは head_sha でフィルタできないので
    // 「PR の head が報告する job 名」は 1 つの fixture で決まる。--verify-sha の
    // 検証は --unverified-contexts で飛ばし、警告の配線だけを確かめる。
    let fx = Fx::new();
    fx.jobs(r#"[{"name":"test"}]"#);
    fx.fixture(
        "repos_tarotene_x_pulls",
        r#"[{"number":7,"title":"open pr","head":{"sha":"abc"}}]"#,
    );
    let o = fx.apply(&["--verify-sha", "abc", "--unverified-contexts"]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    let e = err(&o);
    assert!(
        e.contains("  ⚠   PR #7 (open pr) の head には次の required context が走っていません — rebase して再 push してください:\n      - PR Title / PR title\n"),
        "{e}"
    );
    // --dry-run では走査しない
    let o = fx.apply(&["--verify-sha", "abc", "--unverified-contexts", "--dry-run"]);
    assert!(!err(&o).contains("PR #7"), "{}", err(&o));
}

#[test]
fn write_failure_propagates_the_exit_code() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    std::fs::write(fx.path().join("bin/write-fail"), "").unwrap();
    let o = fx.apply(&["--verify-sha", "abc"]);
    assert_eq!(rc(&o), 1);
}

#[test]
fn argument_errors() {
    let fx = Fx::new();
    let o = fx.run(&[]);
    assert_eq!(rc(&o), 2);
    assert!(
        err(&o).contains("usage: apply-rulesets.sh <owner/repo>"),
        "{}",
        err(&o)
    );
    let o = fx.run(&["not-owner-repo"]);
    assert_eq!(rc(&o), 2);
    assert!(err(&o).contains("'not-owner-repo' is not owner/repo"));
    let o = fx.run(&["tarotene/x", "--bogus"]);
    assert_eq!(rc(&o), 2);
    assert!(err(&o).contains("Unknown option: --bogus"));
    let o = fx.run(&["--help"]);
    assert_eq!(rc(&o), 0);
    assert!(out(&o).contains("usage: apply-rulesets.sh <owner/repo>"));
}

#[test]
fn default_branch_is_resolved_when_ref_is_omitted() {
    let fx = Fx::new();
    fx.jobs(ALL_JOBS);
    fx.fixture("repos_tarotene_x", r#"{"default_branch":"trunk"}"#);
    let decl = fx.decl();
    let o = fx.run(&[
        "tarotene/x",
        "--from-dir",
        decl.to_str().unwrap(),
        "--verify-sha",
        "abc",
    ]);
    assert_eq!(rc(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("(ref=trunk, reconcile=false)"),
        "{}",
        out(&o)
    );
    // 解決できなければ exit 1
    fx.rm_fixture("repos_tarotene_x");
    let o = fx.run(&["tarotene/x", "--from-dir", decl.to_str().unwrap()]);
    assert_eq!(rc(&o), 1);
    assert!(
        err(&o).contains("could not resolve default branch for tarotene/x"),
        "{}",
        err(&o)
    );
}
