//! plan-view の hook / CLI 経路(旧 `plan-view.sh --selftest` の全ケース、#412)。
//!
//! `PLAN_VIEW_UNDER_TEST` に実行ファイルのパスがあればそれを起動する(移植の段2:
//! bash 版 `config/claude/hooks/plan-view.sh` に向けて緑にするため)。無ければ
//! Rust 版。pandoc が無い環境では HTML 生成系のケースを skip する(bash 版と同じ)。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn under_test() -> PathBuf {
    match std::env::var_os("PLAN_VIEW_UNDER_TEST") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(env!("CARGO_BIN_EXE_plan-view")),
    }
}

fn repo_css() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/claude/assets/plan-view.css")
}

fn have_pandoc() -> bool {
    Command::new("pandoc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn write_exec(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// 1 ケース分の隔離環境(偽ブラウザ・偽 open・VIEW_DIR・HOME)。
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let e = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        // 偽ブラウザ: 渡された引数を記録するだけ。実際に窓は開かない。
        write_exec(
            &e.fake_browser(),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_BROWSER_LOG\"\nexit 0\n",
        );
        fs::create_dir_all(e.path("fake-open-bin")).unwrap();
        write_exec(
            &e.path("fake-open-bin/open"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_OPEN_LOG\"\nexit 0\n",
        );
        fs::create_dir_all(e.path("home/.claude/plans")).unwrap();
        e
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn view_dir(&self) -> PathBuf {
        self.path("views")
    }
    fn fake_browser(&self) -> PathBuf {
        self.path("fake-browser")
    }
    fn browser_log(&self) -> PathBuf {
        self.path("browser.log")
    }
    fn open_log(&self) -> PathBuf {
        self.path("open.log")
    }

    /// 既定の env(DISPLAY あり・偽ブラウザ・リポジトリの CSS)で組んだコマンド。
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(under_test());
        c.args(args).current_dir(self.dir.path());
        for k in [
            "PLAN_VIEW_DIR",
            "PLAN_VIEW_PANDOC",
            "PLAN_VIEW_BROWSER",
            "PLAN_VIEW_CSS",
            "PLAN_VIEW_HIGHLIGHT",
            "PLAN_VIEW_WINDOW_SIZE",
            "PLAN_VIEW_RETENTION_DAYS",
            "PLAN_VIEW_SKIP",
            "PLAN_VIEW_UNAME_OVERRIDE",
            "PLAN_VIEW_DARWIN_CHROME_APP",
            "WAYLAND_DISPLAY",
        ] {
            c.env_remove(k);
        }
        c.env("HOME", self.path("home"))
            .env("PLAN_VIEW_DIR", self.view_dir())
            .env("PLAN_VIEW_BROWSER", self.fake_browser())
            .env("PLAN_VIEW_CSS", repo_css())
            .env("FAKE_BROWSER_LOG", self.browser_log())
            .env("FAKE_OPEN_LOG", self.open_log())
            .env("DISPLAY", ":99");
        c
    }

    fn run(&self, mut c: Command, stdin: &str) -> Output {
        use std::io::Write;
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut sin = child.stdin.take().unwrap();
        let _ = sin.write_all(stdin.as_bytes());
        drop(sin);
        child.wait_with_output().unwrap()
    }

    fn hook_input(&self, sid: &str, plan: Option<&str>, file: Option<&Path>) -> String {
        let mut ti = serde_json::Map::new();
        if let Some(p) = plan {
            ti.insert("plan".into(), p.into());
        }
        if let Some(f) = file {
            ti.insert("planFilePath".into(), f.display().to_string().into());
        }
        serde_json::json!({
            "session_id": sid,
            "cwd": self.dir.path(),
            "hook_event_name": "PreToolUse",
            "tool_name": "ExitPlanMode",
            "permission_mode": "plan",
            "tool_input": ti,
        })
        .to_string()
    }

    fn htmls(&self) -> Vec<PathBuf> {
        let Ok(rd) = fs::read_dir(self.view_dir()) else {
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "html"))
            .collect();
        v.sort();
        v
    }

    /// ブラウザ起動は切り離し(非同期)なので、ログが `n` 行になるまで短く待つ。
    fn wait_log(&self, log: &Path, n: usize) -> String {
        let start = Instant::now();
        loop {
            let s = fs::read_to_string(log).unwrap_or_default();
            if s.lines().count() >= n || start.elapsed() > Duration::from_secs(5) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 呼ばれないことの確認: 少し待ってもログが空のまま。
    fn settled_log(&self, log: &Path) -> String {
        std::thread::sleep(Duration::from_millis(300));
        fs::read_to_string(log).unwrap_or_default()
    }
}

const INLINE_PLAN: &str =
    "# インラインのプラン\n\n本文である。\n\n```bash\nif true; then echo hi; fi\n```\n";

fn line_of(hay: &str, needle: &str) -> Option<usize> {
    hay.lines().position(|l| l.contains(needle)).map(|i| i + 1)
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

// --- CSS ----------------------------------------------------------------

#[test]
fn css_has_no_style_end_tag_and_overrides_skylighting_base() {
    let css = fs::read_to_string(repo_css()).unwrap();
    // style 要素の中では CSS コメントも解釈されず、終了タグの並びで要素が閉じる。
    assert!(!css.to_lowercase().contains("</style"));
    assert!(!css
        .to_lowercase()
        .lines()
        .any(|l| l.contains("</ style") || l.contains("</\tstyle")));
    // skylighting の基底色(`code span` / `div.sourceCode`)をライト側で上書きしている。
    assert!(css
        .lines()
        .any(|l| l.trim_start().starts_with("code span {")));
    assert!(css
        .lines()
        .any(|l| l.trim_start().starts_with("div.sourceCode,")));
}

// --- hook 経路 ----------------------------------------------------------

#[test]
fn hook_inline_plan() {
    if !have_pandoc() {
        eprintln!("skip: pandoc 不在");
        return;
    }
    let e = Env::new();
    let out = e.run(e.cmd(&[]), &e.hook_input("s-plan", Some(INLINE_PLAN), None));
    assert!(out.status.success());
    assert_eq!(out.stdout, b"", "stdout は空");
    let htmls = e.htmls();
    assert_eq!(htmls.len(), 1, "HTML が 1 本できる");
    let log = e.wait_log(&e.browser_log(), 1);
    assert_eq!(log.lines().count(), 1, "ブラウザを 1 回呼ぶ: {log}");
    assert!(log.contains("--app=file://"));
    assert!(log.contains("--window-size=1000,900"));
    assert!(log.contains(&format!("--app=file://{}", htmls[0].display())));
    // 生成物名: <date>-<session 先頭 8 文字>.html
    let name = htmls[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("-s-plan.html"), "{name}");
    assert_eq!(name.len(), "YYYYmmdd-HHMMSS-s-plan.html".len());
    assert_eq!(mode(&htmls[0]), 0o600, "生成物の権限");
    assert_eq!(mode(&e.view_dir()), 0o700, "VIEW_DIR の権限");
    let html = fs::read_to_string(&htmls[0]).unwrap();
    assert!(html.contains("<title>インラインのプラン</title>"));
    assert!(!html.contains("title-block-header"), "h1 が二重にならない");
    assert!(html.contains("plan-view-meta"));
    // メタ行のラベルは cwd の basename
    let label = e.dir.path().file_name().unwrap().to_string_lossy();
    assert!(html.contains(&format!("{label} · ")), "ラベル: {label}");
    // 自前 CSS が skylighting より後ろに入る(順序依存の回帰テスト)
    let hl = line_of(&html, "/* Keyword */").expect("skylighting");
    let own = line_of(&html, "--measure:").expect("own css");
    assert!(own > hl, "hl={hl} own={own}");
    // style 要素が本文の前で閉じている(CSS が本文に漏れていない)
    let last_style_end = html
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("</style>"))
        .map(|(i, _)| i + 1)
        .last()
        .unwrap();
    let body = line_of(&html, "<body").unwrap();
    assert!(last_style_end < body);
    // ライト用のハイライト上書きが載る
    assert!(html.contains("prefers-color-scheme: light"));
    assert!(html.contains("code span.kw"));
    // 一時ファイルは残らない
    let leftovers: Vec<_> = fs::read_dir(e.view_dir())
        .unwrap()
        .filter_map(|x| x.ok())
        .filter(|x| x.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn hook_plan_file_path() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let pf = e.path("from-file.md");
    fs::write(&pf, "# ファイル経由のプラン\n\n本文\n").unwrap();
    let out = e.run(e.cmd(&[]), &e.hook_input("s-file", None, Some(&pf)));
    assert_eq!(out.stdout, b"");
    let htmls = e.htmls();
    assert_eq!(htmls.len(), 1);
    let html = fs::read_to_string(&htmls[0]).unwrap();
    assert!(html.contains("<title>ファイル経由のプラン</title>"));
}

#[test]
fn hook_plan_file_without_h1_uses_basename() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let pf = e.path("lucky-name.md");
    fs::write(&pf, "## h2 だけ\n").unwrap();
    e.run(e.cmd(&[]), &e.hook_input("s-base", None, Some(&pf)));
    let htmls = e.htmls();
    assert_eq!(htmls.len(), 1);
    let html = fs::read_to_string(&htmls[0]).unwrap();
    assert!(html.contains("<title>lucky-name</title>"), "{html}");
}

#[test]
fn hook_latest_plan() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    fs::write(
        e.path("home/.claude/plans/newest.md"),
        "# 最新のプラン\n\n本文\n",
    )
    .unwrap();
    let out = e.run(e.cmd(&[]), &e.hook_input("s-latest", None, None));
    assert_eq!(out.stdout, b"");
    let htmls = e.htmls();
    assert_eq!(htmls.len(), 1);
    assert!(fs::read_to_string(&htmls[0])
        .unwrap()
        .contains("<title>最新のプラン</title>"));
}

#[test]
fn hook_invalid_json_still_renders_latest() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    fs::write(e.path("home/.claude/plans/newest.md"), "# 壊れた入力\n").unwrap();
    let out = e.run(e.cmd(&[]), "{not json");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"");
    let htmls = e.htmls();
    assert_eq!(htmls.len(), 1);
    let name = htmls[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("-unknown.html"), "{name}");
}

