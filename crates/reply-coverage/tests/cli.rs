//! CLI の端から端まで。`claude` は環境変数 `REPLY_COVERAGE_CLAUDE_BIN` で偽のスクリプトに差し替える
//! (本物のモデルは呼ばない)。状態はすべて一時ディレクトリに向ける。

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Sandbox {
    dir: tempfile::TempDir,
}

const FAIL_DETECTION: &str = r#"{"asks":[{"id":"1","quote":"来週の火曜日 10 時、または水曜日 15 時はいかがでしょう。","by":"them","kind":"date","status":"unanswered","evidence":"","late":false}],"apology_present":false,"apology_evidence":""}"#;
const PASS_DETECTION: &str = r#"{"asks":[{"id":"1","quote":"来週の火曜日 10 時、または水曜日 15 時はいかがでしょう。","by":"them","kind":"date","status":"answered","evidence":"水曜日 15 時でお願いします","late":false}],"apology_present":false,"apology_evidence":""}"#;

impl Sandbox {
    fn new() -> Self {
        Sandbox {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// 偽の `claude`: 引数と標準入力を控え、`detection` を `structured_output` に入れた封筒を出す。
    fn fake_claude(&self, detection: &str, fail: bool) {
        let envelope =
            format!(r#"{{"is_error":false,"result":"","structured_output":{detection}}}"#);
        std::fs::write(self.path("claude.out"), envelope).unwrap();
        let script = if fail {
            "#!/bin/sh\ncat > /dev/null\necho 'Not logged in' >&2\nexit 1\n".to_string()
        } else {
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > $0.args\ncat > $0.stdin\ncat $0.out\n".to_string()
        };
        let p = self.path("claude");
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_reply-coverage"));
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.dir.path())
            .env("REPLY_COVERAGE_STATE_DIR", self.path("state"))
            .env("REPLY_COVERAGE_CLAUDE_BIN", self.path("claude"));
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().unwrap()
    }

    fn write(&self, rel: &str, body: &str) -> String {
        let p = self.path(rel);
        std::fs::write(&p, body).unwrap();
        p.to_str().unwrap().to_string()
    }
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const THREAD: &str = "# thread: gmail:t1\n## them 2026-11-01T10:00\n30 分ほどお電話できますか。来週の火曜日 10 時、または水曜日 15 時はいかがでしょう。\n";

#[test]
fn import_check_verify_roundtrip_with_a_failing_then_a_passing_draft() {
    let sb = Sandbox::new();
    let thread = sb.write("thread.txt", THREAD);
    let o = sb.run(&[
        "import",
        "--id",
        "gmail:t1",
        "--file",
        &thread,
        "--format",
        "normalized",
    ]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));

    // 1 回目: 日程に答えていない下書き → 不合格(終了コード 1)。報告に未回答の依頼が出る。
    sb.fake_claude(FAIL_DETECTION, false);
    let bad = sb.write(
        "bad.draft",
        "ご連絡ありがとうございます。前向きに検討します。",
    );
    let o = sb.run(&["check", "--thread", "gmail:t1", "--draft", &bad]);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(
        out.contains("★未回答") && out.contains("来週の火曜日 10 時"),
        "{out}"
    );
    assert!(out.contains("判定: 不合格"), "{out}");

    // 検出に渡した引数: --bare を使わず、道具を渡さず、設定・skill・MCP を切っている。
    let args = std::fs::read_to_string(sb.path("claude.args")).unwrap();
    let a: Vec<&str> = args.lines().collect();
    assert!(!a.contains(&"--bare"), "{args}");
    assert!(a.windows(2).any(|w| w == ["--tools", ""]), "{args}");
    assert!(
        a.windows(2).any(|w| w == ["--setting-sources", "project"]),
        "{args}"
    );
    assert!(
        a.contains(&"--disable-slash-commands") && a.contains(&"--strict-mcp-config"),
        "{args}"
    );
    assert!(
        a.contains(&"--json-schema") && a.contains(&"--no-session-persistence"),
        "{args}"
    );
    // 標準入力に、会話の全文と下書きが区切りつきで渡る。
    let stdin = std::fs::read_to_string(sb.path("claude.stdin")).unwrap();
    assert!(
        stdin.contains("<thread>") && stdin.contains("水曜日 15 時はいかがでしょう"),
        "{stdin}"
    );
    assert!(
        stdin.contains("<draft>") && stdin.contains("前向きに検討します"),
        "{stdin}"
    );

    // 不合格の下書きは、verify も通らない(未回答の依頼が文面に出る)。
    let o = sb.run(&["verify", "--thread", "gmail:t1", "--draft", &bad]);
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stderr).contains("来週の火曜日 10 時"),
        "{}",
        text(&o.stderr)
    );

