//! claude-statusline の統合テスト(リポ名キャッシュ・bash の printf 互換・
//! Herdr への報告)。純粋な stdin→stdout のケースは `tests/cases/claude-statusline/`
//! (trycmd)にある。
//!
//! 対象バイナリ: `CLAUDE_STATUSLINE_ORACLE`(bash スクリプトのパス、`bash` で起動 —
//! 配備側の statusLine command と同じ)を最優先にし、未設定なら既定の実装を使う。

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

fn target() -> Command {
    if let Some(o) = std::env::var_os("CLAUDE_STATUSLINE_ORACLE") {
        let mut c = Command::new("bash");
        c.arg(o);
        return c;
    }
    let mut c = Command::new("bash");
    c.arg(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/claude/statusline/claude-statusline.sh"),
    );
    c
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "claude-statusline-test-{}-{}-{tag}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(stdin: &str, envs: &[(&str, &str)]) -> Output {
    let mut c = target();
    c.env_remove("HERDR_ENV")
        .env_remove("HERDR_SOCKET_PATH")
        .env_remove("HERDR_PANE_ID")
        .env("COLUMNS", "80");
    for (k, v) in envs {
        c.env(k, v);
    }
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

const PEACH: &str = "\x1b[1;38;2;250;179;135m";
const PINK: &str = "\x1b[1;38;2;245;194;231m";
const TEAL: &str = "\x1b[38;2;148;226;213m";
const RESET: &str = "\x1b[0m";
const SEP: &str = " \x1b[38;2;108;112;134m·\x1b[0m ";

fn payload_with_dir(dir: &Path) -> String {
    format!(
        r#"{{"model":{{"display_name":"M"}},"workspace":{{"project_dir":"{}"}}}}"#,
        dir.display()
    )
}

#[test]
fn repo_name_from_git_common_dir_and_cached() {
    let t = TempDir::new("repo");
    let repo = t.path().join("myrepo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let xdg = t.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    let o = run(
        &payload_with_dir(&repo),
        &[("XDG_RUNTIME_DIR", xdg.to_str().unwrap())],
    );
    assert_eq!(
        stdout(&o),
        format!("■ {PEACH}myrepo{RESET}{SEP}◆ {PINK}M{RESET}\n")
    );
    let sanitized: String = repo
        .to_str()
        .unwrap()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
                b as char
            } else {
                '_'
            }
        })
        .collect();
    let cache = xdg.join(format!("claude-statusline-repo.{sanitized}"));
    assert_eq!(std::fs::read_to_string(&cache).unwrap(), "myrepo");
}

