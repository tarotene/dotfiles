//! gh をスタブにした経路A(参照 Issue の子項目照合)と `--check` の統合テスト。
//!
//! trycmd の fixture(tests/cmd/)では PATH を差し替えられないため、gh を使う
//! ケースだけここに置く。`PLAN_SCOPE_GATE_UNDER_TEST` に実行ファイルのパスを
//! 渡すと、そのファイル(移植元の bash 版など)に同じケースを流せる
//! (docs/rust-migration.md の段2 → 段3)。

use plan_scope_gate::{deny_message, missing_lines};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn under_test() -> PathBuf {
    std::env::var_os("PLAN_SCOPE_GATE_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_plan-scope-gate")))
}

const GH_STUB: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_STUB_DIR/calls.log"
case "$1 $2" in
  "api graphql") [ -f "$GH_STUB_DIR/graphql.json" ] || exit 1; cat "$GH_STUB_DIR/graphql.json" ;;
  "issue view") [ -f "$GH_STUB_DIR/body.md" ] || exit 1; cat "$GH_STUB_DIR/body.md" ;;
  *) exit 1 ;;
esac
"#;

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        write_exec(&bin.join("gh"), GH_STUB);
        fs::create_dir(dir.path().join("gate")).unwrap();
        Env { dir }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn graphql(&self, json: &str) {
        fs::write(self.p("graphql.json"), json).unwrap();
    }
    fn body(&self, body: &str) {
        fs::write(self.p("body.md"), body).unwrap();
    }
    fn calls(&self) -> String {
        fs::read_to_string(self.p("calls.log")).unwrap_or_default()
    }
    fn path_var(&self) -> String {
        format!(
            "{}:{}",
            self.p("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }
    fn run(&self, args: &[&str], cwd: &Path, stdin: &str, path: &str) -> Output {
        let mut c = Command::new(under_test());
        c.args(args)
            .current_dir(cwd)
            .env("PATH", path)
            .env("GH_STUB_DIR", self.dir.path())
            .env("CLAUDE_PLAN_SCOPE_GATE_DIR", self.p("gate"))
            .env("HOME", self.p("home"))
            .env_remove("SKIP_PLAN_SCOPE_GATE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        use std::io::Write;
        // 早期終了(skip・gh 不在)では stdin を読まずに終わるので EPIPE は無視する。
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }
}

fn write_exec(p: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(p, body).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
}

fn git_repo(dir: &Path, origin: &str) {
    fs::create_dir_all(dir).unwrap();
    for args in [vec!["init", "-q"], vec!["remote", "add", "origin", origin]] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(&args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap()
            .success());
    }
}

const SUB2: &str = r#"{"data":{"repository":{"issue":{"subIssues":{"totalCount":2,"nodes":[{"number":137,"title":"t1"},{"number":138,"title":"t2"}]}}}}}"#;
const SUB0: &str = r#"{"data":{"repository":{"issue":{"subIssues":{"totalCount":0,"nodes":[]}}}}}"#;

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn check_sub_issues_missing() {
    let e = Env::new();
    e.graphql(SUB2);
    fs::write(
        e.p("plan.md"),
        "## 要求インベントリ\n- R1: #137 は段1で実装\n",
    )
    .unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#136"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        s(&o.stdout),
        "children: {\"totalCount\":2,\"kind\":\"sub\",\"items\":[{\"number\":137,\"title\":\"t1\"},{\"number\":138,\"title\":\"t2\"}]}\nMISSING\n#138 t2\n"
    );
    let calls = e.calls();
    assert!(
        calls.contains("-f owner=o -f repo=r -F number=136"),
        "{calls}"
    );
    assert!(calls.starts_with(
        "api graphql -f query=\n    query($owner:String!,$repo:String!,$number:Int!){"
    ));
}

#[test]
fn check_covered_and_reference_only() {
    let e = Env::new();
    e.graphql(SUB2);
    fs::write(
        e.p("plan.md"),
        "## 要求インベントリ\n- R1: #137 は段1で実装\n- R2: #138 — Obsolete: 済\n",
    )
    .unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#136"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert!(s(&o.stdout).ends_with("\nCOVERED\n"), "{}", s(&o.stdout));
    fs::write(e.p("plan.md"), "Reference-Only: o/r#136 — 参照のみ\n").unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#136"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert!(
        s(&o.stdout).ends_with("\nREFERENCE_ONLY\n"),
        "{}",
        s(&o.stdout)
    );
}

