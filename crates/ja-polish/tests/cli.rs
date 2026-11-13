//! CLI の端から端まで。`llm` は環境変数 `JA_POLISH_LLM_BIN` で偽のスクリプトに差し替える
//! (本物のモデルは呼ばない)。状態・設定・スタイルガイドもすべて一時ディレクトリに向ける。

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("style.md"), "# 規約\nR1. 常体で書く\n").unwrap();
        Sandbox { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// 偽の `llm`: 標準入力を読み捨て、`body` を標準出力に出す(`fail` なら標準エラーに出して失敗)。
    fn fake_llm(&self, body: &str, fail: bool) {
        let script = if fail {
            format!("#!/bin/sh\ncat > /dev/null\necho '{body}' >&2\nexit 1\n")
        } else {
            format!("#!/bin/sh\ncat > $0.stdin\nprintf '%s\\n' '{body}'\n")
        };
        let p = self.path("llm");
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_ja-polish"));
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.dir.path())
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("JA_POLISH_LLM_BIN", self.path("llm"))
            .env("JA_POLISH_STYLE_FILE", self.path("style.md"));
        c
    }

    fn records(&self) -> String {
        std::fs::read_to_string(self.path("state/claude/ja-polish/records.jsonl"))
            .unwrap_or_default()
    }

    fn gate_events(&self) -> String {
        std::fs::read_to_string(self.path("state/claude/gate-events.jsonl")).unwrap_or_default()
    }
}

fn text(o: &Output) -> (String, String, Option<i32>) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code(),
    )
}

const GOOD: &str = "<para n=\"1\">文章を直す。</para>";
const SRC: &str = "文章を直すことができる。\n";

#[test]
fn file_mode_polishes_records_and_is_idempotent() {
    let sb = Sandbox::new();
    sb.fake_llm(GOOD, false);
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();

    let (out, err, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("書き直し 1 / 変更なし 0 / 素通し 0"), "{out}");
    assert!(out.contains("- 文章を直すことができる。"), "{out}");
    assert!(out.contains("+ 文章を直す。"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "文章を直す。\n");
    assert_eq!(sb.records().lines().count(), 1);
    assert!(sb.gate_events().contains("\"gate\":\"ja-polish\""));
    assert!(sb.gate_events().contains("\"decision\":\"pass\""));

    // 指示文は stdin ではなく引数で渡るので、依頼文の中身は偽の llm が残した stdin で見る。
    let sent = std::fs::read_to_string(sb.path("llm.stdin")).unwrap();
    assert!(
        sent.contains("<text>\n文章を直すことができる。\n</text>"),
        "{sent}"
    );

    // 2 回目: 記録があるので何もしない(LLM も呼ばない)。
    std::fs::remove_file(sb.path("llm.stdin")).unwrap();
    let (out, _, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0));
    assert!(out.contains("書き直す日本語の段落はありません"), "{out}");
    assert!(!sb.path("llm.stdin").exists(), "2 回目は llm を呼ばない");
}

