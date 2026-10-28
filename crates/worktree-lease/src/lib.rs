//! worktree-lease — 1 つの linked worktree を、同時に 2 つのセッションが git で動かさない
//! ための PreToolUse の判定と、`pr-gate` が「別セッションの作業」を見分ける印。設計:
//! docs/agents/worktree-lease.md。
//!
//! 困りごと(2026-10-27): 同じ worktree で 2 つのセッションが同時に git を操作し、一方の
//! 後始末(`git switch -`)のあとにブランチが別のものへ動いた。気付けたのは reflog を読んだとき
//! だけ。さらに `pr-gate` は、別セッションが作った commit・PR をこのセッションのものとして
//! Stop を止めた。
//!
//! 仕組み(検出のみ — 起動時に 1 worktree = 1 セッションを強制するのではない):
//!
//! - linked worktree の git-dir に `dotfiles-session-lease`(`{"session","ts"}`)を置く。
//!   Bash の呼び出しのたびに、自分の分を更新する。
//! - 別のセッションの lease が [`ACTIVE_SECS`] 秒以内に更新されているとき、HEAD を動かす git
//!   ([`moves_head`])を deny する。読み取りは通し、相手の lease には触れない。
//! - 古い lease(相手が終わった・長く止まっている)は黙って引き継ぐ。終了を知らせる hook が
//!   無いので、生きているかは更新時刻だけで見る。
//! - 本物の checkout(main worktree)は対象外。そちらは `main-checkout-guard` の責務。

use agent_session::{HookInput, Outcome};
use command_inspection::simple_shell::split_segments;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// この秒数以内に更新された別セッションの lease は「生きている」。
pub const ACTIVE_SECS: u64 = 120;

/// lease の置き場(git-dir の中)。
const LEASE_FILE: &str = "dotfiles-session-lease";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub session: String,
    pub ts: u64,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `dir` が linked worktree なら、その git-dir の絶対パス。本物の checkout・git の外は `None`。
fn linked_git_dir(dir: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--git-dir", "--git-common-dir"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut lines = text.lines();
    let abs = |p: &str| {
        let p = Path::new(p);
        std::fs::canonicalize(if p.is_absolute() {
            p.to_path_buf()
        } else {
            dir.join(p)
        })
        .ok()
    };
    let git_dir = abs(lines.next()?)?;
    let common = abs(lines.next()?)?;
    (git_dir != common).then_some(git_dir)
}

