//! 実際の一時 git repo と gh / adr-number-check のスタブで hook を走らせる
//! 統合テスト。旧 `adr-number.sh --selftest`(Claude 版 15 チェック)と
//! `config/codex/hooks/adr-number.sh --selftest`(3 ケース)の 1 対 1 対応。
//!
//! `ADR_NUMBER_UNDER_TEST` に実行ファイルを渡すと、そのファイルに同じケースを
//! 流せる(移植元 bash 版の照合用。Codex 版は `--agent codex` を渡さないので
//! bash 側には Claude 版を指定する)。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn under_test() -> PathBuf {
    std::env::var_os("ADR_NUMBER_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_adr-number")))
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn git_init(dir: &Path) {
    let ok = Command::new("git")
        .args(["init", "-q"])
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

struct Fx {
    tmp: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let fx = Fx { tmp };
        fs::create_dir_all(fx.bin("bin")).unwrap();
        // gh スタブ(pr view のみ応答、それ以外は失敗)
        script(
            &fx.bin("bin/gh"),
            r#"if [[ "$1 $2" == "pr view" ]]; then echo 999; exit 0; fi; exit 1"#,
        );
        script(&fx.bin("bin/gh-noop"), "exit 1");
        script(
            &fx.bin("bin/adr-number-check"),
            r#"echo "called with: $*" >> "$ADR_FIX_LOG"; echo "0000-x.md -> 999-x.md"; exit 0"#,
        );
        fx
    }

    fn bin(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn log(&self) -> PathBuf {
        self.bin("fix.log")
    }

    fn logged(&self) -> String {
        fs::read_to_string(self.log()).unwrap_or_default()
    }

    /// docs/adr/<draft> と scripts/adr-number-check(opt-in)を持つ git repo。
    fn repo(&self, name: &str, draft: Option<&str>, opt_in: bool) -> PathBuf {
        let repo = self.bin(name);
        fs::create_dir_all(repo.join("docs/adr")).unwrap();
        git_init(&repo);
        if let Some(d) = draft {
            fs::write(repo.join("docs/adr").join(d), "# ADR-0000 — x\n").unwrap();
        }
        if opt_in {
            fs::create_dir_all(repo.join("scripts")).unwrap();
            script(&repo.join("scripts/adr-number-check"), "exit 0");
        }
        repo
    }

    /// `path_dir` を PATH の先頭に足して hook を走らせ、stdout を返す。
    fn run(
        &self,
        agent: Option<&str>,
        path_dir: &Path,
        project_env: Option<(&str, &Path)>,
        input: &str,
    ) -> String {
        fs::write(self.log(), "").unwrap();
        let mut c = Command::new(under_test());
        if let Some(a) = agent {
            c.args(["--agent", a]);
        }
        c.env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CODEX_PROJECT_DIR")
            .env("ADR_NUMBER_CHECK_BIN", self.bin("bin/adr-number-check"))
            .env("ADR_FIX_LOG", self.log())
            .env(
                "PATH",
                format!("{}:{}", path_dir.display(), std::env::var("PATH").unwrap()),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some((k, v)) = project_env {
            c.env(k, v);
        }
        let mut child = c.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(input.as_bytes()).ok();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "exit {:?}", out.status);
        String::from_utf8(out.stdout).unwrap()
    }
}

fn input(cwd: &Path, cmd: &str) -> String {
    serde_json::json!({"tool_name":"Bash","tool_input":{"command":cmd},"cwd":cwd}).to_string()
}

const CREATE: &str = "gh pr create --title x --body y";

// ---- Claude 版 selftest 相当(15 チェック) ----

#[test]
fn case1_draft_and_pr_create_emits_context() {
    // 1a / 1b
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-x.md"), true);
    let out = fx.run(
        None,
        &fx.bin("bin"),
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        &input(&repo, CREATE),
    );
    assert!(out.contains("999"), "1a: {out}");
    assert!(fx.logged().contains("--fix 999"), "1b: {}", fx.logged());
    // 出力は jq -n と同じ整形・末尾改行
    assert!(out.starts_with("{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PostToolUse\",\n    \"additionalContext\": \"ADR-0000 を PR #999 の番号へ改番しました(0000-x.md -> 999-x.md)。commit して push してください。\"\n  }\n}\n"), "{out}");
}

#[test]
fn case1c_base_branch_passed_to_fix() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-x.md"), true);
    let basebin = fx.bin("basebin");
    fs::create_dir_all(&basebin).unwrap();
    script(
        &basebin.join("gh"),
        r#"if [[ "$1 $2" == "pr view" ]]; then echo "999 stack/prev"; exit 0; fi; exit 1"#,
    );
    fx.run(
        None,
        &basebin,
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        &input(&repo, CREATE),
    );
    assert!(
        fx.logged().contains("--fix 999 --base origin/stack/prev"),
        "{}",
        fx.logged()
    );
}

