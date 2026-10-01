//! 旧 `stack-base-guard.sh --selftest`(Claude 版 18 ケース)と Codex adapter の
//! `--selftest`(2 ケース)の移植(#415)。ケース番号・名前は bash 版のまま。
//!
//! 対象は既定で cargo の bin。`STACK_BASE_GUARD_ORACLE_DIR` にリポジトリの
//! `config/` を入れると bash 版(`claude/hooks/stack-base-guard.sh` /
//! `codex/hooks/stack-base-guard.sh`)に差し替わる(移植手順の段 2: 同じ
//! ケースを bash に向けて先に緑にする、docs/rust-migration.md)。
//!
//! 旧 selftest は decide_stack() をプロセス内で直接呼んでいた。ここでは同じ
//! 関数を外から叩ける `--check '<cmd>' <project>`(セッション ID は環境変数
//! `SESSION_ID`)を使う。hook 経路(stdin JSON)のケース(10・16・Codex)は
//! 旧 selftest でも別プロセスで起動していた。
//!
//! 旧 selftest 冒頭の「owner_repo() の式の骨格が pr-gate.sh / issue-index.sh と
//! ズレていないか」の検査は、式が guard-core の `repo::owner_repo` 1 箇所に
//! 集約されたので不要になった(guard-core の単体テストが同じ式を固定する)。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 旧 selftest の gh スタブ(そのまま):
///   STACK_STUB_PR_LIST_FILE   : gh pr list --json ... の応答(JSON 配列)
///   STACK_STUB_PR_LIST_RC     : gh pr list の exit code(既定 0)
///   STACK_STUB_PR_LIST_NWO    : 設定時は -R がこれと一致するときだけ応答を返す(#538)
///   STACK_STUB_DEFAULT_BRANCH : gh repo view --json defaultBranchRef の応答
const STUB_GH: &str = r#"#!/usr/bin/env bash
jqbin="$(command -v jq)"
case "$1" in
  pr)
    case "$2" in
      list)
        [[ "${STACK_STUB_PR_LIST_RC:-0}" == "0" ]] || exit 1
        nwo_arg=""
        shift 2
        while [[ $# -gt 0 ]]; do
          if [[ "$1" == "-R" ]]; then
            nwo_arg="$2"
            break
          fi
          shift
        done
        if [[ -n "${STACK_STUB_PR_LIST_NWO:-}" && "$nwo_arg" != "${STACK_STUB_PR_LIST_NWO}" ]]; then
          echo '[]'
        elif [[ -n "${STACK_STUB_PR_LIST_FILE:-}" && -f "${STACK_STUB_PR_LIST_FILE:-}" ]]; then
          cat "${STACK_STUB_PR_LIST_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  repo)
    case "$2" in
      view)
        if [[ -n "${STACK_STUB_DEFAULT_BRANCH:-}" ]]; then
          "$jqbin" -n --arg b "${STACK_STUB_DEFAULT_BRANCH}" '{defaultBranchRef:{name:$b}}' \
            | "$jqbin" -r '.defaultBranchRef.name'
        else
          exit 1
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
"#;

struct Run {
    code: i32,
    stdout: String,
}

impl Run {
    fn denied(&self) -> bool {
        self.code == 1 && self.stdout.starts_with("deny: ")
    }
    fn passed(&self) -> bool {
        self.code == 0 && self.stdout == "pass\n"
    }
}

/// 実験環境: github remote 付きの git repo。main <- stage1 <- stage2、main から
/// 直接切った unrelated ブランチ、別 owner/repo の第 2 リポジトリ(#538)。
struct Fx {
    tmp: tempfile::TempDir,
    repo: PathBuf,
    repo2: PathBuf,
    path: String,
    stage1: String,
    stage2: String,
}

fn git_env(c: &mut Command) {
    c.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    git_env(&mut c);
    let out = c.stdin(Stdio::null()).output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

fn commit(dir: &Path, msg: &str) {
    git_in(
        dir,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            msg,
        ],
    );
}

fn init_repo(dir: &Path, url: &str, base_msg: &str) {
    std::fs::create_dir_all(dir).unwrap();
    git_in(dir, &["init", "-q", "-b", "main"]);
    commit(dir, base_msg);
    git_in(dir, &["remote", "add", "origin", url]);
    let h = git_in(dir, &["rev-parse", "HEAD"]);
    git_in(dir, &["update-ref", "refs/remotes/origin/main", &h]);
    git_in(
        dir,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
}

fn oracle_dir() -> Option<PathBuf> {
    std::env::var_os("STACK_BASE_GUARD_ORACLE_DIR")
        .filter(|v| !v.is_empty())
        .map(|p| std::fs::canonicalize(p).expect("oracle dir"))
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, STUB_GH).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let repo = tmp.path().join("repo");
        init_repo(&repo, "https://github.com/example/example.git", "base");
        git_in(&repo, &["switch", "-c", "stage1", "-q"]);
        commit(&repo, "c1");
        let stage1 = git_in(&repo, &["rev-parse", "HEAD"]);
        git_in(&repo, &["switch", "-c", "stage2", "-q"]);
        commit(&repo, "c2");
        let stage2 = git_in(&repo, &["rev-parse", "HEAD"]);
        git_in(&repo, &["switch", "main", "-q"]);
        git_in(&repo, &["switch", "-c", "unrelated", "-q"]);
        commit(&repo, "u1");
        git_in(&repo, &["switch", "stage2", "-q"]);

        let repo2 = tmp.path().join("repo2");
        init_repo(
            &repo2,
            "https://github.com/other-owner/other-repo.git",
            "base2",
        );
        git_in(&repo2, &["switch", "-c", "feature", "-q"]);
        commit(&repo2, "f1");

        let fx = Fx {
            tmp,
            repo,
            repo2,
            path,
            stage1,
            stage2,
        };
        fx.write(
            "prs-stage1-only.json",
            &format!(
                r#"[{{"number":1,"headRefName":"stage1","headRefOid":"{}","baseRefName":"main"}}]"#,
                fx.stage1
            ),
        );
        fx.write(
            "prs-stage1-stage2.json",
            &format!(
                r#"[{{"number":1,"headRefName":"stage1","headRefOid":"{}","baseRefName":"main"}},{{"number":2,"headRefName":"stage2","headRefOid":"{}","baseRefName":"stage1"}}]"#,
                fx.stage1, fx.stage2
            ),
        );
        fx.write("prs-empty.json", "[]");
        fx
    }

    fn dir(&self) -> &Path {
        self.tmp.path()
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let p = self.dir().join(name);
        std::fs::write(&p, format!("{body}\n")).unwrap();
        p
    }

    fn state_root(&self) -> PathBuf {
        self.dir().join("state-root")
    }

    fn reset_state(&self) {
        let _ = std::fs::remove_dir_all(self.state_root());
    }

    fn switch(&self, b: &str) {
        git_in(&self.repo, &["switch", b, "-q"]);
    }

    /// 対象(cargo bin、または bash オラクル)のコマンド。`codex` なら Codex 版。
    fn cmd(&self, codex: bool) -> Command {
        let mut c = match oracle_dir() {
            Some(d) => {
                let script = if codex {
                    d.join("codex/hooks/stack-base-guard.sh")
                } else {
                    d.join("claude/hooks/stack-base-guard.sh")
                };
                let mut c = Command::new("bash");
                c.arg(script);
                c
            }
            None => {
                let mut c = Command::new(env!("CARGO_BIN_EXE_stack-base-guard"));
                if codex {
                    c.args(["--agent", "codex"]);
                }
                c
            }
        };
        git_env(&mut c);
        c.env("PATH", &self.path)
            .env("HOME", self.dir())
            .env("STACK_BASE_GUARD_DIR", self.state_root())
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("SKIP_STACK_BASE_GUARD")
            .env_remove("SESSION_ID")
            .env_remove("STACK_STUB_PR_LIST_FILE")
            .env_remove("STACK_STUB_PR_LIST_RC")
            .env_remove("STACK_STUB_PR_LIST_NWO")
            .env_remove("STACK_STUB_DEFAULT_BRANCH");
        c
    }

    /// 旧 `run_decide`(`--check`、SESSION_ID=selftest-sid)。
    fn decide(&self, prs: &str, cmd: &str, project: &Path, env: &[(&str, &str)]) -> Run {
        let mut c = self.cmd(false);
        c.args(["--check", cmd])
            .arg(project)
            .env("SESSION_ID", "selftest-sid")
            .env("STACK_STUB_PR_LIST_FILE", self.dir().join(prs));
        for (k, v) in env {
            c.env(k, v);
        }
        let out = c.stdin(Stdio::null()).output().unwrap();
        Run {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        }
    }

    /// hook 経路(stdin JSON)。
    fn hook(&self, codex: bool, stdin: &str, env: &[(&str, &str)]) -> Run {
        use std::io::Write;
        let mut c = self.cmd(codex);
        for (k, v) in env {
            c.env(k, v);
        }
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // skip 経路は stdin を読まずに終わるので、書き込みの EPIPE は無視する。
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        let out = child.wait_with_output().unwrap();
        Run {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        }
    }

    /// 層(ii) の前提: 1 本目(stage1, base main)を通して記録してから
    /// unrelated に切り替える。
    fn first_pr_from_stage1(&self) {
        self.reset_state();
        self.switch("stage1");
        let r = self.decide(
            "prs-empty.json",
            "gh pr create --base main --title t --body b",
            &self.repo,
            &[],
        );
        assert!(r.passed(), "1 本目は pass: {}", r.stdout);
        self.switch("unrelated");
    }
}