#[test]
fn stdin_mode_prints_text_and_reports_on_stderr() {
    let sb = Sandbox::new();
    sb.fake_llm(GOOD, false);
    let mut child = sb
        .cmd()
        .arg("--stdin")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(SRC.as_bytes())
        .unwrap();
    let (out, err, code) = text(&child.wait_with_output().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(out, "文章を直す。\n");
    assert!(err.contains("書き直し 1"), "{err}");
}

#[test]
fn dry_run_changes_nothing_and_records_nothing() {
    let sb = Sandbox::new();
    sb.fake_llm(GOOD, false);
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();
    let (out, _, code) = text(&sb.cmd().arg("--dry-run").arg(&file).output().unwrap());
    assert_eq!(code, Some(0));
    assert!(out.contains("+ 文章を直す。"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SRC);
    assert!(sb.records().is_empty());
    assert!(sb.gate_events().is_empty());
}

#[test]
fn llm_failure_passes_through_records_skip_and_logs_fail_open() {
    let sb = Sandbox::new();
    sb.fake_llm(
        "Error: No key found - add one with `llm keys set gemini`",
        true,
    );
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();

    let (out, _, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "素通しは失敗にしない");
    assert!(out.contains("素通し 1"), "{out}");
    assert!(out.contains("No key found"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SRC, "元のまま");
    assert!(sb.records().contains("\"state\":\"skipped\""));
    assert!(
        !sb.records().contains("transient"),
        "キーの未設定は一時的な失敗ではない: {}",
        sb.records()
    );
    assert!(sb.gate_events().contains("\"decision\":\"fail-open\""));
}

impl Sandbox {
    /// 設定ファイル(`ja-polish.toml`)を置く。待ちは 0 にして、テストを待たせない。
    fn config(&self, extra: &str) {
        let p = self.path("config/dotfiles/ja-polish.toml");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("retry_base_ms = 0\n{extra}")).unwrap();
    }

    /// 偽の `llm`: 最初の `busy_calls` 回は混雑のエラーで失敗し、その後は `body` を返す。
    /// 呼ばれるたびに、`-m` のモデル名と `-o thinking_level` の値(無ければ `none`)を
    /// `llm.args` に 1 行で残す(指示文は複数行なので、引数の全体は残さない)。
    fn flaky_llm(&self, busy_calls: u32, body: &str) {
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\nmodel=; level=none; prev=\nfor a in \"$@\"; do\n  [ \"$prev\" = \"-m\" ] && model=\"$a\"\n  [ \"$prev\" = \"thinking_level\" ] && level=\"$a\"\n  prev=\"$a\"\ndone\necho \"$model $level\" >> $0.args\nn=$(cat $0.count 2>/dev/null || echo 0)\nn=$((n+1))\necho $n > $0.count\nif [ \"$n\" -le {busy_calls} ]; then\n  echo 'Error: This model is currently experiencing high demand.' >&2\n  exit 1\nfi\nprintf '%s\\n' '{body}'\n"
        );
        let p = self.path("llm");
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn llm_args(&self) -> Vec<String> {
        std::fs::read_to_string(self.path("llm.args"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn busy_is_retried_and_the_thinking_level_is_passed_to_llm() {
    let sb = Sandbox::new();
    sb.config("thinking_level = \"minimal\"\nfallback_model = \"\"\n");
    sb.flaky_llm(1, GOOD);
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();

    let (out, err, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("書き直し 1 / 変更なし 0 / 素通し 0"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "文章を直す。\n");
    let args = sb.llm_args();
    assert_eq!(
        args,
        vec!["gemini-3.6-flash minimal", "gemini-3.6-flash minimal"],
        "混雑で 1 回失敗し、同じモデルへの再依頼で通った"
    );
    assert!(sb.gate_events().contains("\"decision\":\"pass\""));
}

#[test]
fn busy_forever_falls_back_then_records_a_transient_skip() {
    let sb = Sandbox::new();
    sb.config("fallback_model = \"fb-model\"\nthinking_level = \"low\"\n");
    sb.flaky_llm(99, GOOD);
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();

    let (out, _, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "素通しは失敗にしない");
    assert!(out.contains("素通し 1"), "{out}");
    assert!(out.contains("予備の fb-model も失敗"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SRC, "元のまま");
    let args = sb.llm_args();
    assert_eq!(
        args,
        vec![
            "gemini-3.6-flash low",
            "gemini-3.6-flash low",
            "gemini-3.6-flash low",
            "fb-model none"
        ],
        "主を 3 回、予備を 1 回(予備には思考量を渡さない)"
    );
    assert!(
        sb.records().contains("\"transient\":true"),
        "{}",
        sb.records()
    );
    assert!(sb.gate_events().contains("\"decision\":\"fail-open\""));
}

#[test]
fn a_passed_through_paragraph_is_polished_on_the_next_run_while_the_gate_counts_it_as_passed() {
    let sb = Sandbox::new();
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();

    // 1 回目: キーが無くて素通し。
    sb.fake_llm("Error: No key found - add one", true);
    let (out, _, _) = text(&sb.cmd().arg(&file).output().unwrap());
    assert!(out.contains("素通し 1"), "{out}");

    // 関所の見方(`--check`): 素通しも通した。動けない間に何も出せなくならないための設計。
    let (out, _, code) = text(&sb.cmd().arg("--check").arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("未通過の日本語の段落はありません"), "{out}");

    // 2 回目: 直ったので、同じ文面が書き直される(以前は「書き直す段落はありません」で、直らなかった)。
    sb.fake_llm(GOOD, false);
    let (out, err, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("書き直し 1 / 変更なし 0 / 素通し 0"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "文章を直す。\n");

    // 3 回目: 書き直した記録があるので、もう送らない。
    std::fs::remove_file(sb.path("llm.stdin")).unwrap();
    let (out, _, _) = text(&sb.cmd().arg(&file).output().unwrap());
    assert!(out.contains("書き直す日本語の段落はありません"), "{out}");
    assert!(!sb.path("llm.stdin").exists());
}

#[test]
fn missing_style_guide_refuses_to_send_and_passes_through() {
    let sb = Sandbox::new();
    sb.fake_llm(GOOD, false);
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();
    let (out, _, code) = text(
        &sb.cmd()
            .env("JA_POLISH_STYLE_FILE", sb.path("nope.md"))
            .arg(&file)
            .output()
            .unwrap(),
    );
    assert_eq!(code, Some(0));
    assert!(out.contains("素通し 1") && out.contains("nope.md"), "{out}");
    assert!(!sb.path("llm.stdin").exists(), "基準なしでは送らない");
}

#[test]
fn check_lists_unrecorded_paragraphs_and_clears_after_polishing() {
    let sb = Sandbox::new();
    sb.fake_llm(GOOD, false);
    let file = sb.path("doc.md");
    std::fs::write(&file, SRC).unwrap();

    let (out, _, code) = text(&sb.cmd().args(["--check"]).arg(&file).output().unwrap());
    assert_eq!(code, Some(1));
    assert!(out.contains("未通過") && out.contains("段落 1"), "{out}");

    sb.cmd().arg(&file).output().unwrap();
    let (out, _, code) = text(&sb.cmd().args(["--check"]).arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn only_changed_paragraphs_are_polished_when_a_baseline_exists() {
    let sb = Sandbox::new();
    let root = sb.path("repo");
    std::fs::create_dir(&root).unwrap();
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "commit.gpgsign", "false"]);
    let file = root.join("doc.md");
    std::fs::write(&file, "承認済みの翻訳調な段落です。\n").unwrap();
    git(&["add", "doc.md"]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(
        &file,
        "承認済みの翻訳調な段落です。\n\n文章を直すことができる。\n",
    )
    .unwrap();

    sb.fake_llm(GOOD, false);
    let (out, err, code) = text(&sb.cmd().arg(&file).output().unwrap());
    assert_eq!(code, Some(0), "{out}{err}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "承認済みの翻訳調な段落です。\n\n文章を直す。\n",
        "基点にある段落は触らない"
    );
    let sent = std::fs::read_to_string(sb.path("llm.stdin")).unwrap();
    assert!(
        sent.contains("<before>\n承認済みの翻訳調な段落です。\n</before>"),
        "前の段落は参考として渡す: {sent}"
    );
    assert!(!sent.contains("<text>\n承認済み"), "{sent}");
}

#[test]
fn bad_usage_exits_2() {
    let sb = Sandbox::new();
    let o = sb.cmd().output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}
