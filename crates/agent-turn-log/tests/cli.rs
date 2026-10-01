//! agent-turn-log の出力契約(ADR-0011)を、出力ファイルのパスと内容の
//! バイト一致で固定する characterization test(#413)。
//!
//! 下流(別リポジトリの daily-report)は
//! `${XDG_STATE_HOME:-$HOME/.local/state}/daily-report/agent-events.jsonl`
//! のパスと、各行のフィールド名・順序・値に依存する。ここでは実行のたびに
//! 変わる 2 値だけを正規化してから完全一致で比べる:
//!
//! - `"ts":"YYYY-MM-DDTHH:MM:SSZ"` → `"ts":"<TS>"`(形式は検査し、実行前後の
//!   UTC 時刻の範囲に入ることも確かめる)
//! - payload に `prompt_id` / `turn_id` が無いときの代替 ID
//!   `"<数字>-<数字>"` → `"<FALLBACK-ID>"`
//!
//! テスト対象は `AGENT_TURN_LOG_ORACLE`(bash 版のパス)で差し替えられる。
//! 未設定なら cargo がビルドした Rust 版を使う。

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    match std::env::var_os("AGENT_TURN_LOG_ORACLE") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(env!("CARGO_BIN_EXE_agent-turn-log")),
    }
}

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let state = tmp.path().join("state");
        fs::create_dir_all(&home).unwrap();
        Env {
            _tmp: tmp,
            home,
            state,
        }
    }

    fn out_file(&self) -> PathBuf {
        self.state.join("daily-report/agent-events.jsonl")
    }

    /// 既定の環境(HOME / XDG_STATE_HOME を一時ディレクトリへ、AGENT_* は消す)
    /// に `extra` を足して 1 回実行する。
    fn run(&self, stdin: &[u8], extra: &[(&str, &str)]) {
        let mut cmd = Command::new(bin());
        cmd.env("HOME", &self.home)
            .env("XDG_STATE_HOME", &self.state)
            .env_remove("AGENT_NAME")
            .env_remove("AGENT_TURN_LOG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "常に exit 0(fail-open)");
        assert!(out.stdout.is_empty(), "stdout には何も出さない");
        assert!(out.stderr.is_empty(), "stderr には何も出さない");
    }

    fn lines(&self) -> String {
        normalize(&fs::read_to_string(self.out_file()).unwrap())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// `YYYY-MM-DDTHH:MM:SSZ` を epoch 秒に(テスト用の素朴な換算)。
fn parse_ts(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s[r].parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // days_from_civil(Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + h * 3600 + mi * 60 + se) as u64)
}

/// 非決定値 2 つを正規化する。ts は形式と時刻範囲(前後 120 秒)も検査する。
fn normalize(s: &str) -> String {
    let mut out = String::new();
    for line in s.split_inclusive('\n') {
        let mut l = line.to_string();
        if let Some(i) = l.find("\"ts\":\"") {
            let start = i + 6;
            let ts = &l[start..start + 20];
            let t = parse_ts(ts).unwrap_or_else(|| panic!("ts の形式が違う: {ts}"));
            let now = now_secs();
            assert!(
                t + 120 >= now && t <= now + 120,
                "ts が現在時刻から遠い: {ts}"
            );
            l.replace_range(start..start + 20, "<TS>");
        }
        if let Some(i) = l.find("\"prompt_id\":\"") {
            let start = i + 13;
            let end = start + l[start..].find('"').unwrap();
            let id = &l[start..end];
            if let Some((a, b)) = id.split_once('-') {
                if !a.is_empty()
                    && !b.is_empty()
                    && a.bytes().all(|c| c.is_ascii_digit())
                    && b.bytes().all(|c| c.is_ascii_digit())
                {
                    l.replace_range(start..end, "<FALLBACK-ID>");
                }
            }
        }
        out.push_str(&l);
    }
    out
}

const CLAUDE_PROMPT: &str = r#"{"session_id":"s-1","transcript_path":"/x.jsonl","cwd":"/work/repo","permission_mode":"default","hook_event_name":"UserPromptSubmit","prompt_id":"p-1","prompt":"line1\nline2 \"quoted\" \\ back\ttab 日本語 \u0001 \u007f"}"#;

#[test]
fn claude_prompt_and_stop() {
    let e = Env::new();
    e.run(CLAUDE_PROMPT.as_bytes(), &[]);
    e.run(
        br#"{"session_id":"s-1","cwd":"/work/repo","hook_event_name":"Stop","stop_hook_active":false}"#,
        &[],
    );
    assert_eq!(
        e.lines(),
        concat!(
            r#"{"kind":"prompt","agent":"claude-code","ts":"<TS>","session_id":"s-1","prompt_id":"p-1","cwd":"/work/repo","prompt":"line1\nline2 \"quoted\" \\ back\ttab 日本語 \u0001 \u007f"}"#,
            "\n",
            r#"{"kind":"turn_end","agent":"claude-code","ts":"<TS>","session_id":"s-1"}"#,
            "\n",
        )
    );
}

#[test]
fn output_path_and_permissions() {
    let e = Env::new();
    e.run(CLAUDE_PROMPT.as_bytes(), &[]);
    let f = e.out_file();
    assert!(f.is_file());
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&f), 0o600);
    assert_eq!(mode(f.parent().unwrap()), 0o700);
}

