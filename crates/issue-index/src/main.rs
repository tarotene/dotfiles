//! issue-index の hook 本体(SessionStart)。縮退はすべて exit 0
//! (SessionStart は exit 2 でもブロックできない)。詳細は lib.rs と
//! docs/agents/issue-index.md。

use issue_index::{
    build, build_human_only, ledger, owner_repo, total_count, Context, PrStatus, Scope,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

fn git(project: &Path, args: &[&str]) -> Option<Output> {
    git_repository::run(project, args)
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

/// 機体の名前(`dotfiles.host.name` が書く `$XDG_CONFIG_HOME/dotfiles/host`)。無い・空なら `None`。
fn read_host() -> Option<String> {
    let path = process_env::Xdg::Config.resolve()?.join("dotfiles/host");
    let text = std::fs::read_to_string(path).ok()?;
    let host = text.trim();
    (!host.is_empty()).then(|| host.to_string())
}

/// hms がこの機体に最後に適用した dotfiles の rev(`$XDG_STATE_HOME/hms/last-apply.json`)。
fn applied_rev() -> Option<String> {
    let path = process_env::Xdg::State
        .resolve()?
        .join("hms/last-apply.json");
    ledger::applied_rev(&std::fs::read_to_string(path).ok()?)
}

/// 台帳の節を組み立てる。台帳が無い・取得に失敗した・未了が無いなら `None`(沈黙)。
///
/// 着手の可否: 台帳の各項目の PR の状態を 1 回の GraphQL で読み、マージ済みなら merge commit が
/// この機体の適用 rev の祖先かを `git merge-base --is-ancestor` で確かめる。適用 rev がこの repo の
/// commit でない(別の repo の台帳、記録が無い)ときは、適用を確かめられないものとして扱う。
fn ledger_section(project: &Path, nwo: &str, host: &str, list: Res) -> Option<String> {
    if !list.ok {
        return None;
    }
    let v: Value = serde_json::from_str(&list.stdout).ok()?;
    let title = ledger::ledger_title(host);
    let issue = v
        .as_array()?
        .iter()
        .find(|i| i.get("title").and_then(Value::as_str) == Some(title.as_str()))?;
    let number = issue.get("number")?.as_u64()?;
    let parsed = ledger::parse(issue.get("body").and_then(Value::as_str).unwrap_or(""));

    let mut prs: Vec<u64> = parsed.open.iter().filter_map(|i| i.pr).collect();
    prs.sort_unstable();
    prs.dedup();
    let states = match nwo.split_once('/') {
        Some((owner, name)) if !prs.is_empty() => {
            let query = ledger::pr_states_query(owner, name, &prs);
            let r = wait(gh(
                &["api", "graphql", "-f", &format!("query={query}")],
                false,
            ));
            if r.ok {
                ledger::pr_states(&r.stdout)
            } else {
                Default::default()
            }
        }
        _ => Default::default(),
    };

    let rev = applied_rev().filter(|rev| {
        git(project, &["cat-file", "-e", &format!("{rev}^{{commit}}")])
            .is_some_and(|o| o.status.success())
    });
    let applied = |oid: &str| -> Option<bool> {
        let rev = rev.as_deref()?;
        let o = git(project, &["merge-base", "--is-ancestor", oid, rev])?;
        match o.status.code() {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        }
    };
    let status: Vec<_> = parsed
        .open
        .iter()
        .map(|i| ledger::item_readiness(i, &states, applied))
        .collect();
    ledger::section(number, host, &parsed, &status)
}

/// `jq -r '.total_count // 0' file 2>/dev/null || echo 0` 相当(不正 JSON・空は 0)。
fn parse(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

fn main() {
    if !process_control::command_exists("gh") {
        return;
    }
    let input = agent_session::entry::read_stdin();
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

    // 6 本の gh を並列に起動する(bash 版の `&` + `wait`)。
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

    // 個人 owner 全体の軽い人手タスク。リポジトリ単位でなく owner 単位で引くのは、人手タスクが
    // 作業中のリポジトリと無関係な場所で起きるため。owner が viewer と別(会社など)なら、
    // 結果は使わない(下の `human_raw`)。
    let owner = nwo.split('/').next().unwrap_or("").to_string();
    let human = gh(
        &[
            "api",
            &format!(
                "search/issues?q=owner:{owner}+is:issue+is:open+assignee:@me+label:%22handoff:human%22+label:%22handoff:light%22+sort:updated-desc&per_page=15"
            ),
        ],
        true,
    );

    // この機体の確認の台帳(ADR-1529 の置き場 6)。機体の名前は home-manager が書く marker から読み、
    // marker の無い機体では問い合わせない。題の完全一致は、取得のあとで見る(検索は語に分けて当てる)。
    let host = read_host();
    let ledger_list = host.as_ref().map(|_| {
        gh(
            &[
                "issue",
                "list",
                "-R",
                &nwo,
                "--state",
                "open",
                "--search",
                "verify-ledger in:title",
                "--json",
                "number,title,body",
                "--limit",
                "20",
            ],
            false,
        )
    });

    let mine = wait(mine);
    let all = wait(all);
    let pr = pr.map(wait);
    let who = wait(who);
    let handoff = wait(handoff);
    let human = wait(human);
    let ledger_text = match (host.as_deref(), ledger_list) {
        (Some(host), Some(child)) => ledger_section(&project, &nwo, host, wait(child)),
        _ => None,
    };

    // `[[ rc_who -eq 0 && -s who.txt ]] && me="$(cat who.txt)"`
    let me = if who.ok && !who.stdout.is_empty() {
        who.stdout.trim_end_matches('\n').to_string()
    } else {
        String::new()
    };

    // owner が viewer 自身のときだけ使う(viewer が不明なら使わない: fail-closed)。
    let human_raw = (human.ok && !me.is_empty() && owner.eq_ignore_ascii_case(&me))
        .then_some(human.stdout.as_str());

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
            // open Issue が 1 件も無い。それでも軽い人手タスクがあれば、
            // その節だけを出す(#831)。無ければ従来どおり沈黙する。
            if let Some(out) = build_human_only(&nwo, human_raw) {
                print!("{out}");
            }
            return;
        }
        (Scope::All, &all_v)
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
        human: human_raw,
        ledger: ledger_text.as_deref(),
    };
    let (stdout, warn) = build(&ctx);
    if let Some(w) = warn {
        eprintln!("{w}");
    }
    print!("{stdout}");
}