fn json_input(sid: &str, cwd: Option<&Path>, cmd: &str) -> String {
    let mut v = serde_json::json!({
        "session_id": sid,
        "tool_name": "Bash",
        "tool_input": {"command": cmd},
    });
    if let Some(c) = cwd {
        v["cwd"] = serde_json::Value::String(c.display().to_string());
    }
    v.to_string()
}

// --- 層(i) 祖先一致検査 ---

#[test]
fn t01_ancestor_pr_wrong_base_is_denied() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base main --title t --body b",
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
    assert!(
        r.stdout.contains("gh pr create --base stage1"),
        "1 正しい base を案内"
    );
    assert!(r.stdout.contains("#1"), "1 PR 番号を案内");
    // 理由文はバイト単位で bash 版と同じ
    assert_eq!(r.stdout, "deny: HEAD は open PR #1(stage1)のコミットを含んでいます。base を main にすると先行 PR の差分がこの PR に混入します。`gh pr create --base stage1` で作成してください。この積み方は依存の有無によらず常時とります(ADR-0027)。\n");
}

#[test]
fn t02_correct_base_passes() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base stage1 --title t --body b",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

#[test]
fn t03_first_pr_passes() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-empty.json",
        "gh pr create --base main --title t --body b",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

// --- 層(ii) セッションチェーン ---

