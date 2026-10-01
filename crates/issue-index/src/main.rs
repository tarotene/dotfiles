//! issue-index の hook 本体(SessionStart)。縮退はすべて exit 0
//! (SessionStart は exit 2 でもブロックできない)。詳細は lib.rs と
//! docs/claude/issue-index.md。

use issue_index::{build, owner_repo, total_count, Context, PrStatus, Scope};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

/// `command -v <name>`(PATH 上の実行可能ファイル)。
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|d| {
        let p = d.join(name);
        std::fs::metadata(&p)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false)
    })
}

fn git(project: &Path, args: &[&str]) -> Option<Output> {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
}

/// bash の `${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input" 2>/dev/null || echo '')}`。
fn project_dir(input: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLAUDE_PROJECT_DIR").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let v: Value = serde_json::from_str(input).ok()?;
    let s = match v.get("cwd")? {
        Value::Null | Value::Bool(false) => return None,
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    (!s.is_empty()).then(|| PathBuf::from(s))
}

fn gh(args: &[&str], capture_err: bool) -> Option<Child> {
    Command::new("gh")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if capture_err {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .ok()
}

/// 子の結果(終了コードが 0 か、stdout、stderr)。起動失敗は失敗扱い。
struct Res {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn wait(child: Option<Child>) -> Res {
    match child.map(Child::wait_with_output) {
        Some(Ok(o)) => Res {
            ok: o.status.success(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        _ => Res {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

fn fail(err: &str) -> ! {
    // bash: `fail "$(head -1 "$tmp/x.err")"` → `${1:-不明なエラー}`
    let first = err.split('\n').next().unwrap_or("");
    let msg = if first.is_empty() {
        "不明なエラー"
    } else {
        first
    };
    eprintln!("[issue-index] Issue 索引の取得に失敗しました: {msg}");
    std::process::exit(0);
}

/// `jq -r '.total_count // 0' file 2>/dev/null || echo 0` 相当(不正 JSON・空は 0)。
fn parse(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

fn main() {
    if !on_path("gh") {
        return;
    }
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let Some(project) = project_dir(&input) else {
        return;
    };
    if !project.is_dir() {
        return;
    }
    match git(&project, &["rev-parse", "--is-inside-work-tree"]) {
        Some(o) if o.status.success() => {}
        _ => return,
    }
    let remote_v = git(&project, &["remote", "-v"])
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let Some(nwo) = owner_repo(&remote_v) else {
        return;
    };
    let branch = git(&project, &["branch", "--show-current"])
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default();

    // 5 本の gh を並列に起動する(bash 版の `&` + `wait`)。
    let q_common = format!("repo:{nwo}+is:issue+is:open");
    let mine = gh(
        &[
            "api",
            &format!("search/issues?q={q_common}+assignee:@me+sort:updated-desc&per_page=15"),
        ],
        true,
    );
    let all = gh(
        &[
            "api",
            &format!("search/issues?q={q_common}+sort:updated-desc&per_page=15"),
        ],
        true,
    );
    let pr = (!branch.is_empty()).then(|| {
        gh(
            &[
                "pr",
                "list",
                "-R",
                &nwo,
                "--head",
                &branch,
                "--state",
                "open",
                "--limit",
                "1",
                "--json",
                "number,title,isDraft,closingIssuesReferences",
            ],
            false,
        )
    });
    let who = gh(
        &[
            "api",
            "graphql",
            "-f",
            "query={viewer{login}}",
            "--jq",
            ".data.viewer.login",
        ],
        false,
    );
    let handoff = gh(
        &[
            "api",
            &format!(
                "search/issues?q={q_common}+label:%22handoff:ai%22+sort:updated-desc&per_page=15"
            ),
        ],
        true,
    );

    let mine = wait(mine);
    let all = wait(all);
    let pr = pr.map(wait);
    let who = wait(who);
    let handoff = wait(handoff);

    if !mine.ok {
        fail(&mine.stderr);
    }
    let mine_v = parse(&mine.stdout);
    let all_v;
    let (scope, src) = if total_count(&mine_v) > 0 {
        (Scope::Mine, &mine_v)
    } else {
        if !all.ok {
            fail(&all.stderr);
        }
        all_v = parse(&all.stdout);
        if total_count(&all_v) <= 0 {
            return;
        }
        (Scope::All, &all_v)
    };

    // `[[ rc_who -eq 0 && -s who.txt ]] && me="$(cat who.txt)"`
    let me = if who.ok && !who.stdout.is_empty() {
        who.stdout.trim_end_matches('\n').to_string()
    } else {
        String::new()
    };

    let pr_status = match &pr {
        None => PrStatus::NoBranch,
        Some(r) if r.ok => PrStatus::Ok(&r.stdout),
        Some(_) => PrStatus::Failed,
    };

    let ctx = Context {
        nwo: &nwo,
        scope,
        src,
        me: &me,
        branch: &branch,
        pr: pr_status,
        handoff: handoff.ok.then_some(handoff.stdout.as_str()),
    };
    let (stdout, warn) = build(&ctx);
    if let Some(w) = warn {
        eprintln!("{w}");
    }
    print!("{stdout}");
}