#[test]
fn case2_no_draft_early_exit() {
    // 2a / 2b
    let fx = Fx::new();
    let repo = fx.repo("repo2", None, true);
    let out = fx.run(
        None,
        &fx.bin("bin"),
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        &input(&repo, "gh pr create --title x"),
    );
    assert_eq!(out, "", "2a");
    assert_eq!(fx.logged(), "", "2b");
}

#[test]
fn case2c_not_opted_in_repo_is_left_alone() {
    // 2c / 2d / 2e / 2f / 2g
    let fx = Fx::new();
    let repo = fx.repo("repo3", Some("0000-template.md"), false);
    let inp = input(&repo, "gh pr create --title x");
    let out = fx.run(
        None,
        &fx.bin("bin"),
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        &inp,
    );
    assert_eq!(out, "", "2c");
    assert_eq!(fx.logged(), "", "2d");
    assert!(repo.join("docs/adr/0000-template.md").exists(), "2e");
    // CLAUDE_PROJECT_DIR 未設定(cwd 解決)でも出力なし
    let out = fx.run(None, &fx.bin("bin"), None, &inp);
    assert_eq!(out, "", "2f");
    assert_eq!(fx.logged(), "", "2g");
}

#[test]
fn case3_not_pr_create_does_nothing() {
    // 3a / 3b
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-x.md"), true);
    let out = fx.run(
        None,
        &fx.bin("bin"),
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        &input(&repo, "git status"),
    );
    assert_eq!(out, "", "3a");
    assert_eq!(fx.logged(), "", "3b");
}

#[test]
fn case4_gh_pr_view_fails_is_silent() {
    // 4a / 4b
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-x.md"), true);
    let badbin = fx.bin("badbin");
    fs::create_dir_all(&badbin).unwrap();
    std::os::unix::fs::symlink(fx.bin("bin/gh-noop"), badbin.join("gh")).unwrap();
    let out = fx.run(
        None,
        &badbin,
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        &input(&repo, CREATE),
    );
    assert_eq!(out, "", "4a");
    assert_eq!(fx.logged(), "", "4b");
}

#[test]
fn case5_non_bash_tool_does_nothing() {
    // 5a
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-x.md"), true);
    let out = fx.run(
        None,
        &fx.bin("bin"),
        Some(("CLAUDE_PROJECT_DIR", &repo)),
        r#"{"tool_name":"Read","tool_input":{}}"#,
    );
    assert_eq!(out, "", "5a");
}

// ---- Codex adapter selftest 相当(3 ケース) ----

#[test]
fn codex_emits_context_via_codex_project_dir() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-draft.md"), true);
    let out = fx.run(
        Some("codex"),
        &fx.bin("bin"),
        Some(("CODEX_PROJECT_DIR", &repo)),
        &input(&repo, CREATE),
    );
    assert!(out.contains("additionalContext"), "{out}");
    // CLAUDE_PROJECT_DIR は Codex では見ない(cwd 解決でも同じ結果)
    let out = fx.run(Some("codex"), &fx.bin("bin"), None, &input(&repo, CREATE));
    assert!(out.contains("additionalContext"), "{out}");
}

#[test]
fn codex_non_pr_create_is_silent() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("0000-draft.md"), true);
    let out = fx.run(
        Some("codex"),
        &fx.bin("bin"),
        None,
        &input(&repo, "git status"),
    );
    assert_eq!(out, "");
}

#[test]
fn codex_not_opted_in_repo_is_silent() {
    let fx = Fx::new();
    let repo = fx.repo("repo3", Some("0000-template.md"), false);
    let out = fx.run(
        Some("codex"),
        &fx.bin("bin"),
        None,
        &input(&repo, "gh pr create --title x"),
    );
    assert_eq!(out, "");
}

// ---- command_ran_pr_create の境界(guard-core の対応表どおり) ----

#[test]
fn pr_create_detection_boundaries() {
    use adr_number::command_ran_pr_create as d;
    assert!(d("gh pr create --title x"));
    assert!(d("git push && gh pr create --fill"));
    assert!(d("/usr/bin/gh pr create"));
    assert!(!d("echo gh pr create"));
    assert!(!d("gh pr view"));
    assert!(!d("gh pr"));
    assert!(!d("gh pr create 'unterminated"));
}