#[test]
fn t04_second_pr_off_chain_without_tag_is_denied() {
    let fx = Fx::new();
    fx.first_pr_from_stage1();
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base main --title t --body b",
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
    assert!(r.stdout.contains("Independent-PR"));
    assert_eq!(r.stdout, "deny: このセッションでは既に PR(stage1)を作成しています。2 本目以降は直前の段に積むのが既定です(ADR-0027)。`git rebase --onto stage1 ...` で積み替えて `gh pr create --base stage1` とするか、真に独立な PR なら本文に `Independent-PR: <理由>` を書いて明示的に抜けてください。\n");
}

#[test]
fn t05_independent_tag_passes() {
    let fx = Fx::new();
    fx.first_pr_from_stage1();
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base main --title t --body 'Independent-PR: 緊急hotfixのため'",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

#[test]
fn t06_empty_independent_tag_is_denied() {
    let fx = Fx::new();
    fx.first_pr_from_stage1();
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base main --title t --body 'x Independent-PR: '",
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
}

// --- 縮退・境界 ---

#[test]
fn t07_heredoc_independent_tag_passes() {
    let fx = Fx::new();
    fx.first_pr_from_stage1();
    let cmd = "gh pr create --base main --body \"$(cat <<'EOF'\nIndependent-PR: heredoc 経由の理由\nEOF\n)\"";
    let r = fx.decide("prs-stage1-only.json", cmd, &fx.repo, &[]);
    assert!(r.passed(), "{}", r.stdout);
}

/// bleep の正準形(ADR-0003)は本文を --body-file の絶対パスだけで渡す(#675)。
#[test]
fn t07b_body_file_independent_tag_passes() {
    let fx = Fx::new();
    let body = fx.write("indep-body.md", "Independent-PR: 緊急hotfixのため");
    fx.first_pr_from_stage1();
    let r = fx.decide(
        "prs-stage1-only.json",
        &format!(
            "gh pr create -R example/example --base main --title t --body-file {}",
            body.display()
        ),
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

#[test]
fn t07b_body_file_without_tag_is_denied() {
    let fx = Fx::new();
    let body = fx.write("plain-body.md", "本文だけ");
    fx.first_pr_from_stage1();
    let r = fx.decide(
        "prs-stage1-only.json",
        &format!(
            "gh pr create -R example/example --base main --title t --body-file {}",
            body.display()
        ),
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
    assert!(r.stdout.contains("Independent-PR"));
}

#[test]
fn t08_non_command_position_passes() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-stage1-only.json",
        "echo 'gh pr create --base main'",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

#[test]
fn t09_gh_failure_passes() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base main --title t --body b",
        &fx.repo,
        &[("STACK_STUB_PR_LIST_RC", "1")],
    );
    assert!(r.passed(), "{}", r.stdout);
}

#[test]
fn t10_non_git_project_is_silent() {
    let fx = Fx::new();
    let nogit = fx.dir().join("nogit");
    std::fs::create_dir_all(&nogit).unwrap();
    let r = fx.hook(
        false,
        r#"{"session_id":"selftest-sid","tool_name":"Bash","tool_input":{"command":"gh pr create --base main"}}"#,
        &[("CLAUDE_PROJECT_DIR", nogit.to_str().unwrap())],
    );
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, "");
}

/// --web + 親あり(--base 省略、default branch は origin/HEAD symref から main)。
#[test]
fn t11_web_with_parent_is_denied() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide("prs-stage1-only.json", "gh pr create --web", &fx.repo, &[]);
    assert!(r.denied(), "{}", r.stdout);
    assert!(r.stdout.contains("stage1"));
}