#[test]
fn hook_session_id_is_sanitized_and_truncated() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    e.run(
        e.cmd(&[]),
        &e.hook_input("ab/cd:efgh123", Some("# t\n"), None),
    );
    let htmls = e.htmls();
    assert_eq!(htmls.len(), 1);
    let name = htmls[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("-ab_cd_ef.html"), "{name}");
}

#[test]
fn hook_no_source_is_silent() {
    let e = Env::new();
    let out = e.run(e.cmd(&[]), &e.hook_input("s-none", None, None));
    assert!(out.status.success());
    assert_eq!(out.stdout, b"");
    assert!(e.htmls().is_empty());
    assert_eq!(e.settled_log(&e.browser_log()), "");
}

fn assert_silent_noop(e: &Env, out: &Output) {
    assert!(out.status.success());
    assert_eq!(out.stdout, b"", "stdout は空");
    assert!(e.htmls().is_empty(), "HTML を作らない");
    assert_eq!(e.settled_log(&e.browser_log()), "", "ブラウザを呼ばない");
}

#[test]
fn hook_skip_env() {
    let e = Env::new();
    let mut c = e.cmd(&[]);
    c.env("PLAN_VIEW_SKIP", "1");
    let out = e.run(
        c,
        &e.hook_input("s-skip", Some("# skip されるプラン\n"), None),
    );
    assert_silent_noop(&e, &out);
}