#[test]
fn repo_name_in_worktree_is_the_main_checkout() {
    let t = TempDir::new("wt");
    let repo = t.path().join("mainrepo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let wt = t.path().join("worktree-xyz");
    git(&repo, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let xdg = t.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    let o = run(
        &payload_with_dir(&wt),
        &[("XDG_RUNTIME_DIR", xdg.to_str().unwrap())],
    );
    assert_eq!(
        stdout(&o),
        format!("■ {PEACH}mainrepo{RESET}{SEP}◆ {PINK}M{RESET}\n")
    );
}

#[test]
fn cached_repo_name_wins_over_git() {
    let t = TempDir::new("cache");
    let xdg = t.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    // 実在しない project_dir でもキャッシュがあればそれを出す。
    std::fs::write(xdg.join("claude-statusline-repo._nope_x"), "cached\n").unwrap();
    let o = run(
        r#"{"model":{"display_name":"M"},"workspace":{"project_dir":"/nope/x"}}"#,
        &[("XDG_RUNTIME_DIR", xdg.to_str().unwrap())],
    );
    assert_eq!(
        stdout(&o),
        format!("■ {PEACH}cached{RESET}{SEP}◆ {PINK}M{RESET}\n")
    );
}

fn cost_line(cost_json: &str) -> String {
    let o = run(
        &format!(r#"{{"model":{{"display_name":"M"}},"cost":{{"total_cost_usd":{cost_json}}}}}"#),
        &[("XDG_RUNTIME_DIR", "/nonexistent/claude-statusline-test")],
    );
    stdout(&o)
}

#[test]
fn cost_rounding_follows_bash_long_double_printf() {
    // bash の printf は long double で %.2f を丸める(dash/double とは結果が違う)。
    for (lit, want) in [
        ("0.045", "$0.05"),
        ("2.345", "$2.35"),
        ("1.005", "$1.00"),
        ("0.125", "$0.12"),
        ("1.2345678", "$1.23"),
        ("12", "$12.00"),
        ("-0.0", "$-0.00"),
    ] {
        assert_eq!(
            cost_line(lit),
            format!("◆ {PINK}M{RESET}{SEP}{TEAL}{want}{RESET}\n"),
            "cost {lit}"
        );
    }
}

#[test]
fn non_numeric_cost_uses_bash_printf_fallback() {
    // printf が数値化に失敗すると、変換できた分(無ければ 0)を出してから
    // `|| printf '%s' "$cost"` が生の値を連結する。
    for (lit, want) in [
        (r#""abc""#, "$0.00abc"),
        (r#""1.5x""#, "$1.501.5x"),
        (r#""3""#, "$3.00"),
        ("true", "$0.00true"),
    ] {
        assert_eq!(
            cost_line(lit),
            format!("◆ {PINK}M{RESET}{SEP}{TEAL}{want}{RESET}\n"),
            "cost {lit}"
        );
    }
}

/// `"id": "..."` と `"seq": N` を固定値に置き換える(実行ごとに変わる部分)。
fn normalise(req: &str) -> String {
    let mut s = req.to_string();
    if let Some(i) = s.find(r#""id": ""#) {
        let start = i + r#""id": ""#.len();
        let end = start + s[start..].find('"').unwrap();
        let id = &s[start..end];
        let parts: Vec<&str> = id.split(':').collect();
        assert_eq!(parts.len(), 3, "id {id}");
        assert_eq!(parts[0], "claude-statusline");
        assert!(parts[1].chars().all(|c| c.is_ascii_digit()));
        assert!(parts[2].len() == 6 && parts[2].chars().all(|c| c.is_ascii_digit()));
        s.replace_range(start..end, "ID");
    }
    if let Some(i) = s.find(r#""seq": "#) {
        let start = i + r#""seq": "#.len();
        let end = start + s[start..].find(|c: char| !c.is_ascii_digit()).unwrap();
        s.replace_range(start..end, "0");
    }
    s
}

/// listener に接続が来れば 1 行読んで応答を返す。`wait` 内に来なければ None。
fn accept_one(listener: &UnixListener, wait: Duration) -> Option<String> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut w = s;
                let _ = w.write_all(b"{\"id\":\"x\",\"result\":{}}\n");
                return Some(line);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("accept: {e}"),
        }
    }
}

fn wait_for_file(p: &Path, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let got = std::fs::read_to_string(p).unwrap_or_default();
        if got == want || Instant::now() > deadline {
            return got;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Herdr {
    _t: TempDir,
    xdg: PathBuf,
    sock: PathBuf,
    listener: UnixListener,
}

impl Herdr {
    fn new(tag: &str) -> Self {
        let t = TempDir::new(tag);
        let xdg = t.path().join("xdg");
        std::fs::create_dir_all(&xdg).unwrap();
        let sock = t.path().join("h.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        Herdr {
            _t: t,
            xdg,
            sock,
            listener,
        }
    }
    fn state(&self) -> PathBuf {
        self.xdg.join("herdr-claude-status.pane_1")
    }
    fn run(&self, stdin: &str) -> Output {
        run(
            stdin,
            &[
                ("XDG_RUNTIME_DIR", self.xdg.to_str().unwrap()),
                ("HERDR_ENV", "1"),
                ("HERDR_SOCKET_PATH", self.sock.to_str().unwrap()),
                ("HERDR_PANE_ID", "pane/1"),
            ],
        )
    }
}

const FULL: &str = r#"{"model":{"display_name":"Fable 5"},"context_window":{"used_percentage":42.4},"cost":{"total_cost_usd":1.5},"effort":{"level":"low"}}"#;
const FULL_FP: &str = r#"{"cost": "$1.50", "ctx": "42%", "effort": "low", "model": "Fable 5"}"#;

#[test]
fn reports_tokens_to_herdr_and_records_fingerprint() {
    let h = Herdr::new("report");
    let o = h.run(FULL);
    assert!(stdout(&o).starts_with("◆ "), "display first");
    let req = accept_one(&h.listener, Duration::from_secs(5)).expect("report sent");
    assert_eq!(
        normalise(&req),
        concat!(
            r#"{"id": "ID", "method": "pane.report_metadata", "params": {"pane_id": "pane/1", "#,
            r#""source": "claude-statusline", "seq": 0, "tokens": {"model": "Fable 5", "ctx": "42%", "#,
            r#""cost": "$1.50", "effort": "low"}, "ttl_ms": 14400000}}"#,
            "\n"
        )
    );
    assert_eq!(wait_for_file(&h.state(), FULL_FP), FULL_FP);
}

#[test]
fn null_tokens_and_non_ascii_are_python_json() {
    let h = Herdr::new("ascii");
    h.run(r#"{"model":{"display_name":"モデル\"x"}}"#);
    let req = accept_one(&h.listener, Duration::from_secs(5)).expect("report sent");
    // python の json.dumps は ensure_ascii=True(非 ASCII は小文字 hex の
    // バックスラッシュ u エスケープ)。
    let bs = '\\';
    let m = format!("{bs}u30e2{bs}u30c7{bs}u30eb{bs}\"x");
    let tokens =
        format!(r#""tokens": {{"model": "{m}", "ctx": null, "cost": null, "effort": null}}"#);
    assert!(req.contains(&tokens), "{req}");
    let fp = format!(r#"{{"cost": null, "ctx": null, "effort": null, "model": "{m}"}}"#);
    assert_eq!(wait_for_file(&h.state(), &fp), fp);
}

fn age(p: &Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn same_fingerprint_is_not_resent() {
    let h = Herdr::new("same");
    std::fs::write(h.state(), FULL_FP).unwrap();
    age(&h.state(), 60);
    h.run(FULL);
    assert!(accept_one(&h.listener, Duration::from_millis(1500)).is_none());
}

#[test]
fn recent_send_throttles_changed_values() {
    let h = Herdr::new("throttle");
    std::fs::write(h.state(), "{}").unwrap();
    h.run(FULL);
    assert!(accept_one(&h.listener, Duration::from_millis(1500)).is_none());
    assert_eq!(std::fs::read_to_string(h.state()).unwrap(), "{}");
}

#[test]
fn stale_changed_values_are_sent() {
    let h = Herdr::new("stale");
    std::fs::write(h.state(), "{}").unwrap();
    age(&h.state(), 60);
    h.run(FULL);
    assert!(accept_one(&h.listener, Duration::from_secs(5)).is_some());
    assert_eq!(wait_for_file(&h.state(), FULL_FP), FULL_FP);
}

#[test]
fn no_listener_leaves_state_untouched() {
    let h = Herdr::new("nolisten");
    drop(std::fs::remove_file(&h.sock));
    let o = h.run(FULL);
    assert_eq!(o.status.code(), Some(0));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!h.state().exists());
}
