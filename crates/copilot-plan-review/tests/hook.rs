//! hook / advisory 経路の統合テスト(旧 `copilot-plan-review.sh --selftest` の
//! 「ログ衛生」「hook 経路」「copilot 呼び出し契約」「critic の JSON 厳格検証」節)。
//!
//! 実行対象は `COPILOT_PLAN_REVIEW_UNDER_TEST`(パス)があればそれ、無ければ
//! この crate のバイナリ。移植時はまず bash 版
//! (`COPILOT_PLAN_REVIEW_UNDER_TEST=$PWD/config/claude/hooks/copilot-plan-review.sh`)
//! に対して緑にし、同じテストを Rust 版に向けた(docs/rust-migration.md 段2→段3)。
//!
//! 文面のバイト一致は `golden.rs` の期待値(bash 版の出力から生成し、一時
//! ディレクトリ・時刻・乱数部分だけをマスクしたもの)で固定する。
//! `GOLDEN_DUMP=<dir>` を付けて走らせると、比較の代わりに実出力を書き出す。

mod golden;

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

fn under_test() -> PathBuf {
    std::env::var_os("COPILOT_PLAN_REVIEW_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_copilot-plan-review")))
}

const FAKE_COPILOT: &str = r#"#!/usr/bin/env bash
set -u
prompt="" agent="" model=""
saw_silent=0 saw_no_custom=0 saw_disable_mcps=0 saw_no_ask_user=0
add_dirs=()
while [ $# -gt 0 ]; do
  case "$1" in
    -p) prompt="${2:-}"; shift 2 ;;
    --agent) agent="${2:-}"; shift 2 ;;
    --model) model="${2:-}"; shift 2 ;;
    --add-dir) add_dirs+=("${2:-}"); shift 2 ;;
    --silent) saw_silent=1; shift ;;
    --no-custom-instructions) saw_no_custom=1; shift ;;
    --disable-builtin-mcps) saw_disable_mcps=1; shift ;;
    --no-ask-user) saw_no_ask_user=1; shift ;;
    --allow-all-tools | --yolo | --allow-all | --allow-all-paths | --allow-all-urls | --allow-url*)
      echo "FAKE_COPILOT: dangerous flag $1 must never be passed" >&2
      exit 99
      ;;
    *) shift ;;
  esac
done
if [ "$agent" != "${FAKE_COPILOT_EXPECT_AGENT:-plan-reviewer}" ]; then exit 98; fi
if [ "$model" != "${FAKE_COPILOT_EXPECT_MODEL:-gpt-6-astra}" ]; then exit 97; fi
if [ "$saw_silent$saw_no_custom$saw_disable_mcps$saw_no_ask_user" != "1111" ]; then exit 96; fi
lens=M
case "$prompt" in
  *"(lens A)"*) lens=A ;;
  *"(lens B)"*) lens=B ;;
  *"(lens C)"*) lens=C ;;
  *"(lens Z"*) lens=Z ;;
esac
{
  printf 'agent=%s\n' "$agent"
  printf 'model=%s\n' "$model"
  printf 'silent=%s custom=%s mcps=%s askuser=%s\n' \
    "$saw_silent" "$saw_no_custom" "$saw_disable_mcps" "$saw_no_ask_user"
  printf 'add_dirs=%s\n' "${add_dirs[*]-}"
  printf 'agent_turn_log=%s\n' "${AGENT_TURN_LOG-unset}"
} > "$FAKE_COPILOT_DIR/last-invocation-$lens.txt" 2>/dev/null
printf '%s' "$prompt" > "$FAKE_COPILOT_DIR/last-prompt-$lens.txt"
[ -e "$FAKE_COPILOT_DIR/$lens.fail" ] && exit 1
if [ -e "$FAKE_COPILOT_DIR/$lens.fence" ]; then
  printf '```json\n'
  cat "$FAKE_COPILOT_DIR/$lens.json"
  printf '\n```\n'
else
  cat "$FAKE_COPILOT_DIR/$lens.json"
fi
[ -e "$FAKE_COPILOT_DIR/$lens.rcfail" ] && exit 1
exit 0
"#;