#[test]
fn check_bare_ref_resolves_origin() {
    let e = Env::new();
    e.graphql(SUB2);
    let repo = e.p("repo");
    git_repo(&repo, "git@github.com:foo/bar.git");
    fs::write(repo.join("plan.md"), "x\n").unwrap();
    let o = e.run(&["--check", "plan.md", "#136"], &repo, "", &e.path_var());
    assert_eq!(o.status.code(), Some(0), "{}", s(&o.stderr));
    assert!(e.calls().contains("-f owner=foo -f repo=bar -F number=136"));
    assert!(s(&o.stdout).contains("\nMISSING\n#137 t1\n#138 t2\n"));
}

#[test]
fn check_bare_ref_without_remote_fails() {
    let e = Env::new();
    fs::write(e.p("plan.md"), "x\n").unwrap();
    let o = e.run(
        &["--check", "plan.md", "#136"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(o.stdout, b"");
    assert_eq!(
        s(&o.stderr),
        "cwd is not inside a GitHub-remote repo; pass owner/repo#N explicitly\n"
    );
}

#[test]
fn check_gh_failure() {
    let e = Env::new();
    fs::write(e.p("plan.md"), "x\n").unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#1"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        s(&o.stderr),
        "failed to fetch children for o/r#1 (gh 不在・未認証・ネットワーク不通のいずれか)\n"
    );
}

#[test]
fn check_checkbox_fallback() {
    let e = Env::new();
    e.graphql(SUB0);
    e.body("intro\n- [ ] child item one two three four five\n- [x] done item\n  * [ ] SECOND unchecked item that is quite long indeed and more\n- [ ] short\n- [ ] \n");
    fs::write(
        e.p("plan.md"),
        "計画: child item one two three four five を扱う\n",
    )
    .unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#1"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        s(&o.stdout),
        "children: {\"totalCount\":3,\"kind\":\"checkbox\",\"items\":[{\"text\":\"child item one two three four five\"},{\"text\":\"SECOND unchecked item that is quite long indeed and more\"},{\"text\":\"short\"}]}\nMISSING\nSECOND unchecked item that is quite long indeed and more\n"
    );
    assert!(e
        .calls()
        .contains("issue view 1 --repo o/r --json body -q .body"));
    // 大文字小文字を無視した 40 文字前方一致で拾う
    fs::write(
        e.p("plan.md"),
        "child item one two three four five\nsecond unchecked item that is quite long\n",
    )
    .unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#1"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert!(s(&o.stdout).ends_with("\nCOVERED\n"), "{}", s(&o.stdout));
}

#[test]
fn check_issue_view_failure_is_none() {
    let e = Env::new();
    e.graphql(SUB0);
    fs::write(e.p("plan.md"), "x\n").unwrap();
    let o = e.run(
        &["--check", "plan.md", "o/r#1"],
        e.dir.path(),
        "",
        &e.path_var(),
    );
    assert_eq!(
        s(&o.stdout),
        "children: {\"totalCount\":0,\"kind\":\"none\",\"items\":[]}\nSKIP\n"
    );
}

fn transcript(e: &Env, lines: &[&str]) -> PathBuf {
    let p = e.p("t.jsonl");
    fs::write(&p, lines.join("\n") + "\n").unwrap();
    p
}

fn hook_input(cwd: &Path, transcript: &Path, plan: &str) -> String {
    serde_json::json!({
        "session_id": "s1",
        "cwd": cwd,
        "hook_event_name": "PreToolUse",
        "tool_name": "ExitPlanMode",
        "transcript_path": transcript,
        "tool_input": {"plan": plan},
    })
    .to_string()
}