fn read_lease(git_dir: &Path) -> Option<Lease> {
    let text = std::fs::read_to_string(git_dir.join(LEASE_FILE)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some(Lease {
        session: v.get("session")?.as_str()?.to_owned(),
        ts: v.get("ts")?.as_u64()?,
    })
}

/// 一時ファイル経由で置き換える(読む側が書きかけを見ない)。失敗は無視する: lease は
/// 補助の印で、書けなくても作業を止めない。
fn write_lease(git_dir: &Path, lease: &Lease) {
    let tmp = git_dir.join(format!("{LEASE_FILE}.{}", std::process::id()));
    let body = json!({"session": lease.session, "ts": lease.ts}).to_string();
    if std::fs::write(&tmp, body).is_ok()
        && std::fs::rename(&tmp, git_dir.join(LEASE_FILE)).is_err()
    {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// 別セッションの、生きている lease。
fn live_foreign(git_dir: &Path, session: &str, at: u64) -> Option<Lease> {
    read_lease(git_dir).filter(|l| l.session != session && at.saturating_sub(l.ts) < ACTIVE_SECS)
}

/// `project` が linked worktree で、そこを別のセッションが使っているか(`pr-gate` 用)。
/// `session` が不明(`unknown`・空)のときは判定しない。
pub fn held_by_other(project: &Path, session: &str) -> bool {
    held_by_other_at(project, session, now())
}

pub fn held_by_other_at(project: &Path, session: &str, at: u64) -> bool {
    if session.is_empty() || session == "unknown" {
        return false;
    }
    linked_git_dir(project)
        .and_then(|g| live_foreign(&g, session, at))
        .is_some()
}

/// 手で外す(`worktree-lease release`)。外した lease があれば `true`。
pub fn release(dir: &Path) -> bool {
    linked_git_dir(dir).is_some_and(|g| std::fs::remove_file(g.join(LEASE_FILE)).is_ok())
}

/// 手で見る(`worktree-lease status`)。
pub fn status(dir: &Path) -> String {
    let Some(g) = linked_git_dir(dir) else {
        return "linked worktree ではありません(lease の対象外)".into();
    };
    match read_lease(&g) {
        Some(l) => {
            let age = now().saturating_sub(l.ts);
            let state = if age < ACTIVE_SECS {
                "生きている"
            } else {
                "古い"
            };
            format!("session {} が {age} 秒前に使用({state})", l.session)
        }
        None => "lease なし".into(),
    }
}

/// 先頭の `VAR=value` を読み飛ばす。
fn strip_env(words: &[String]) -> &[String] {
    let n = words
        .iter()
        .take_while(|w| {
            w.split_once('=').is_some_and(|(k, _)| {
                !k.is_empty()
                    && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !k.starts_with(|c: char| c.is_ascii_digit())
            })
        })
        .count();
    &words[n..]
}

/// `git [-C dir] [-c k=v] <sub> args…` の `(sub, args)`。
fn git_call(words: &[String]) -> Option<(&str, &[String])> {
    let words = strip_env(words);
    if words.first().map(String::as_str) != Some("git") {
        return None;
    }
    let mut i = 1;
    while let Some(w) = words.get(i) {
        match w.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" => i += 2,
            s if s.starts_with("--git-dir=")
                || s.starts_with("--work-tree=")
                || s.starts_with("--no-pager")
                || s == "-P" =>
            {
                i += 1
            }
            _ => break,
        }
    }
    let sub = words.get(i)?.as_str();
    Some((sub, &words[i + 1..]))
}

/// このコマンドが、HEAD・ブランチ・コミットを動かしうる git を含むか。展開などで解析できない文は、
/// 文の中に `git` と該当のサブコマンドが現れるかで粗く見る。
pub fn moves_head(cmd: &str) -> bool {
    const HEAD_MOVING: &[&str] = &[
        "switch",
        "checkout",
        "reset",
        "rebase",
        "merge",
        "cherry-pick",
        "revert",
        "commit",
        "push",
        "pull",
        "am",
    ];
    split_segments(cmd).iter().any(|seg| match &seg.words {
        Some(words) => git_call(words).is_some_and(|(sub, args)| {
            HEAD_MOVING.contains(&sub)
                || (sub == "branch"
                    && args.iter().any(|a| {
                        matches!(
                            a.as_str(),
                            "-d" | "-D" | "-m" | "-M" | "-f" | "--delete" | "--move" | "--force"
                        )
                    }))
        }),
        None => {
            let text = &seg.text;
            text.contains("git")
                && HEAD_MOVING
                    .iter()
                    .any(|s| text.contains(&format!(" {s}")) || text.contains(&format!("\t{s}")))
        }
    })
}

fn deny_reason(other: &Lease, at: u64) -> String {
    format!(
        "この worktree は別のセッション({})が {} 秒前まで使っていました(deny)。同じ worktree で git を同時に動かすと、ブランチが別のものへ動いたり、相手の commit・PR がこのセッションのものとして扱われたりします。別の worktree で作業するか(`herdr worktree create --cwd <repo> --branch <名前>`)、相手のセッションが終わっているなら、ユーザーが `! worktree-lease release` で外してください(エージェントが自分で外さない)。読み取りの git と、git 以外のコマンドは通ります。",
        other.session,
        at.saturating_sub(other.ts)
    )
}

/// 判定の本体(時刻を渡せる版)。Bash 以外・本物の checkout・セッション不明は `None`。
pub fn decide(input: &HookInput, at: u64) -> Option<Outcome> {
    let cmd = input.bash_command()?;
    let session = input.session_id.as_str();
    if session.is_empty() || session == "unknown" {
        return None;
    }
    let cwd = input.cwd.clone().or_else(|| std::env::current_dir().ok())?;
    let git_dir = linked_git_dir(&cwd)?;
    if let Some(other) = live_foreign(&git_dir, session, at) {
        // 相手が使っている間は、相手の lease に触れない。HEAD を動かす git だけを止める。
        return moves_head(cmd).then(|| Outcome::Deny(deny_reason(&other, at)));
    }
    let mine = read_lease(&git_dir).is_some_and(|l| l.session == session && at < l.ts + 5);
    if !mine {
        write_lease(
            &git_dir,
            &Lease {
                session: session.to_owned(),
                ts: at,
            },
        );
    }
    None
}

/// ルータ(`crates/hook-router`)が呼ぶ入口。標準入力はルータが読んだ生の JSON。
pub fn hook(raw: &str) -> Option<Outcome> {
    decide(&HookInput::parse(raw)?, now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    /// (一時ディレクトリ, 本物の checkout, linked worktree)
    fn repos() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        run(&main, &["init", "-q", "-b", "main"]);
        run(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let linked = tmp.path().join("linked");
        run(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "topic",
                linked.to_str().unwrap(),
            ],
        );
        (tmp, main, linked)
    }

    fn input(session: &str, cwd: &Path, cmd: &str) -> HookInput {
        HookInput::parse(
            &json!({"session_id": session, "cwd": cwd, "tool_name": "Bash",
                    "tool_input": {"command": cmd}})
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn head_moving_git_is_recognised_in_every_common_shape() {
        for yes in [
            "git switch -c x",
            "git checkout main",
            "git -C /tmp/w commit -m x",
            "GIT_ALLOW_MAIN_COMMIT=1 git commit -m x",
            "ls && git rebase origin/main",
            "git -c core.editor=true merge topic",
            "git push --force-with-lease origin x",
            "git branch -D old",
            "git reset --hard HEAD~1",
            "git cherry-pick abc",
        ] {
            assert!(moves_head(yes), "{yes}");
        }
        for no in [
            "git status",
            "git log --oneline -3",
            "git diff HEAD",
            "git branch --list",
            "git branch",
            "git fetch origin",
            "echo git switch",
            "ls -la",
            "cargo test",
            "",
        ] {
            assert!(!moves_head(no), "{no}");
        }
    }

    #[test]
    fn a_second_live_session_is_denied_head_moving_git_but_not_reads() {
        let (_t, _main, linked) = repos();
        let t0 = 1_000_000;
        // A が使い始める。
        assert!(decide(&input("A", &linked, "ls"), t0).is_none());
        assert_eq!(
            read_lease(&linked_git_dir(&linked).unwrap()),
            Some(Lease {
                session: "A".into(),
                ts: t0
            })
        );
        // B は読み取りを通され、A の lease は動かない。
        assert!(decide(&input("B", &linked, "git status"), t0 + 10).is_none());
        assert_eq!(
            read_lease(&linked_git_dir(&linked).unwrap())
                .unwrap()
                .session,
            "A"
        );
        // B が HEAD を動かす git は deny。理由に相手のセッションが出る。
        match decide(&input("B", &linked, "git switch -c x"), t0 + 20) {
            Some(Outcome::Deny(reason)) => {
                assert!(
                    reason.contains("(A)") && reason.contains("worktree-lease release"),
                    "{reason}"
                );
            }
            other => panic!("expected deny, got {other:?}"),
        }
        // 自分(A)は止められない。
        assert!(decide(&input("A", &linked, "git commit -m x"), t0 + 30).is_none());
    }

    #[test]
    fn a_stale_lease_is_taken_over_silently() {
        let (_t, _main, linked) = repos();
        let t0 = 2_000_000;
        assert!(decide(&input("A", &linked, "ls"), t0).is_none());
        let later = t0 + ACTIVE_SECS + 1;
        assert!(decide(&input("B", &linked, "git switch -c y"), later).is_none());
        assert_eq!(
            read_lease(&linked_git_dir(&linked).unwrap())
                .unwrap()
                .session,
            "B"
        );
        // 引き継がれた A が戻ってきて、B が生きている間は A が止められる。
        assert!(matches!(
            decide(&input("A", &linked, "git push"), later + 10),
            Some(Outcome::Deny(_))
        ));
    }

    #[test]
    fn the_real_checkout_and_non_bash_and_unknown_sessions_are_left_alone() {
        let (_t, main, linked) = repos();
        assert!(decide(&input("A", &main, "ls"), 5).is_none());
        assert!(decide(&input("B", &main, "git commit -m x"), 6).is_none());
        assert!(!main.join(".git").join(LEASE_FILE).exists());
        let not_bash = HookInput::parse(
            &json!({"session_id":"A","cwd":linked,"tool_name":"Read","tool_input":{"file_path":"x"}}).to_string(),
        )
        .unwrap();
        assert!(decide(&not_bash, 7).is_none());
        assert!(decide(&input("unknown", &linked, "git switch x"), 8).is_none());
        assert!(linked_git_dir(&linked).is_some_and(|g| !g.join(LEASE_FILE).exists()));
    }

    #[test]
    fn held_by_other_is_what_pr_gate_asks() {
        let (_t, main, linked) = repos();
        let t0 = 3_000_000;
        assert!(decide(&input("A", &linked, "ls"), t0).is_none());
        assert!(held_by_other_at(&linked, "B", t0 + 5));
        assert!(!held_by_other_at(&linked, "A", t0 + 5));
        assert!(!held_by_other_at(&linked, "B", t0 + ACTIVE_SECS + 1));
        assert!(!held_by_other_at(&linked, "unknown", t0 + 5));
        assert!(!held_by_other_at(&main, "B", t0 + 5));
    }

    #[test]
    fn release_removes_the_lease() {
        let (_t, _main, linked) = repos();
        assert!(decide(&input("A", &linked, "ls"), 100).is_none());
        assert!(release(&linked));
        assert!(!release(&linked));
        assert!(decide(&input("B", &linked, "git commit -m x"), 101).is_none());
    }
}