fn ready_all() -> Value {
    json!({"requirements":true,"scope":true,"implementation":true,"verification":true})
}
fn finding(sev: &str, summary: &str) -> Value {
    json!({"severity": sev, "kind": "TECHNICAL", "readiness_axis": "IMPLEMENTATION",
           "summary": summary, "failure_mode": "壊れる", "trigger": "常に",
           "evidence": "evidence.md:1"})
}
fn critic_data(findings: Value, carry: Value) -> Value {
    json!({"readiness": ready_all(), "findings": findings, "carryover": carry})
}
fn open1() -> Value {
    json!([{"id":"R1-A-1","severity":"BLOCKER","kind":"TECHNICAL",
            "readiness_axis":"IMPLEMENTATION","summary":"前ラウンドの指摘",
            "failure_mode":"壊れる","trigger":"常に","evidence":"a.sh:1"}])
}

/// 1 テスト分の隔離環境。
struct Env {
    _root: tempfile::TempDir,
    review: PathBuf,
    work: PathBuf,
    home: PathBuf,
    fake: PathBuf,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let review = root.path().join("reviews");
        let work = root.path().join("work");
        let home = root.path().join("home");
        let fake = root.path().join("fake");
        for d in [&review, &work, &home, &fake] {
            fs::create_dir_all(d).unwrap();
        }
        let copilot = fake.join("copilot");
        fs::write(&copilot, FAKE_COPILOT).unwrap();
        chmod_x(&copilot);
        Env {
            _root: root,
            review,
            work,
            home,
            fake,
        }
    }

    fn state(&self, f: &str) -> PathBuf {
        self.review.join("state").join(f)
    }

    fn copilot(&self) -> String {
        self.fake.join("copilot").to_string_lossy().into_owned()
    }

    fn write_fake(&self, name: &str, content: &str) {
        fs::write(self.fake.join(name), content).unwrap();
    }

    fn touch_fake(&self, name: &str) {
        fs::write(self.fake.join(name), "").unwrap();
    }

    fn rm_fake(&self, name: &str) {
        let _ = fs::remove_file(self.fake.join(name));
    }

    fn seed(&self, sid: &str, count: &str, open: &Value) {
        fs::create_dir_all(self.review.join("state")).unwrap();
        fs::write(self.state(&format!("{sid}.count")), format!("{count}\n")).unwrap();
        fs::write(self.state(&format!("{sid}.open.json")), format!("{open}\n")).unwrap();
    }

    fn command(&self, extra: &[(&str, &str)]) -> Command {
        let mut c = Command::new(under_test());
        for k in [
            "COPILOT_BIN",
            "COPILOT_PLAN_REVIEW_DIR",
            "COPILOT_PLAN_REVIEW_MODEL",
            "COPILOT_PLAN_REVIEW_AGENT",
            "COPILOT_PLAN_REVIEW_TIMEOUT",
            "COPILOT_PLAN_REVIEW_GATE_SEVERITIES",
            "COPILOT_PLAN_REVIEW_PARALLEL",
            "COPILOT_PLAN_REVIEW_RETENTION_DAYS",
            "COPILOT_PLAN_REVIEW_SCHEMA",
            "MAX_PLAN_REVIEWS",
            "SKIP_PLAN_REVIEW",
            "FAKE_COPILOT_EXPECT_MODEL",
            "FAKE_COPILOT_EXPECT_AGENT",
        ] {
            c.env_remove(k);
        }
        c.current_dir(&self.work)
            .env("HOME", &self.home)
            .env("COPILOT_PLAN_REVIEW_DIR", &self.review)
            .env("PLAN_PRECEDENT_GATE_BIN", "/dev/null")
            .env("PLAN_SCOPE_GATE_BIN", "/dev/null")
            .env("FAKE_COPILOT_DIR", &self.fake)
            .env("COPILOT_PLAN_REVIEW_TIMEOUT", "20");
        for (k, v) in extra {
            c.env(k, v);
        }
        c
    }

    /// hook を 1 回走らせて stdout を返す。
    fn hook(&self, input: &Value, extra: &[(&str, &str)]) -> String {
        let mut c = self.command(extra);
        c.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = c.spawn().unwrap();
        {
            use std::io::Write;
            let mut si = child.stdin.take().unwrap();
            let _ = si.write_all(input.to_string().as_bytes());
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook must exit 0");
        String::from_utf8(out.stdout).unwrap()
    }

    fn mask(&self, s: &str) -> String {
        let s = s.replace(&self.review.to_string_lossy().into_owned(), "<DIR>");
        mask_random(&mask_ts(&s))
    }
}