#[test]
fn hook_skip_file() {
    let e = Env::new();
    fs::create_dir_all(e.view_dir()).unwrap();
    fs::write(e.view_dir().join("skip"), "").unwrap();
    let out = e.run(e.cmd(&[]), &e.hook_input("s-skip", Some("# x\n"), None));
    assert_silent_noop(&e, &out);
    assert!(e.view_dir().join("skip").exists());
}

#[test]
fn hook_pandoc_missing() {
    let e = Env::new();
    let mut c = e.cmd(&[]);
    c.env("PLAN_VIEW_PANDOC", "definitely-not-a-real-binary");
    let out = e.run(c, &e.hook_input("s", Some("# x\n"), None));
    assert_silent_noop(&e, &out);
}

#[test]
fn hook_browser_missing() {
    let e = Env::new();
    let mut c = e.cmd(&[]);
    c.env("PLAN_VIEW_BROWSER", "definitely-not-a-real-browser");
    let out = e.run(c, &e.hook_input("s", Some("# x\n"), None));
    assert!(out.status.success());
    assert_eq!(out.stdout, b"");
    assert!(e.htmls().is_empty());
}

#[test]
fn hook_no_display() {
    let e = Env::new();
    let mut c = e.cmd(&[]);
    c.env_remove("DISPLAY");
    let out = e.run(c, &e.hook_input("s", Some("# x\n"), None));
    assert_silent_noop(&e, &out);
}