#[test]
fn hook_route_a_denies_missing_children() {
    let e = Env::new();
    e.graphql(SUB2);
    let repo = e.p("repo");
    git_repo(&repo, "https://github.com/tarotene/dotfiles.git");
    let t = transcript(
        &e,
        &[
            r##"{"type":"user","message":{"content":[{"type":"tool_result","content":"issue-index: #999"}]}}"##,
            r##"{"type":"user","promptSource":"system","origin":{"kind":"task-notification"},"message":{"content":"--check plan.md #998"}}"##,
            r##"{"type":"user","promptSource":"typed","message":{"content":"#136 と other/repo#5 と telepath#7 をお願い"}}"##,
        ],
    );
    let plan = "## 要求インベントリ\n- R1: #137 は段1で実装\n- R2: 処分なし\n";
    let o = e.run(
        &[],
        &e.dir.path().join("bin"),
        &hook_input(&repo, &t, plan),
        &e.path_var(),
    );
    assert_eq!(o.status.code(), Some(0));
    let mut lines = missing_lines("other/repo#5", &["#138 t2".to_string()]);
    lines.extend(missing_lines(
        "tarotene/dotfiles#136",
        &["#138 t2".to_string()],
    ));
    lines.push("R2: 処分(実装する段、または棄却タグ)が未記載です".into());
    assert_eq!(
        s(&o.stdout),
        hook_io::jqfmt::deny_for_event("PreToolUse", &deny_message(&lines))
    );
    let calls = e.calls();
    assert!(!calls.contains("number=999") && !calls.contains("number=998"));
    assert!(!calls.contains("number=7"), "{calls}");
}

#[test]
fn hook_route_a_reference_only_and_covered_pass() {
    let e = Env::new();
    e.graphql(SUB2);
    let repo = e.p("repo");
    git_repo(&repo, "git@github.com:tarotene/dotfiles.git");
    let t = transcript(
        &e,
        &[r##"{"type":"user","message":{"content":"#136 をお願い"}}"##],
    );
    let o = e.run(
        &[],
        &repo,
        &hook_input(&repo, &t, "Reference-Only: #136 — 参照だけ\n"),
        &e.path_var(),
    );
    assert_eq!(s(&o.stdout), "");
    let o = e.run(
        &[],
        &repo,
        &hook_input(
            &repo,
            &t,
            "## 要求インベントリ\n- R1: #137 段1\n- R2: #138 User-Excluded: 除外\n",
        ),
        &e.path_var(),
    );
    assert_eq!(s(&o.stdout), "");
}

#[test]
fn hook_gh_failure_is_fail_open() {
    let e = Env::new();
    let repo = e.p("repo");
    git_repo(&repo, "git@github.com:tarotene/dotfiles.git");
    let t = transcript(
        &e,
        &[r##"{"type":"user","message":{"content":"#136 をお願い"}}"##],
    );
    let o = e.run(&[], &repo, &hook_input(&repo, &t, "計画\n"), &e.path_var());
    assert_eq!(s(&o.stdout), "");
}

#[test]
fn hook_without_gh_is_silent() {
    let e = Env::new();
    // gh だけを除いた PATH(他のコマンドは元の PATH から symlink で並べる)
    let farm = e.p("farm");
    fs::create_dir(&farm).unwrap();
    for d in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for ent in rd.flatten() {
            let name = ent.file_name();
            if name == "gh" || farm.join(&name).exists() {
                continue;
            }
            let _ = std::os::unix::fs::symlink(ent.path(), farm.join(&name));
        }
    }
    let input = serde_json::json!({"cwd": "/", "tool_input": {"plan": "## 要求インベントリ\n- R1: 未記載\n"}})
        .to_string();
    let o = e.run(&[], e.dir.path(), &input, &farm.display().to_string());
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(s(&o.stdout), "");
    // 同じ入力でも gh があれば経路Bで deny する(対照)
    let o = e.run(&[], e.dir.path(), &input, &e.path_var());
    assert!(s(&o.stdout).contains("\"permissionDecision\": \"deny\""));
}