fn chmod_x(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    // 並列テストの別スレッドが fork した瞬間に書き込み fd を持っていくと、
    // 直後の exec が ETXTBSY(Text file busy)で落ちる。実行できるまで待つ。
    for _ in 0..200 {
        match Command::new(p)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("FAKE_COPILOT_DIR", "/nonexistent")
            .status()
        {
            Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(Duration::from_millis(10)),
            _ => return,
        }
    }
}

/// `\d{8}-\d{6}` を `<TS>` に。
fn mask_ts(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if i + 15 <= c.len()
            && c[i..i + 8].iter().all(char::is_ascii_digit)
            && c[i + 8] == '-'
            && c[i + 9..i + 15].iter().all(char::is_ascii_digit)
        {
            out.push_str("<TS>");
            i += 15;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// `.plan.XXXXXX.md` の乱数部分を `<RND>` に。
fn mask_random(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(".plan.") {
        out.push_str(&rest[..i + 6]);
        rest = &rest[i + 6..];
        if rest.len() >= 9 && rest.is_char_boundary(6) && rest[6..].starts_with(".md") {
            out.push_str("<RND>");
            rest = &rest[6..];
        }
    }
    out.push_str(rest);
    out
}

fn golden(name: &str, actual: &str) {
    if let Some(dir) = std::env::var_os("GOLDEN_DUMP") {
        fs::write(Path::new(&dir).join(format!("{name}.txt")), actual).unwrap();
        return;
    }
    let expected = golden::get(name).unwrap_or_else(|| panic!("golden {name} が無い"));
    assert_eq!(actual, expected, "golden {name}");
}

fn input(sid: &str) -> Value {
    json!({"session_id": sid, "cwd": ".", "hook_event_name": "PreToolUse",
           "tool_name": "ExitPlanMode", "permission_mode": "plan",
           "tool_input": {"plan": "# plan\n"}})
}

fn decision(out: &str) -> String {
    serde_json::from_str::<Value>(out)
        .ok()
        .and_then(|v| {
            v["hookSpecificOutput"]["permissionDecision"]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_else(|| "none".into())
}

fn reason(out: &str) -> String {
    serde_json::from_str::<Value>(out)
        .ok()
        .and_then(|v| {
            v["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_default()
}

fn system_message(out: &str) -> String {
    serde_json::from_str::<Value>(out)
        .ok()
        .and_then(|v| v["systemMessage"].as_str().map(String::from))
        .unwrap_or_default()
}

fn json_len(p: &Path) -> usize {
    serde_json::from_str::<Value>(&fs::read_to_string(p).unwrap())
        .unwrap()
        .as_array()
        .unwrap()
        .len()
}

fn read_trim(p: &Path) -> String {
    fs::read_to_string(p).unwrap().trim_end().to_string()
}

// ---------------------------------------------------------------------------
// ログ衛生
// ---------------------------------------------------------------------------

#[test]
fn prune_keeps_skip_and_permissions() {
    let e = Env::new();
    let old = e.review.join("20250101-000000-deadbeef.md");
    fs::write(&old, "x").unwrap();
    let skip = e.review.join("skip");
    fs::write(&skip, "").unwrap();
    let past = SystemTime::now() - Duration::from_secs(40 * 86400);
    for p in [&old, &skip] {
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(past)
            .unwrap();
    }
    // skip があるので hook は素通りするが、ensure_dirs/prune_old は先に走る
    let out = e.hook(&input("selftest-prune"), &[("COPILOT_BIN", "true")]);
    assert_eq!(out, "");
    assert!(!old.exists(), "保持期限より古い log が掃除される");
    assert!(skip.exists(), "skip フラグは掃除されない");
    fs::remove_file(&skip).unwrap();

    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&e.review), 0o700);
    assert_eq!(mode(&e.review.join("state")), 0o700);
    assert_eq!(mode(&e.review.join("backlog")), 0o700);
    assert_eq!(mode(&e.review.join("debug-last-input.json")), 0o600);
    // 緩い権限の既存ファイルは締め直される
    let loose = e.review.join("loose.md");
    fs::write(&loose, "x").unwrap();
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o644)).unwrap();
    e.hook(&input("selftest-perm"), &[("COPILOT_BIN", "true")]);
    assert_eq!(mode(&loose), 0o600);
}

// ---------------------------------------------------------------------------
// hook 経路
// ---------------------------------------------------------------------------

#[test]
fn cap_reached_with_open_escalates_once() {
    let e = Env::new();
    e.seed("selftest-cap", "3", &open1());
    let out = e.hook(&input("selftest-cap"), &[("COPILOT_BIN", "true")]);
    assert_eq!(decision(&out), "deny");
    let r = reason(&out);
    assert!(r.contains("AskUserQuestion") && r.contains("GO") && r.contains("NO-GO"));
    assert!(r.contains("前ラウンドの指摘"));
    assert!(e.state("selftest-cap.escalated").exists());
    golden("cap_escalate", &e.mask(&out));
    let out = e.hook(&input("selftest-cap"), &[("COPILOT_BIN", "true")]);
    assert_eq!(decision(&out), "none");
    golden("escalated_pass", &e.mask(&out));
}

#[test]
fn cap_reached_with_empty_open_passes() {
    let e = Env::new();
    e.seed("selftest-cap2", "3", &json!([]));
    let out = e.hook(&input("selftest-cap2"), &[("COPILOT_BIN", "true")]);
    assert_eq!(decision(&out), "none");
    golden("cap_pass", &e.mask(&out));
}

#[test]
fn copilot_absent_is_silent() {
    let e = Env::new();
    let out = e.hook(
        &input("selftest-nocopilot"),
        &[("COPILOT_BIN", "definitely-not-a-real-binary")],
    );
    assert_eq!(out, "");
    assert!(!e.state("selftest-nocopilot.count").exists());
}

#[test]
fn skip_env_and_flag() {
    let e = Env::new();
    let out = e.hook(
        &input("selftest-skip"),
        &[("COPILOT_BIN", "true"), ("SKIP_PLAN_REVIEW", "1")],
    );
    assert_eq!(out, "");
    fs::write(e.review.join("skip"), "").unwrap();
    let out = e.hook(&input("selftest-skip"), &[("COPILOT_BIN", "true")]);
    assert_eq!(out, "");
}

#[test]
fn precheck_deny_stub() {
    let e = Env::new();
    let gate = e.fake.join("fake-deny-gate.sh");
    fs::write(
        &gate,
        r#"#!/usr/bin/env bash
cat >/dev/null
jq -n '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny",
        permissionDecisionReason: "stub-deny-reason-12345"}}'
"#,
    )
    .unwrap();
    chmod_x(&gate);
    let g = gate.to_string_lossy().into_owned();
    let out = e.hook(
        &input("selftest-precheck-deny"),
        &[("COPILOT_BIN", "true"), ("PLAN_PRECEDENT_GATE_BIN", &g)],
    );
    assert_eq!(decision(&out), "deny");
    assert!(reason(&out).contains("stub-deny-reason-12345"));
    assert!(!e.state("selftest-precheck-deny.count").exists());
    golden("precheck_deny", &e.mask(&out));

    // scope 側だけが deny(PermissionRequest 形式)
    let out = e.hook(
        &json!({"session_id": "s-pr", "cwd": ".", "hook_event_name": "PermissionRequest",
                "tool_input": {"plan": "# plan\n"}}),
        &[("COPILOT_BIN", "true"), ("PLAN_SCOPE_GATE_BIN", &g)],
    );
    golden("precheck_deny_scope_permission_request", &e.mask(&out));
}

#[test]
fn precheck_broken_stub_fails_open() {
    let e = Env::new();
    let gate = e.fake.join("fake-broken-gate.sh");
    fs::write(&gate, "#!/usr/bin/env bash\ncat >/dev/null\nexit 3\n").unwrap();
    chmod_x(&gate);
    let g = gate.to_string_lossy().into_owned();
    let out = e.hook(
        &input("selftest-precheck-broken"),
        &[("COPILOT_BIN", "true"), ("PLAN_PRECEDENT_GATE_BIN", &g)],
    );
    assert_eq!(decision(&out), "none");
    assert!(system_message(&out).contains("実行に失敗しました"));
    golden("critics_failed", &e.mask(&out));
}

/// 実 sibling(テスト対象と同じディレクトリにある plan-precedent-gate /
/// plan-scope-gate、bash 版なら `.sh`)との統合。無ければ skip。
#[test]
fn precheck_real_sibling() {
    let e = Env::new();
    let dir = under_test().parent().unwrap().to_path_buf();
    let find = |name: &str| {
        [dir.join(name), dir.join(format!("{name}.sh"))]
            .into_iter()
            .find(|p| p.is_file())
    };
    let (Some(prec), Some(scope)) = (find("plan-precedent-gate"), find("plan-scope-gate")) else {
        eprintln!("skip: sibling gate が見つからない");
        return;
    };
    let input = json!({"session_id": "selftest-precheck-real", "cwd": ".",
                       "hook_event_name": "PreToolUse", "tool_name": "ExitPlanMode",
                       "permission_mode": "plan",
                       "tool_input": {"plan": "先行例: 該当なし — selftest\n"}});
    let out = e.hook(
        &input,
        &[
            ("COPILOT_BIN", "true"),
            ("PLAN_PRECEDENT_GATE_BIN", &prec.to_string_lossy()),
            ("PLAN_SCOPE_GATE_BIN", &scope.to_string_lossy()),
        ],
    );
    assert_eq!(decision(&out), "none");
    assert!(system_message(&out).contains("実行に失敗しました"));
}

#[test]
fn debug_dump_has_no_plan_body() {
    let e = Env::new();
    e.hook(&input("selftest-debug"), &[("COPILOT_BIN", "true")]);
    let v: Value =
        serde_json::from_str(&fs::read_to_string(e.review.join("debug-last-input.json")).unwrap())
            .unwrap();
    assert!(v.get("tool_input").is_none());
    assert_eq!(v["plan_chars"], 7);
    assert_eq!(v["has_plan_file_path"], false);
    assert_eq!(v["session_id"], "selftest-debug");
}

fn cp(e: &Env, extra: &[(&'static str, &'static str)]) -> Vec<(String, String)> {
    let mut v = vec![("COPILOT_BIN".to_string(), e.copilot())];
    v.extend(extra.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    v
}

fn run(e: &Env, sid: &str, extra: &[(&'static str, &'static str)]) -> String {
    let env = cp(e, extra);
    let refs: Vec<(&str, &str)> = env.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    e.hook(&input(sid), &refs)
}

#[test]
fn parallel_round_flow_and_closer() {
    let e = Env::new();
    e.write_fake(
        "A.json",
        &critic_data(json!([finding("BLOCKER", "lens A の指摘")]), json!([])).to_string(),
    );
    e.touch_fake("B.fail");
    let out = run(&e, "selftest-parallel", &[]);
    assert_eq!(decision(&out), "deny");
    assert!(reason(&out).contains("lens B"));
    assert_eq!(read_trim(&e.state("selftest-parallel.count")), "1");
    assert_eq!(json_len(&e.state("selftest-parallel.open.json")), 1);
    golden("round1_deny", &e.mask(&out));
    golden(
        "prompt_lens_a",
        &e.mask(&fs::read_to_string(e.fake.join("last-prompt-A.txt")).unwrap()),
    );
    // critic には AGENT_TURN_LOG=0 が渡る
    assert!(fs::read_to_string(e.fake.join("last-invocation-A.txt"))
        .unwrap()
        .contains("agent_turn_log=0"));

    // ラウンド 2: lens C が carry-over を解消
    e.write_fake(
        "C.json",
        &critic_data(
            json!([]),
            json!([{"id": "R1-A-1", "status": "RESOLVED", "rationale": "修正済み"}]),
        )
        .to_string(),
    );
    let out = run(&e, "selftest-parallel", &[]);
    assert_eq!(decision(&out), "none");
    assert_eq!(json_len(&e.state("selftest-parallel.open.json")), 0);
    assert_eq!(read_trim(&e.state("selftest-parallel.count")), "2");
    golden("round2_pass", &e.mask(&out));
    golden(
        "prompt_lens_c",
        &e.mask(&fs::read_to_string(e.fake.join("last-prompt-C.txt")).unwrap()),
    );
    // レビュー log(render_log)と backlog の本文
    let logs: Vec<PathBuf> = fs::read_dir(&e.review)
        .unwrap()
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    // 同じ秒に 2 ラウンド走ると `<ts>-<sid8>.md` が同名になり上書きされる
    // (bash 版から引き継いだ挙動)ので、件数は 1 か 2。
    assert!(!logs.is_empty() && logs.len() <= 2);
}

#[test]
fn round2_carryover_unresolved_denies() {
    let e = Env::new();
    e.seed("selftest-r2", "1", &open1());
    e.write_fake(
        "C.json",
        &critic_data(
            json!([finding("MINOR", "小さい話")]),
            json!([{"id": "R1-A-1", "status": "UNRESOLVED", "rationale": "まだ"}]),
        )
        .to_string(),
    );
    let out = run(&e, "selftest-r2", &[]);
    assert_eq!(decision(&out), "deny");
    golden("round2_deny", &e.mask(&out));
    golden(
        "backlog_r2",
        &e.mask(&fs::read_to_string(e.review.join("backlog/selftest-r2.md")).unwrap()),
    );
    let log = fs::read_dir(&e.review)
        .unwrap()
        .flatten()
        .map(|d| d.path())
        .find(|p| p.extension().is_some_and(|x| x == "md"))
        .unwrap();
    golden("log_r2", &e.mask(&fs::read_to_string(log).unwrap()));
}

#[test]
fn closer_pass() {
    let e = Env::new();
    e.write_fake(
        "Z.json",
        &critic_data(
            json!([finding("MAJOR", "closer の新規 MAJOR")]),
            json!([{"id": "R1-A-1", "status": "RESOLVED", "rationale": "改訂で解消"}]),
        )
        .to_string(),
    );
    e.seed("selftest-closer-pass", "2", &open1());
    let out = run(&e, "selftest-closer-pass", &[]);
    assert_eq!(decision(&out), "none");
    assert_eq!(json_len(&e.state("selftest-closer-pass.open.json")), 0);
    assert_eq!(read_trim(&e.state("selftest-closer-pass.count")), "3");
    assert!(!e.state("selftest-closer-pass.escalated").exists());
    golden("closer_pass", &e.mask(&out));
    golden(
        "prompt_lens_z",
        &e.mask(&fs::read_to_string(e.fake.join("last-prompt-Z.txt")).unwrap()),
    );
}

#[test]
fn closer_deny_escalates() {
    let e = Env::new();
    e.write_fake(
        "Z.json",
        &critic_data(
            json!([finding("BLOCKER", "closer が見つけた新規")]),
            json!([{"id": "R1-A-1", "status": "UNRESOLVED", "rationale": "まだ直っていない"}]),
        )
        .to_string(),
    );
    e.seed("selftest-closer-deny", "2", &open1());
    let out = run(&e, "selftest-closer-deny", &[]);
    assert_eq!(decision(&out), "deny");
    let r = reason(&out);
    assert!(r.contains("AskUserQuestion") && r.contains("NO-GO"));
    assert!(!r.contains("再度 ExitPlanMode を呼んでください"));
    assert!(e.state("selftest-closer-deny.escalated").exists());
    let open: Value = serde_json::from_str(
        &fs::read_to_string(e.state("selftest-closer-deny.open.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(open.as_array().unwrap().len(), 1);
    assert_eq!(open[0]["id"], "R1-A-1");
    assert!(
        fs::read_to_string(e.review.join("backlog/selftest-closer-deny.md"))
            .unwrap()
            .contains("closer が見つけた新規")
    );
    golden("closer_deny", &e.mask(&out));
}

#[test]
fn escalated_under_cap_passes() {
    let e = Env::new();
    e.seed("selftest-escalated", "1", &open1());
    fs::write(e.state("selftest-escalated.escalated"), "").unwrap();
    let out = run(&e, "selftest-escalated", &[]);
    assert_eq!(decision(&out), "none");
    assert_eq!(read_trim(&e.state("selftest-escalated.count")), "1");
}

#[test]
fn max1_is_not_closer() {
    let e = Env::new();
    e.write_fake(
        "M.json",
        &critic_data(json!([finding("BLOCKER", "lens M の指摘")]), json!([])).to_string(),
    );
    let out = run(
        &e,
        "selftest-max1",
        &[
            ("MAX_PLAN_REVIEWS", "1"),
            ("COPILOT_PLAN_REVIEW_PARALLEL", "0"),
        ],
    );
    assert_eq!(decision(&out), "deny");
    assert_eq!(json_len(&e.state("selftest-max1.open.json")), 1);
    assert!(!e.state("selftest-max1.escalated").exists());
    golden("max1_deny", &e.mask(&out));
}

#[test]
fn rcfail_and_bothfail_do_not_consume() {
    let e = Env::new();
    e.write_fake(
        "A.json",
        &critic_data(json!([finding("BLOCKER", "lens A の指摘")]), json!([])).to_string(),
    );
    e.touch_fake("B.fail");
    e.touch_fake("A.rcfail");
    let out = run(&e, "selftest-rcfail", &[]);
    assert_eq!(decision(&out), "none");
    assert!(!e.state("selftest-rcfail.count").exists());
    e.touch_fake("A.fail");
    let out = run(&e, "selftest-bothfail", &[]);
    assert_eq!(decision(&out), "none");
    assert!(!e.state("selftest-bothfail.count").exists());
}

// ---------------------------------------------------------------------------
// copilot 呼び出し契約
// ---------------------------------------------------------------------------

fn invocation_line(e: &Env, n: usize) -> String {
    fs::read_to_string(e.fake.join("last-invocation-M.txt"))
        .unwrap_or_default()
        .lines()
        .nth(n)
        .unwrap_or_default()
        .to_string()
}

#[test]
fn copilot_contract() {
    let e = Env::new();
    e.write_fake(
        "M.json",
        &critic_data(json!([finding("BLOCKER", "lens M の指摘")]), json!([])).to_string(),
    );
    run(
        &e,
        "selftest-contract",
        &[("COPILOT_PLAN_REVIEW_PARALLEL", "0")],
    );
    assert_eq!(invocation_line(&e, 0), "agent=plan-reviewer");
    assert_eq!(invocation_line(&e, 1), "model=gpt-6-astra");
    assert_eq!(invocation_line(&e, 2), "silent=1 custom=1 mcps=1 askuser=1");
    // plan file(REVIEW_DIR 配下)は workdir の外なので親ディレクトリだけ --add-dir
    assert!(invocation_line(&e, 3).starts_with(&format!("add_dirs={}", e.review.display())));

    run(
        &e,
        "selftest-model-override",
        &[
            ("COPILOT_PLAN_REVIEW_PARALLEL", "0"),
            ("COPILOT_PLAN_REVIEW_MODEL", "gpt-9-test"),
            ("FAKE_COPILOT_EXPECT_MODEL", "gpt-9-test"),
        ],
    );
    assert_eq!(invocation_line(&e, 1), "model=gpt-9-test");

    run(
        &e,
        "selftest-agent-override",
        &[
            ("COPILOT_PLAN_REVIEW_PARALLEL", "0"),
            ("COPILOT_PLAN_REVIEW_AGENT", "custom-reviewer"),
            ("FAKE_COPILOT_EXPECT_AGENT", "custom-reviewer"),
        ],
    );
    assert_eq!(invocation_line(&e, 0), "agent=custom-reviewer");
}

#[test]
fn plan_file_inside_workdir_has_no_add_dir() {
    let e = Env::new();
    e.write_fake(
        "M.json",
        &critic_data(json!([finding("BLOCKER", "lens M の指摘")]), json!([])).to_string(),
    );
    let plan = e.work.join("plan.md");
    fs::write(&plan, "# plan\n").unwrap();
    let env = cp(&e, &[("COPILOT_PLAN_REVIEW_PARALLEL", "0")]);
    let refs: Vec<(&str, &str)> = env.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    e.hook(
        &json!({"session_id": "s-in", "cwd": e.work, "hook_event_name": "PreToolUse",
                "tool_input": {"planFilePath": plan}}),
        &refs,
    );
    assert_eq!(invocation_line(&e, 3), "add_dirs=");
    let p = fs::read_to_string(e.fake.join("last-prompt-M.txt")).unwrap();
    assert!(p.contains(&format!("プラン本文: {} を読むこと。", plan.display())));
}

// ---------------------------------------------------------------------------
// critic の JSON 厳格検証
// ---------------------------------------------------------------------------

#[test]
fn fenced_json_fails_open() {
    let e = Env::new();
    e.write_fake(
        "M.json",
        &critic_data(json!([finding("BLOCKER", "lens M の指摘")]), json!([])).to_string(),
    );
    e.touch_fake("M.fence");
    let out = run(
        &e,
        "selftest-fence",
        &[("COPILOT_PLAN_REVIEW_PARALLEL", "0")],
    );
    assert_eq!(decision(&out), "none");
    assert!(!e.state("selftest-fence.count").exists());
    e.rm_fake("M.fence");
}

#[test]
fn schema_violations_fail_open() {
    let base = critic_data(json!([finding("BLOCKER", "lens M の指摘")]), json!([]));
    let mut cases: Vec<(&str, Value)> = Vec::new();
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove("carryover");
    cases.push(("selftest-missingkey", v));
    let mut v = base.clone();
    v["extra"] = json!("unexpected");
    cases.push(("selftest-extrakey", v));
    let mut v = base.clone();
    v["findings"][0]["severity"] = json!("CRITICAL");
    cases.push(("selftest-badenum", v));
    let mut v = base.clone();
    v["readiness"]["requirements"] = json!("true");
    cases.push(("selftest-badtype", v));
    let mut v = base;
    v["findings"][0]["extra"] = json!("nope");
    cases.push(("selftest-findingextrakey", v));
    let e = Env::new();
    for (sid, data) in cases {
        e.write_fake("M.json", &data.to_string());
        let out = run(&e, sid, &[("COPILOT_PLAN_REVIEW_PARALLEL", "0")]);
        assert_eq!(decision(&out), "none", "{sid}");
        assert!(!e.state(&format!("{sid}.count")).exists(), "{sid}");
    }
}

// ---------------------------------------------------------------------------
// advisory
// ---------------------------------------------------------------------------

fn advisory(e: &Env, args: &[&str], extra: &[(&str, &str)]) -> (i32, String, String) {
    let mut c = e.command(extra);
    c.arg("--advisory").args(args).stdin(Stdio::null());
    let out = c.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn advisory_renders_full_log() {
    let e = Env::new();
    let plan = e.work.join("plan.md");
    fs::write(&plan, "# plan\n").unwrap();
    let mut empty_ev = finding("BLOCKER", "根拠なし");
    empty_ev["evidence"] = json!("  ");
    let mut needs = finding("MAJOR", "人間の判断が要る");
    needs["kind"] = json!("NEEDS_DECISION");
    needs["readiness_axis"] = json!("SCOPE");
    let data = json!({"readiness": {"requirements": false, "scope": true,
                                    "implementation": true, "verification": false},
        "findings": [finding("BLOCKER", "Dup  Summary"), finding("BLOCKER", "dup summary"),
                     needs, finding("MINOR", "命名"), finding("NIT", "句点"),
                     empty_ev],
        "carryover": [{"id": "R9-Z-9", "status": "RESOLVED", "rationale": "x"}]});
    e.write_fake("M.json", &data.to_string());
    let copilot = e.copilot();
    let plan_s = plan.to_string_lossy().into_owned();
    let work_s = e.work.to_string_lossy().into_owned();
    let (rc, out, err) = advisory(
        &e,
        &[&plan_s, &work_s],
        &[
            ("COPILOT_BIN", &copilot),
            ("COPILOT_PLAN_REVIEW_PARALLEL", "0"),
        ],
    );
    assert_eq!(rc, 0);
    golden("advisory_log", &e.mask(&out));
    golden("advisory_stderr", &e.mask(&err));
}

#[test]
fn advisory_parallel_with_failed_lens() {
    let e = Env::new();
    let plan = e.work.join("plan.md");
    fs::write(&plan, "# plan\n").unwrap();
    e.write_fake("A.json", &critic_data(json!([]), json!([])).to_string());
    e.touch_fake("B.fail");
    let copilot = e.copilot();
    let plan_s = plan.to_string_lossy().into_owned();
    let (rc, out, err) = advisory(&e, &[&plan_s], &[("COPILOT_BIN", &copilot)]);
    assert_eq!(rc, 0);
    golden("advisory_parallel_log", &e.mask(&out));
    golden("advisory_parallel_stderr", &e.mask(&err));
}

#[test]
fn advisory_errors() {
    let e = Env::new();
    let (rc, _, err) = advisory(
        &e,
        &["/nonexistent.md"],
        &[("COPILOT_BIN", "definitely-not-a-real-binary")],
    );
    assert_eq!(rc, 1);
    assert_eq!(
        err,
        "copilot が見つかりません (COPILOT_BIN=definitely-not-a-real-binary)。\n"
    );
    let (rc, _, err) = advisory(&e, &["/nonexistent.md"], &[("COPILOT_BIN", "true")]);
    assert_eq!(rc, 1);
    assert_eq!(err, "copilot レビューの実行に失敗しました（タイムアウト・未ログイン・モデル利用不可・ネットワーク等）。\n");
    let (rc, _, _) = advisory(&e, &[], &[("COPILOT_BIN", "true")]);
    assert_ne!(rc, 0);
}