// --- darwin 分岐(#230、PLAN_VIEW_UNAME_OVERRIDE で実機非依存に検査) -------

fn darwin_cmd(e: &Env, app: &Path) -> Command {
    let mut c = e.cmd(&[]);
    let path = format!(
        "{}:{}",
        e.path("fake-open-bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    c.env_remove("DISPLAY")
        .env("PATH", path)
        .env("PLAN_VIEW_UNAME_OVERRIDE", "Darwin")
        .env("PLAN_VIEW_DARWIN_CHROME_APP", app);
    c
}

#[test]
fn darwin_with_chrome_app_opens_without_display() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let app = e.path("DarwinChromePresent.app");
    fs::create_dir_all(&app).unwrap();
    let out = e.run(
        darwin_cmd(&e, &app),
        &e.hook_input("s", Some(INLINE_PLAN), None),
    );
    assert_eq!(out.stdout, b"");
    assert_eq!(e.htmls().len(), 1);
    let log = e.wait_log(&e.open_log(), 1);
    assert!(
        log.contains(&format!("-a {} --args --app=file://", app.display())),
        "{log}"
    );
    assert!(log.contains("--window-size="));
}

#[test]
fn darwin_without_chrome_app_is_silent() {
    let e = Env::new();
    let app = e.path("DarwinChromeAbsent.app");
    let out = e.run(
        darwin_cmd(&e, &app),
        &e.hook_input("s", Some(INLINE_PLAN), None),
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"");
    assert!(e.htmls().is_empty());
    assert_eq!(e.settled_log(&e.open_log()), "");
}

// --- CLI 経路 -----------------------------------------------------------

#[test]
fn cli_no_open_out() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let t1 = e.path("t1.md");
    fs::write(&t1, "# 本当のタイトル\n\n本文\n").unwrap();
    let out_html = e.path("cli.html");
    let out = e.run(
        e.cmd(&[
            "--no-open",
            "--out",
            out_html.to_str().unwrap(),
            t1.to_str().unwrap(),
        ]),
        "",
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", out_html.display())
    );
    assert!(fs::metadata(&out_html).unwrap().len() > 0);
    assert_eq!(
        e.settled_log(&e.browser_log()),
        "",
        "--no-open なら呼ばない"
    );
}

#[test]
fn cli_stdin() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let out_html = e.path("cli-stdin.html");
    let out = e.run(
        e.cmd(&["--no-open", "--out", out_html.to_str().unwrap(), "-"]),
        "# 標準入力のプラン\n\n本文\n",
    );
    assert!(out.status.success(), "{out:?}");
    let html = fs::read_to_string(&out_html).unwrap();
    assert!(html.contains("<title>標準入力のプラン</title>"));
}

#[test]
fn cli_title_overrides_h1() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let t1 = e.path("t1.md");
    fs::write(&t1, "# 本当のタイトル\n").unwrap();
    let out_html = e.path("cli-title.html");
    e.run(
        e.cmd(&[
            "--no-open",
            "--title",
            "明示タイトル",
            "--out",
            out_html.to_str().unwrap(),
            t1.to_str().unwrap(),
        ]),
        "",
    );
    assert!(fs::read_to_string(&out_html)
        .unwrap()
        .contains("<title>明示タイトル</title>"));
}