    // 2 回目: 直した下書き → 合格。verify も通る。
    sb.fake_claude(PASS_DETECTION, false);
    let good = sb.write("good.draft", "お電話は水曜日 15 時でお願いします。");
    assert_eq!(
        code(&sb.run(&["check", "--thread", "gmail:t1", "--draft", &good])),
        0
    );
    assert_eq!(
        code(&sb.run(&["verify", "--thread", "gmail:t1", "--draft", &good])),
        0
    );

    // 合格した後に 1 字直した下書きは、通らない(検査していない)。
    let edited = sb.write("edited.draft", "お電話は水曜日 15 時でお願いします!");
    let o = sb.run(&["verify", "--thread", "gmail:t1", "--draft", &edited]);
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stderr).contains("reply-coverage check"),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn verify_without_a_saved_thread_says_to_read_the_conversation_first() {
    let sb = Sandbox::new();
    let d = sb.write("d.draft", "x");
    let o = sb.run(&["verify", "--thread", "gmail:none", "--draft", &d]);
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stderr).contains("最初から読んで"),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn a_backend_failure_is_exit_2_and_leaves_no_record() {
    let sb = Sandbox::new();
    let thread = sb.write("thread.txt", THREAD);
    sb.run(&["import", "--id", "gmail:t1", "--file", &thread]);
    sb.fake_claude("{}", true);
    let d = sb.write("d.draft", "x");
    let o = sb.run(&["check", "--thread", "gmail:t1", "--draft", &d]);
    assert_eq!(code(&o), 2);
    assert!(
        text(&o.stderr).contains("Not logged in"),
        "{}",
        text(&o.stderr)
    );
    // 失敗は「合格」にならない: verify は通らない。
    assert_eq!(
        code(&sb.run(&["verify", "--thread", "gmail:t1", "--draft", &d])),
        1
    );
}

#[test]
fn a_fabricated_pass_from_the_model_is_not_recorded_as_a_pass() {
    let sb = Sandbox::new();
    let thread = sb.write("thread.txt", THREAD);
    sb.run(&["import", "--id", "gmail:t1", "--file", &thread]);
    // モデルが「答えた」と偽る(根拠が下書きに無い)。
    sb.fake_claude(PASS_DETECTION, false);
    let d = sb.write("d.draft", "前向きに検討します。");
    let o = sb.run(&["check", "--thread", "gmail:t1", "--draft", &d]);
    assert_eq!(code(&o), 1, "{}", text(&o.stdout));
    assert!(
        text(&o.stdout).contains("根拠が照合に外れた"),
        "{}",
        text(&o.stdout)
    );
    assert_eq!(
        code(&sb.run(&["verify", "--thread", "gmail:t1", "--draft", &d])),
        1
    );
}

#[test]
fn bench_prints_ids_and_verdicts_but_no_message_text() {
    let sb = Sandbox::new();
    let cases = sb.path("cases");
    let c = cases.join("c1");
    std::fs::create_dir_all(&c).unwrap();
    std::fs::write(c.join("conversation.thread"), THREAD).unwrap();
    std::fs::write(c.join("draft.draft"), "前向きに検討します。").unwrap();
    std::fs::write(
        c.join("expected.json"),
        r#"{"verdict":"fail","unanswered":["来週の火曜日 10 時"]}"#,
    )
    .unwrap();
    sb.fake_claude(FAIL_DETECTION, false);
    let o = sb.run(&[
        "bench",
        "--cases",
        cases.to_str().unwrap(),
        "--variants",
        "m1",
    ]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("c1\tm1\t一致\t1/1"), "{out}");
    assert!(
        !out.contains("お電話") && !out.contains("前向きに"),
        "{out}"
    );
}

#[test]
fn sweep_and_usage_errors() {
    let sb = Sandbox::new();
    assert_eq!(code(&sb.run(&["sweep"])), 0);
    assert_eq!(code(&sb.run(&[])), 2);
    assert_eq!(code(&sb.run(&["nope"])), 2);
    assert_eq!(code(&sb.run(&["check", "--thread", "x"])), 2);
}
