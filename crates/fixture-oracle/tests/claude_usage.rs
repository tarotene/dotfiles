//! claude-usage の統合テスト。bash 版 `--selftest` の全ケースを写し、
//! curl の起動形(トークンを argv に載せない)と state file のバイト列を足した。
//!
//! 対象バイナリ: `CLAUDE_USAGE_ORACLE`(bash 版スクリプトのパス、直接 exec —
//! herdr の `/bin/sh -lc` と同じく shebang で起動)を最優先にし、未設定なら
//! 既定の実装を使う。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

fn target() -> Command {
    if let Some(o) = std::env::var_os("CLAUDE_USAGE_ORACLE") {
        return Command::new(o);
    }
    Command::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/claude/statusline/claude-usage.sh"),
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "claude-usage-test-{}-{}-{tag}",
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

const NOW: i64 = 1_700_000_000;

/// epoch → (年, 月, 日, 時, 分, 秒)(UTC)。Hinnant の civil_from_days。
fn civil(epoch: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = epoch.div_euclid(86400);
    let secs = epoch.rem_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, secs / 3600, secs % 3600 / 60, secs % 60)
}

fn iso_at(e: i64) -> String {
    let (y, m, d, h, mi, s) = civil(e);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn hm_at(e: i64) -> String {
    let (_, _, _, h, mi, _) = civil(e);
    format!("{h:02}:{mi:02}")
}

fn md_at(e: i64) -> String {
    let (_, m, d, _, _, _) = civil(e);
    format!("{m}/{d}")
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

/// `$(...)` と同じく末尾改行を落とした stdout。
fn line(o: &Output) -> String {
    out(o).trim_end_matches('\n').to_string()
}

fn render_in(dir: &Path, name: &str, limits: &str, tz: &str) -> (Output, PathBuf) {
    let usage = dir.join(format!("{name}.json"));
    std::fs::write(&usage, format!(r#"{{"limits":{limits}}}"#)).unwrap();
    let state = dir.join(format!("{name}.state.json"));
    let o = target()
        .args(["__render"])
        .arg(&usage)
        .arg(&state)
        .arg(NOW.to_string())
        .env("TZ", tz)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (o, state)
}

fn render(limits: &str) -> String {
    let t = TempDir::new("render");
    let (o, _) = render_in(t.path(), "u", limits, "UTC");
    line(&o)
}

fn session(percent: &str, severity: &str, reset: i64) -> String {
    format!(
        r#"{{"kind":"session","group":"session","percent":{percent},"severity":"{severity}","resets_at":"{}","scope":null}}"#,
        iso_at(reset)
    )
}

#[test]
fn normal_two_limits_with_landing() {
    let rs = NOW + 3600;
    let rw = NOW + 3 * 86400;
    let limits = format!(
        r#"[{},{{"kind":"weekly_scoped","group":"weekly","percent":48,"severity":"normal","resets_at":"{}","scope":{{"model":{{"id":null,"display_name":"Fable"}}}}}}]"#,
        session("21", "normal", rs),
        iso_at(rw)
    );
    assert_eq!(
        render(&limits),
        format!("5h 21%→{} ▼26% · Fable 48%→{} ▼84%", hm_at(rs), md_at(rw))
    );
}

#[test]
fn landing_over() {
    let r = NOW + 13500;
    assert_eq!(
        render(&format!("[{}]", session("30", "normal", r))),
        format!("5h 30%→{} ▲120%", hm_at(r))
    );
}

#[test]
fn landing_under() {
    let r = NOW + 302_400;
    let limits = format!(
        r#"[{{"kind":"weekly_scoped","group":"weekly","percent":38,"severity":"normal","resets_at":"{}","scope":{{"model":{{"id":null,"display_name":"Fable"}}}}}}]"#,
        iso_at(r)
    );
    assert_eq!(render(&limits), format!("Fable 38%→{} ▼76%", md_at(r)));
}

#[test]
fn landing_exactly_100_is_up() {
    let r = NOW + 9000;
    assert_eq!(
        render(&format!("[{}]", session("50", "normal", r))),
        format!("5h 50%→{} ▲100%", hm_at(r))
    );
}

#[test]
fn early_guard_hides_landing() {
    let r = NOW + 17280;
    assert_eq!(
        render(&format!("[{}]", session("5", "normal", r))),
        format!("5h 5%→{}", hm_at(r))
    );
}

#[test]
fn window_start_does_not_break_other_segments() {
    let a = NOW + 18000;
    let b = NOW + 302_400;
    let limits = format!(
        r#"[{},{{"kind":"weekly","group":"weekly","percent":20,"severity":"normal","resets_at":"{}"}}]"#,
        session("5", "normal", a),
        iso_at(b)
    );
    assert_eq!(
        render(&limits),
        format!("5h 5%→{} · wk 20%→{} ▼40%", hm_at(a), md_at(b))
    );
}

#[test]
fn past_reset_has_no_landing() {
    let r = NOW - 100;
    assert_eq!(
        render(&format!("[{}]", session("50", "normal", r))),
        format!("5h 50%→{}", hm_at(r))
    );
}

#[test]
fn exceeded_by_percent() {
    let r = NOW + 3600;
    assert_eq!(
        render(&format!("[{}]", session("100", "normal", r))),
        format!("!5h 100%→{}", hm_at(r))
    );
}

#[test]
fn exceeded_by_severity() {
    let r = NOW + 3600;
    assert_eq!(
        render(&format!("[{}]", session("95", "blocked", r))),
        format!("!5h 95%→{}", hm_at(r))
    );
    // 大文字小文字は区別しない(ascii_downcase)
    assert_eq!(
        render(&format!("[{}]", session("10", "Critical", r))),
        format!("!5h 10%→{}", hm_at(r))
    );
}

#[test]
fn weekly_scoped_label_falls_back_to_wk() {
    let r = NOW + 600_000;
    let limits = format!(
        r#"[{{"kind":"weekly_scoped","group":"weekly","percent":10,"severity":"normal","resets_at":"{}","scope":{{"model":{{"display_name":null}}}}}}]"#,
        iso_at(r)
    );
    assert_eq!(render(&limits), format!("wk 10%→{}", md_at(r)));
}

#[test]
fn unknown_kind_uses_group_label() {
    let r = NOW + 3 * 86400;
    let limits = format!(
        r#"[{{"kind":"seven_day_opus","group":"opus_weekly","percent":33,"severity":"normal","resets_at":"{}"}}]"#,
        iso_at(r)
    );
    assert_eq!(render(&limits), format!("opus_weekly 33%→{}", md_at(r)));
    // group も無ければ kind
    let limits = format!(
        r#"[{{"kind":"seven_day_opus","percent":33,"resets_at":"{}"}}]"#,
        iso_at(r)
    );
    assert_eq!(render(&limits), format!("seven_day_opus 33%→{}", md_at(r)));
}

#[test]
fn entries_missing_percent_are_skipped() {
    let r = NOW + 590_000;
    let limits = format!(
        r#"[{{"kind":"session","group":"session","percent":null,"resets_at":"{}"}},{{"kind":"weekly","group":"weekly","percent":5,"severity":"normal","resets_at":"{}"}}]"#,
        iso_at(r),
        iso_at(r)
    );
    assert_eq!(render(&limits), format!("wk 5%→{}", md_at(r)));
}

#[test]
fn unparsable_reset_is_dropped_and_real_api_format_parses() {
    let r = NOW + 3600;
    let limits = format!(
        r#"[{{"kind":"session","percent":21,"resets_at":"not a date"}},{{"kind":"weekly","percent":5.6,"resets_at":"{}.944701+00:00"}}]"#,
        iso_at(r + 2 * 86400).trim_end_matches('Z')
    );
    assert_eq!(
        render(&limits),
        format!("wk 6%→{} ▼8%", md_at(r + 2 * 86400))
    );
}

#[test]
fn local_timezone_is_honoured() {
    let t = TempDir::new("tz");
    let r = NOW + 3600; // 2023-11-14T23:13:20Z → JST 08:13
    let (o, _) = render_in(
        t.path(),
        "u",
        &format!("[{}]", session("21", "normal", r)),
        "JST-9",
    );
    assert_eq!(line(&o), "5h 21%→08:13 ▼26%");
}

#[test]
fn empty_or_missing_limits_render_nothing() {
    assert_eq!(render("[]"), "");
    let t = TempDir::new("nolimits");
    let usage = t.path().join("u.json");
    std::fs::write(&usage, "{}").unwrap();
    let state = t.path().join("s.json");
    let o = target()
        .arg("__render")
        .arg(&usage)
        .arg(&state)
        .arg(NOW.to_string())
        .output()
        .unwrap();
    assert_eq!(out(&o), "");
    // authoritative empty: state は今回の now と空行で更新される
    assert_eq!(
        std::fs::read_to_string(&state).unwrap(),
        r#"{"last_fetch":1700000000,"last_line":""}"#
    );
}

#[test]
fn invalid_json_keeps_state_untouched() {
    let t = TempDir::new("badjson");
    let usage = t.path().join("u.json");
    std::fs::write(&usage, "{not valid json").unwrap();
    let state = t.path().join("s.json");
    std::fs::write(&state, "SENTINEL").unwrap();
    for body in ["{not valid json", "null", ""] {
        std::fs::write(&usage, body).unwrap();
        let o = target()
            .arg("__render")
            .arg(&usage)
            .arg(&state)
            .arg(NOW.to_string())
            .output()
            .unwrap();
        assert_eq!(out(&o), "", "{body:?}");
        assert_eq!(std::fs::read_to_string(&state).unwrap(), "SENTINEL");
    }
}

#[test]
fn state_file_bytes_and_no_series() {
    let t = TempDir::new("state");
    let r = NOW + 3600;
    let (o, state) = render_in(
        t.path(),
        "u",
        &format!("[{}]", session("21", "normal", r)),
        "UTC",
    );
    assert_eq!(out(&o), format!("5h 21%→{} ▼26%\n", hm_at(r)));
    assert_eq!(
        std::fs::read_to_string(&state).unwrap(),
        format!(
            r#"{{"last_fetch":1700000000,"last_line":"5h 21%→{} ▼26%"}}"#,
            hm_at(r)
        )
    );
    let mode = std::fs::metadata(&state).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

// ---- 通常経路(stub curl) -----------------------------------------------

struct Env {
    t: TempDir,
    token: String,
}

impl Env {
    fn new(tag: &str) -> Self {
        let t = TempDir::new(tag);
        for d in ["bin", "home/.claude", "xdg"] {
            std::fs::create_dir_all(t.path().join(d)).unwrap();
        }
        let token = format!("TESTTOKEN-{}-marker", std::process::id());
        std::fs::write(
            t.path().join("home/.claude/.credentials.json"),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{token}"}}}}"#),
        )
        .unwrap();
        Env { t, token }
    }
    fn dir(&self) -> &Path {
        self.t.path()
    }
    fn state(&self) -> PathBuf {
        self.dir().join("xdg/claude-usage-tabbar.json")
    }
    fn stub(&self, body: &str) {
        let p = self.dir().join("bin/curl");
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn run(&self) -> Output {
        let path = format!(
            "{}:{}",
            self.dir().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        target()
            .env("HOME", self.dir().join("home"))
            .env("XDG_RUNTIME_DIR", self.dir().join("xdg"))
            .env("PATH", path)
            .env("TZ", "UTC")
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
    fn set_last_fetch(&self, t: i64) {
        let s = std::fs::read_to_string(self.state()).unwrap();
        let v: serde_like::State = serde_like::parse(&s);
        std::fs::write(
            self.state(),
            format!(
                r#"{{"last_fetch":{t},"last_line":{}}}"#,
                serde_like::quote(&v.last_line)
            ),
        )
        .unwrap();
    }
    fn last_fetch(&self) -> String {
        serde_like::parse(&std::fs::read_to_string(self.state()).unwrap()).last_fetch
    }
}

/// state file(`{"last_fetch":N,"last_line":"..."}` 固定形)の最小限の読み書き。
/// fixture-oracle は依存を持たないので serde_json を使わない。
mod serde_like {
    pub struct State {
        pub last_fetch: String,
        pub last_line: String,
    }
    pub fn parse(s: &str) -> State {
        let lf = s.split(r#""last_fetch":"#).nth(1).unwrap();
        let last_fetch: String = lf.chars().take_while(|c| c.is_ascii_digit()).collect();
        let ll = s.split(r#""last_line":""#).nth(1).unwrap();
        let ll = ll.strip_suffix("\"}").unwrap();
        State {
            last_fetch,
            last_line: ll.replace("\\\"", "\"").replace("\\\\", "\\"),
        }
    }
    pub fn quote(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn now_real() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn good_stub(e: &Env, fixture: &Path) -> String {
    let d = e.dir().display();
    format!(
        r#"#!/bin/sh
outfile=""
prev=""
: >"{d}/curl-args.log"
for a in "$@"; do
  case "$a" in *Bearer*) exit 9 ;; esac
  if [ "$prev" = "-o" ]; then outfile="$a"; else printf '%s\n' "$a" >>"{d}/curl-args.log"; fi
  prev="$a"
done
cfg="$(cat)"
printf '%s' "$cfg" >"{d}/curl-config.log"
case "$cfg" in
  *"Authorization: Bearer {tok}"*) : ;;
  *) exit 9 ;;
esac
echo call >>"{d}/curl-calls.log"
if [ -n "$outfile" ]; then
  cat "{f}" >"$outfile"
else
  cat "{f}"
fi
"#,
        tok = e.token,
        f = fixture.display()
    )
}

fn stub_writing(e: &Env, body: &str) {
    e.stub(&format!(
        r#"#!/bin/sh
outfile=""
prev=""
for a in "$@"; do
  if [ "$prev" = "-o" ]; then outfile="$a"; fi
  prev="$a"
done
cat >/dev/null
if [ -n "$outfile" ]; then
  printf '%s' '{body}' >"$outfile"
fi
"#
    ));
}

#[test]
fn normal_path_guard_stale_and_authoritative_empty() {
    let e = Env::new("normal");
    let r = now_real() + 3600;
    let fixture = e.dir().join("fixture.json");
    std::fs::write(
        &fixture,
        format!(
            r#"{{"limits":[{{"kind":"session","group":"session","percent":21,"severity":"normal","resets_at":"{}","scope":null}}]}}"#,
            iso_at(r)
        ),
    )
    .unwrap();
    e.stub(&good_stub(&e, &fixture));

    let o1 = e.run();
    assert_eq!(o1.status.code(), Some(0));
    let out1 = line(&o1);
    assert!(out1.starts_with("5h 21%→"), "{out1}");
    assert!(!out(&o1).contains(&e.token), "トークン非漏えい: stdout");
    assert!(
        !std::fs::read_to_string(e.state())
            .unwrap()
            .contains(&e.token),
        "トークン非漏えい: state file"
    );
    assert_eq!(String::from_utf8_lossy(&o1.stderr), "", "stderr 空");
    // curl の起動形: トークンは argv に載せず、--config - で stdin から渡す
    assert_eq!(
        std::fs::read_to_string(e.dir().join("curl-args.log")).unwrap(),
        "-s\n--fail\n--max-time\n5\n--config\n-\n-o\n"
    );
    assert_eq!(
        std::fs::read_to_string(e.dir().join("curl-config.log")).unwrap(),
        format!(
            "url = \"https://api.anthropic.com/api/oauth/usage\"\nheader = \"Authorization: Bearer {}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"",
            e.token
        )
    );

    // 30 秒ガード
    let o2 = e.run();
    assert_eq!(line(&o2), out1);
    assert_eq!(
        std::fs::read_to_string(e.dir().join("curl-calls.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    // stale-if-error: fetch 失敗は TTL 内なら直前の行
    let back = now_real() - 60;
    e.set_last_fetch(back);
    e.stub("#!/bin/sh\ncat >/dev/null\nexit 1\n");
    let of = e.run();
    assert_eq!(line(&of), out1);
    assert_eq!(String::from_utf8_lossy(&of.stderr), "");
    assert_eq!(e.last_fetch(), back.to_string());

    // TTL 超過は空
    e.set_last_fetch(now_real() - 901);
    assert_eq!(out(&e.run()), "");

    // 不正 JSON レスポンスも stale
    e.set_last_fetch(back);
    stub_writing(&e, "{not valid json");
    assert_eq!(line(&e.run()), out1);
    assert_eq!(e.last_fetch(), back.to_string());

    // authoritative empty
    e.set_last_fetch(back);
    stub_writing(&e, r#"{"limits":[]}"#);
    assert_eq!(out(&e.run()), "");
    let s = std::fs::read_to_string(e.state()).unwrap();
    assert!(s.ends_with(r#","last_line":""}"#), "{s}");
}

#[test]
fn missing_credentials_falls_back_to_stale() {
    let e = Env::new("nocred");
    std::fs::remove_file(e.dir().join("home/.claude/.credentials.json")).unwrap();
    e.stub("#!/bin/sh\necho call >>\"$0.calls\"\nexit 0\n");
    let back = now_real() - 60;
    std::fs::write(
        e.state(),
        format!(r#"{{"last_fetch":{back},"last_line":"5h 1%→00:00"}}"#),
    )
    .unwrap();
    assert_eq!(out(&e.run()), "5h 1%→00:00\n");
    assert!(!e.dir().join("bin/curl.calls").exists());
    // トークンが空でも同じ
    std::fs::write(
        e.dir().join("home/.claude/.credentials.json"),
        r#"{"claudeAiOauth":{}}"#,
    )
    .unwrap();
    assert_eq!(out(&e.run()), "5h 1%→00:00\n");
    assert!(!e.dir().join("bin/curl.calls").exists());
}

#[test]
fn guard_ignores_non_numeric_last_fetch() {
    let e = Env::new("nonnum");
    e.stub("#!/bin/sh\ncat >/dev/null\nexit 1\n");
    // last_fetch が数字でなければ 0 扱い = ガードに掛からず fetch → 失敗 → stale も不成立
    std::fs::write(e.state(), r#"{"last_fetch":"x","last_line":"old"}"#).unwrap();
    assert_eq!(out(&e.run()), "");
}