#[test]
fn cli_opens_browser_and_defaults_out() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let t1 = e.path("t1.md");
    fs::write(&t1, "# t\n").unwrap();
    let out = e.run(e.cmd(&[t1.to_str().unwrap()]), "");
    assert!(out.status.success(), "{out:?}");
    let printed = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    let name = Path::new(&printed).file_name().unwrap().to_string_lossy();
    assert!(
        name.starts_with("cli-") && name.ends_with(".html"),
        "{name}"
    );
    assert!(Path::new(&printed).starts_with(e.view_dir()));
    let log = e.wait_log(&e.browser_log(), 1);
    assert!(log.contains(&format!("--app=file://{printed}")), "{log}");
}

#[test]
fn cli_script_dir_adjacent_css() {
    if !have_pandoc() {
        return;
    }
    // 配備形: ~/.claude/hooks/plan-view(リンク) の隣に plan-view.css。
    let e = Env::new();
    let hooks = e.path("hooks");
    fs::create_dir_all(&hooks).unwrap();
    let link = hooks.join("plan-view");
    std::os::unix::fs::symlink(under_test(), &link).unwrap();
    fs::copy(repo_css(), hooks.join("plan-view.css")).unwrap();
    let t1 = e.path("t1.md");
    fs::write(&t1, "# t\n\n```bash\nls\n```\n").unwrap();
    let out_html = e.path("adj.html");
    let mut c = e.cmd(&[
        "--no-open",
        "--out",
        out_html.to_str().unwrap(),
        t1.to_str().unwrap(),
    ]);
    c.env_remove("PLAN_VIEW_CSS");
    let c = {
        let mut n = Command::new(&link);
        n.args(c.get_args());
        for (k, v) in c.get_envs() {
            match v {
                Some(v) => n.env(k, v),
                None => n.env_remove(k),
            };
        }
        n.current_dir(e.dir.path());
        n
    };
    let out = e.run(c, "");
    assert!(out.status.success(), "{out:?}");
    assert!(fs::read_to_string(&out_html)
        .unwrap()
        .contains("--measure:"));
}

#[test]
fn cli_missing_file_fails() {
    let e = Env::new();
    let out = e.run(e.cmd(&["--no-open", "/definitely/not/a/file.md"]), "");
    if have_pandoc() {
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "plan-view: ファイルがありません: /definitely/not/a/file.md\n"
        );
    } else {
        assert!(!out.status.success());
    }
}

#[test]
fn cli_unknown_flag_fails() {
    let e = Env::new();
    let out = e.run(e.cmd(&["--unknown-flag"]), "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "plan-view: 未知のフラグ: --unknown-flag\n"
    );
}

#[test]
fn cli_argument_errors() {
    let e = Env::new();
    for (args, msg) in [
        (&["--title"][..], "plan-view: --title に値がありません\n"),
        (&["--out"][..], "plan-view: --out に値がありません\n"),
        (
            &["a.md", "b.md"][..],
            "plan-view: ファイルは 1 つだけ指定してください\n",
        ),
        (
            &["a.md", "-"][..],
            "plan-view: ファイルは 1 つだけ指定してください\n",
        ),
    ] {
        let out = e.run(e.cmd(args), "");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), msg, "{args:?}");
    }
    let mut c = e.cmd(&["--no-open", "x.md"]);
    c.env("PLAN_VIEW_PANDOC", "definitely-not-a-real-binary");
    let out = e.run(c, "");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "plan-view: pandoc が見つかりません (PLAN_VIEW_PANDOC=definitely-not-a-real-binary)\n"
    );
}