#[test]
fn existing_file_permissions_are_tightened() {
    let e = Env::new();
    let f = e.out_file();
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(&f, "old\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(f.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
    e.run(br#"{"session_id":"s","hook_event_name":"Stop"}"#, &[]);
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&f), 0o600);
    assert_eq!(mode(f.parent().unwrap()), 0o700);
    assert_eq!(
        e.lines(),
        "old\n{\"kind\":\"turn_end\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"s\"}\n"
    );
}

#[test]
fn xdg_state_home_empty_falls_back_to_home() {
    let e = Env::new();
    e.run(
        br#"{"session_id":"s","hook_event_name":"Stop"}"#,
        &[("XDG_STATE_HOME", "")],
    );
    let f = e.home.join(".local/state/daily-report/agent-events.jsonl");
    assert_eq!(
        normalize(&fs::read_to_string(f).unwrap()),
        "{\"kind\":\"turn_end\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"s\"}\n"
    );
    assert!(!e.out_file().exists());
}

#[test]
fn codex_uses_turn_id() {
    let e = Env::new();
    // Codex の UserPromptSubmit は prompt_id ではなく turn_id を持つ
    e.run(
        br#"{"session_id":"c-1","turn_id":"t-9","transcript_path":null,"cwd":"/w","hook_event_name":"UserPromptSubmit","model":"gpt","permission_mode":"default","prompt":"hi"}"#,
        &[("AGENT_NAME", "codex")],
    );
    e.run(
        br#"{"session_id":"c-1","turn_id":"t-9","cwd":"/w","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"done"}"#,
        &[("AGENT_NAME", "codex")],
    );
    assert_eq!(
        e.lines(),
        concat!(
            r#"{"kind":"prompt","agent":"codex","ts":"<TS>","session_id":"c-1","prompt_id":"t-9","cwd":"/w","prompt":"hi"}"#,
            "\n",
            r#"{"kind":"turn_end","agent":"codex","ts":"<TS>","session_id":"c-1"}"#,
            "\n",
        )
    );
}

#[test]
fn prompt_id_wins_over_turn_id() {
    let e = Env::new();
    e.run(
        br#"{"session_id":"s","hook_event_name":"UserPromptSubmit","prompt_id":"p","turn_id":"t","prompt":"x"}"#,
        &[],
    );
    assert_eq!(
        e.lines(),
        "{\"kind\":\"prompt\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"s\",\"prompt_id\":\"p\",\"cwd\":\"\",\"prompt\":\"x\"}\n"
    );
}

#[test]
fn copilot_snake_case_payload_with_fallback_id() {
    let e = Env::new();
    // PascalCase のイベント名で登録した Copilot は Claude 互換の snake_case
    // payload を送る。prompt_id / turn_id は無い → 代替 ID
    e.run(
        br#"{"session_id":"cp-1","cwd":"/w","hook_event_name":"UserPromptSubmit","prompt":"copilot prompt"}"#,
        &[("AGENT_NAME", "copilot")],
    );
    e.run(
        br#"{"session_id":"cp-1","cwd":"/w","hook_event_name":"Stop"}"#,
        &[("AGENT_NAME", "copilot")],
    );
    assert_eq!(
        e.lines(),
        concat!(
            r#"{"kind":"prompt","agent":"copilot","ts":"<TS>","session_id":"cp-1","prompt_id":"<FALLBACK-ID>","cwd":"/w","prompt":"copilot prompt"}"#,
            "\n",
            r#"{"kind":"turn_end","agent":"copilot","ts":"<TS>","session_id":"cp-1"}"#,
            "\n",
        )
    );
}