// --- gh pr edit --base ---

#[test]
fn t12_edit_wrong_base_is_denied() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-stage1-stage2.json",
        "gh pr edit 2 --base main",
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
    assert!(r.stdout.contains("gh pr edit 2 --base stage1"));
    assert_eq!(r.stdout, "deny: PR #2(stage2)は open PR #1(stage1)のコミットを含んでいます。base を main にすると先行 PR の差分が混入します。`gh pr edit 2 --base stage1` としてください。この積み方は依存の有無によらず常時とります(ADR-0027)。\n");
}

#[test]
fn t13_edit_correct_base_passes() {
    let fx = Fx::new();
    let r = fx.decide(
        "prs-stage1-stage2.json",
        "gh pr edit 2 --base stage1",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

#[test]
fn t14_edit_without_base_passes() {
    let fx = Fx::new();
    let r = fx.decide(
        "prs-stage1-stage2.json",
        "gh pr edit 2 --add-label bug",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
}

/// -R によるクロスリポジトリ指定でも判定が効く。
#[test]
fn t15_edit_cross_repo_is_denied() {
    let fx = Fx::new();
    fx.reset_state();
    let r = fx.decide(
        "prs-stage1-stage2.json",
        "gh pr edit 2 -R example/example --base main",
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
    assert!(r.stdout.contains("stage1"));
}

/// #538: CLAUDE_PROJECT_DIR($repo、stage1/stage2 の chain あり)とは別に
/// clone された $repo2(chain なし)で -R 無しの create。cwd を優先すれば
/// nwo=other-owner/other-repo に解決され、スタブは空一覧を返す(pass)。
/// CLAUDE_PROJECT_DIR 優先の旧実装なら stage2 を stage1 の祖先ありと誤判定
/// して deny する。
#[test]
fn t16_cwd_is_preferred_over_project_dir() {
    let fx = Fx::new();
    fx.reset_state();
    fx.switch("stage2");
    let prs = fx.dir().join("prs-stage1-stage2.json");
    let r = fx.hook(
        false,
        &json_input(
            "selftest-sid-538",
            Some(&fx.repo2),
            "gh pr create --base main --title t --body b",
        ),
        &[
            ("CLAUDE_PROJECT_DIR", fx.repo.to_str().unwrap()),
            ("STACK_STUB_PR_LIST_NWO", "example/example"),
            ("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap()),
        ],
    );
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, "");
}

// --- Codex adapter ---

fn decision(stdout: &str) -> String {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| {
            v.pointer("/hookSpecificOutput/permissionDecision")
                .and_then(|d| d.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

#[test]
fn codex1_ancestor_pr_wrong_base_is_denied() {
    let fx = Fx::new();
    let prs = fx.dir().join("prs-stage1-only.json");
    let r = fx.hook(
        true,
        &json_input(
            "codex-selftest-1",
            Some(&fx.repo),
            "gh pr create --base main --title t --body b",
        ),
        &[("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap())],
    );
    assert_eq!(decision(&r.stdout), "deny", "{}", r.stdout);
}

#[test]
fn codex2_first_pr_passes() {
    let fx = Fx::new();
    let prs = fx.dir().join("prs-empty.json");
    let r = fx.hook(
        true,
        &json_input(
            "codex-selftest-2",
            Some(&fx.repo),
            "gh pr create --base main --title t --body b",
        ),
        &[("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap())],
    );
    assert_eq!(r.stdout, "");
}

// --- 追加: 旧 selftest に無かった分岐(bash オラクルで生成・確認済み) ---

/// Claude の hook 経路の deny 出力(`emit_deny` と同じバイト列)。
#[test]
fn hook_claude_deny_output_shape() {
    let fx = Fx::new();
    let prs = fx.dir().join("prs-stage1-only.json");
    let r = fx.hook(
        false,
        &json_input(
            "hook-sid",
            Some(&fx.repo),
            "gh pr create --base main --body b",
        ),
        &[("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap())],
    );
    assert_eq!(r.code, 0);
    assert!(
        r.stdout.starts_with(
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"HEAD は open PR #1(stage1)"
        ),
        "{}",
        r.stdout
    );
    assert!(r.stdout.ends_with("}\n"));
}

/// skip ファイルがあれば何もしない。
#[test]
fn hook_skip_file_is_silent() {
    let fx = Fx::new();
    std::fs::create_dir_all(fx.state_root()).unwrap();
    std::fs::write(fx.state_root().join("skip"), "").unwrap();
    let prs = fx.dir().join("prs-stage1-only.json");
    let r = fx.hook(
        false,
        &json_input(
            "hook-sid",
            Some(&fx.repo),
            "gh pr create --base main --body b",
        ),
        &[("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap())],
    );
    assert_eq!(r.stdout, "");
}

/// Codex は MCP tool を見ない(#161)。Claude は mcp__github*create_pull を判定する。
#[test]
fn hook_mcp_create_pull() {
    let fx = Fx::new();
    let prs = fx.dir().join("prs-stage1-only.json");
    let input = serde_json::json!({
        "session_id": "mcp-sid",
        "tool_name": "mcp__github__create_pull_request",
        "cwd": fx.repo.display().to_string(),
        "tool_input": {"base": "main", "body": "b"},
    })
    .to_string();
    let env = [("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap())];
    let r = fx.hook(false, &input, &env);
    assert!(
        r.stdout.contains("gh pr create --base stage1"),
        "{}",
        r.stdout
    );
    let r = fx.hook(true, &input, &env);
    assert_eq!(r.stdout, "");
}

/// MCP update_pull: pull_number で対象を引いて edit と同じ判定。
#[test]
fn hook_mcp_update_pull() {
    let fx = Fx::new();
    let prs = fx.dir().join("prs-stage1-stage2.json");
    let input = serde_json::json!({
        "session_id": "mcp-sid",
        "tool_name": "mcp__github__update_pull_request",
        "cwd": fx.repo.display().to_string(),
        "tool_input": {"base": "main", "pull_number": 2},
    })
    .to_string();
    let r = fx.hook(
        false,
        &input,
        &[("STACK_STUB_PR_LIST_FILE", prs.to_str().unwrap())],
    );
    assert!(
        r.stdout.contains("gh pr edit 2 --base stage1"),
        "{}",
        r.stdout
    );
}

/// 台帳の保存形式は bash 版と互換(`<dir>/state/<sid>.chain` に 1 行 1 ブランチ)。
/// bash 版が書いた既存の台帳をそのまま読める。
#[test]
fn ledger_format_is_bash_compatible() {
    let fx = Fx::new();
    fx.reset_state();
    let state = fx.state_root().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("selftest-sid.chain"), "stage1\n").unwrap();
    fx.switch("unrelated");
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base main --title t --body b",
        &fx.repo,
        &[],
    );
    assert!(r.denied(), "{}", r.stdout);
    assert!(r.stdout.contains("PR(stage1)"));
    // 直前の段に積めば pass し、head が追記される
    let r = fx.decide(
        "prs-stage1-only.json",
        "gh pr create --base stage1 --title t --body b",
        &fx.repo,
        &[],
    );
    assert!(r.passed(), "{}", r.stdout);
    assert_eq!(
        std::fs::read_to_string(state.join("selftest-sid.chain")).unwrap(),
        "stage1\nunrelated\n"
    );
}