#[test]
fn cli_open_preconditions() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let t1 = e.path("t1.md");
    fs::write(&t1, "# t\n").unwrap();
    let mut c = e.cmd(&[t1.to_str().unwrap()]);
    c.env("PLAN_VIEW_BROWSER", "definitely-not-a-real-browser");
    let out = e.run(c, "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "plan-view: ブラウザが見つかりません (PLAN_VIEW_BROWSER=definitely-not-a-real-browser)\n"
    );
    let mut c = e.cmd(&[t1.to_str().unwrap()]);
    c.env_remove("DISPLAY");
    let out = e.run(c, "");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "plan-view: DISPLAY / WAYLAND_DISPLAY がありません（--no-open なら HTML だけ作れます）\n"
    );
    let mut c = e.cmd(&[t1.to_str().unwrap()]);
    c.env("PLAN_VIEW_UNAME_OVERRIDE", "Darwin")
        .env("PLAN_VIEW_DARWIN_CHROME_APP", "/nonexistent/Chrome.app");
    let out = e.run(c, "");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "plan-view: Google Chrome.app が見つかりません (/nonexistent/Chrome.app)\n"
    );
    let out = e.run(e.cmd(&["--no-open"]), "");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "plan-view: ~/.claude/plans/ にプランがありません\n"
    );
}

#[test]
fn cli_help() {
    let e = Env::new();
    let out = e.run(e.cmd(&["--help"]), "");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.starts_with("使い方: plan-view [FILE|-] [--title T] [--no-open] [--out PATH]\n\n"));
    assert!(s.ends_with("無効化:   PLAN_VIEW_SKIP=1 または ~/.claude/plan-views/skip\n"));
    assert_eq!(s.lines().count(), 10);
    let out2 = e.run(e.cmd(&["-h"]), "");
    assert_eq!(out.stdout, out2.stdout);
}

// --- ログ衛生 -----------------------------------------------------------

#[test]
fn prune_old_keeps_skip() {
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    fs::create_dir_all(e.view_dir()).unwrap();
    let old = e.view_dir().join("20250101-000000-deadbeef.html");
    let skip = e.view_dir().join("skip");
    let past = std::time::SystemTime::now() - Duration::from_secs(40 * 86400);
    for p in [&old, &skip] {
        fs::write(p, "").unwrap();
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(past)
            .unwrap();
    }
    let t1 = e.path("t1.md");
    fs::write(&t1, "# t\n").unwrap();
    let out_html = e.path("o.html");
    let out = e.run(
        e.cmd(&[
            "--no-open",
            "--out",
            out_html.to_str().unwrap(),
            t1.to_str().unwrap(),
        ]),
        "",
    );
    assert!(out.status.success());
    assert!(!old.exists(), "保持期限より古い生成物が掃除される");
    assert!(skip.exists(), "skip フラグは掃除されない");
}

/// bash 版との生成物比較(段2/段3 の橋渡し)。`PLAN_VIEW_BASH_ORACLE` に bash 版の
/// パスがあるときだけ走る(bash 削除後は自動で何もしない)。メタ行の時刻以外が
/// バイト一致すること。
#[test]
fn same_html_as_bash_oracle() {
    let Some(bash) = std::env::var_os("PLAN_VIEW_BASH_ORACLE").filter(|v| !v.is_empty()) else {
        return;
    };
    if !have_pandoc() {
        return;
    }
    let e = Env::new();
    let src = e.path("p.md");
    fs::write(
        &src,
        "#!/x\n```\n# not\n```\n# 本当 & <タイトル>  \n\n本文\n\n```bash\necho hi\n```\n",
    )
    .unwrap();
    let render = |bin: &Path, out: &Path| {
        let mut c = e.cmd(&[
            "--no-open",
            "--out",
            out.to_str().unwrap(),
            src.to_str().unwrap(),
        ]);
        let mut n = Command::new(bin);
        n.args(c.get_args());
        for (k, v) in c.get_envs() {
            match v {
                Some(v) => n.env(k, v),
                None => n.env_remove(k),
            };
        }
        n.current_dir(e.dir.path());
        let _ = &mut c;
        let o = e.run(n, "");
        assert!(o.status.success(), "{o:?}");
        let html = fs::read_to_string(out).unwrap();
        // メタ行の時刻だけを伏せる
        html.lines()
            .map(|l| {
                if l.contains("plan-view-meta") {
                    l.split(" · ").next().unwrap().to_string()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let a = render(Path::new(&bash), &e.path("bash.html"));
    let b = render(
        Path::new(env!("CARGO_BIN_EXE_plan-view")),
        &e.path("rust.html"),
    );
    assert_eq!(a, b);
}
