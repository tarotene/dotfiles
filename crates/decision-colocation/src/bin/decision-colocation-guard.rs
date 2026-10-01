//! decision-colocation-guard のエントリポイント。判定は
//! `decision_colocation::guard`(bash 版 decision-colocation-guard.sh の移植、
//! ADR-396、docs/claude/decision-colocation.md)。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse、matcher: "Bash|mcp__.*")
//!   手動 e2e:   decision-colocation-guard --check '<コマンド文字列>' [<project-dir>]
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): stdin が読めない・
//! 不正 JSON・project が git 作業ツリーでない場合は何も出さず exit 0。
//! Codex / Copilot 向けの adapter は意図的に作らない — CI required check が
//! 全エージェント共通の backstop になるため(ADR-396 の非目標)。

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use decision_colocation::guard::Guard;
use guard_core::hook::{deny_output, jq_r_path, ToolCall};
use hook_io::Agent;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--check") {
        let cmd = args.get(1).map(String::as_str).unwrap_or("");
        let project = match args.get(2).filter(|p| !p.is_empty()) {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir().unwrap_or_default(),
        };
        return match Guard::with_env(|g| g.decide(cmd, &project)) {
            Some(reason) => {
                println!("deny: {reason}");
                ExitCode::from(1)
            }
            None => {
                println!("pass");
                ExitCode::SUCCESS
            }
        };
    }

    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }
    let Some(call) = ToolCall::parse(Agent::Claude, &buf) else {
        return ExitCode::SUCCESS;
    };

    // `${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty')}`
    let project = match std::env::var("CLAUDE_PROJECT_DIR") {
        Ok(p) if !p.is_empty() => p,
        _ => jq_r_path(&call.raw, &["cwd"])
            .ok()
            .flatten()
            .unwrap_or_default(),
    };
    if project.is_empty() {
        return ExitCode::SUCCESS;
    }
    let project = PathBuf::from(project);
    let inside = Command::new("git")
        .arg("-C")
        .arg(&project)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !inside {
        return ExitCode::SUCCESS;
    }

    // Bash 以外(MCP 等)は対象外。
    let Some(cmd) = call.bash_command() else {
        return ExitCode::SUCCESS;
    };
    if let Some(reason) = Guard::with_env(|g| g.decide(&cmd, &project)) {
        print!("{}", deny_output(Agent::Claude, &reason));
    }
    ExitCode::SUCCESS
}