#[test]
fn copilot_camel_case_payload_is_ignored() {
    let e = Env::new();
    // camelCase のイベント名で登録すると hook_event_name の無い payload になり、
    // 何も書かない(home/modules/claude.nix のコメントどおり)
    e.run(
        br#"{"timestamp":1,"cwd":"/w","sessionId":"cp","prompt":"x"}"#,
        &[("AGENT_NAME", "copilot")],
    );
    assert!(!e.out_file().exists());
    assert!(
        !e.state.exists(),
        "対象外イベントでは出力ディレクトリも作らない"
    );
}

#[test]
fn empty_agent_name_defaults_to_claude_code() {
    let e = Env::new();
    e.run(
        br#"{"session_id":"s","hook_event_name":"Stop"}"#,
        &[("AGENT_NAME", "")],
    );
    assert_eq!(
        e.lines(),
        "{\"kind\":\"turn_end\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"s\"}\n"
    );
}

#[test]
fn missing_fields_become_empty_strings() {
    let e = Env::new();
    e.run(
        br#"{"hook_event_name":"UserPromptSubmit","prompt_id":"p"}"#,
        &[],
    );
    e.run(br#"{"hook_event_name":"Stop"}"#, &[]);
    assert_eq!(
        e.lines(),
        concat!(
            r#"{"kind":"prompt","agent":"claude-code","ts":"<TS>","session_id":"","prompt_id":"p","cwd":"","prompt":""}"#,
            "\n",
            r#"{"kind":"turn_end","agent":"claude-code","ts":"<TS>","session_id":""}"#,
            "\n",
        )
    );
}

#[test]
fn null_values_become_empty_strings() {
    let e = Env::new();
    e.run(
        br#"{"hook_event_name":"UserPromptSubmit","session_id":null,"prompt_id":"p","cwd":null,"prompt":null}"#,
        &[],
    );
    assert_eq!(
        e.lines(),
        "{\"kind\":\"prompt\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"\",\"prompt_id\":\"p\",\"cwd\":\"\",\"prompt\":\"\"}\n"
    );
}

#[test]
fn non_string_scalars_are_stringified_and_trailing_newlines_dropped() {
    let e = Env::new();
    // jq -r の出力を $(...) で受けるため、数値は文字列化され、末尾の改行は
    // 落ちる(先頭・途中の改行は残る)
    e.run(
        br#"{"hook_event_name":"UserPromptSubmit","session_id":123,"prompt_id":"p\n","cwd":"\na\nb\n\n","prompt":"keep\n"}"#,
        &[],
    );
    assert_eq!(
        e.lines(),
        "{\"kind\":\"prompt\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"123\",\"prompt_id\":\"p\",\"cwd\":\"\\na\\nb\",\"prompt\":\"keep\\n\"}\n"
    );
}

#[test]
fn ignored_inputs_write_nothing() {
    for (stdin, extra) in [
        (
            &br#"{"hook_event_name":"SessionStart","session_id":"s"}"#[..],
            &[][..],
        ),
        (&br#"{"session_id":"s"}"#[..], &[][..]),
        (&b"not json"[..], &[][..]),
        (&b""[..], &[][..]),
        (
            &br#"{"hook_event_name":"Stop","session_id":"s"}"#[..],
            &[("AGENT_TURN_LOG", "0")][..],
        ),
    ] {
        let e = Env::new();
        e.run(stdin, extra);
        assert!(!e.state.exists(), "{:?}", String::from_utf8_lossy(stdin));
    }
}

#[test]
fn agent_turn_log_other_than_zero_still_logs() {
    let e = Env::new();
    e.run(
        br#"{"hook_event_name":"Stop","session_id":"s"}"#,
        &[("AGENT_TURN_LOG", "1")],
    );
    assert!(e.out_file().exists());
}

#[test]
fn invalid_utf8_is_replaced() {
    let e = Env::new();
    e.run(
        b"{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"s\",\"prompt_id\":\"p\",\"prompt\":\"a\xffb\"}",
        &[],
    );
    assert_eq!(
        e.lines(),
        "{\"kind\":\"prompt\",\"agent\":\"claude-code\",\"ts\":\"<TS>\",\"session_id\":\"s\",\"prompt_id\":\"p\",\"cwd\":\"\",\"prompt\":\"a\u{fffd}b\"}\n"
    );
}
