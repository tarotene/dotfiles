//! main-checkout-guard — 本物の checkout(main worktree)を変更させない
//! PreToolUse / Stop hook の判定エンジン。設計と根拠:
//! docs/claude/main-checkout-guard.md。
//!
//! 2 段で守る:
//!
//! 1. **PreToolUse**: 本物の checkout への Write/Edit/NotebookEdit と、変更系の
//!    git(`-C` / `cd` で本物の checkout を指すもの)を deny する。Read/Grep/Glob
//!    と読み取り系の Bash は通すが、**触れたこと**を baseline として記録する。
//! 2. **Stop**: 記録した checkout だけを再確認し、触れた時点でクリーンな
//!    default branch にいたものがそこから動いていれば block する。Bash の
//!    `sed -i` やリダイレクトなど、字面からは書き込みと分からない迂回(#661)を
//!    捕まえる。
//!
//! 「本物の checkout」は `git rev-parse` の `--git-dir` と `--git-common-dir` が
//! 一致する main worktree。置き場所(`~/.ghr` 等)には依存しない。linked
//! worktree(herdr、`.claude/worktrees/agent-*`)は対象外。

use hook_io::git::default_branch;
use hook_io::shell::split_segments;
use hook_io::SessionLedger;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `tool_input` から取り出した、パスの種別ごとの集合(git は呼ばない純粋な解析結果)。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    /// 変更しようとしている場所。本物の checkout の中なら deny。
    pub mutations: Vec<PathBuf>,
    /// 触れるだけの場所。本物の checkout の中なら baseline を記録する。
    pub touches: Vec<PathBuf>,
}

/// ツール呼び出しを解析する。`cwd` は相対パスの基準。
pub fn analyze(tool_name: &str, tool_input: &Value, cwd: &Path) -> Analysis {
    let mut a = Analysis::default();
    let path_of = |key: &str| {
        tool_input
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| resolve(cwd, s))
    };
    match tool_name {
        "Write" | "Edit" | "MultiEdit" => a.mutations.extend(path_of("file_path")),
        "NotebookEdit" => a.mutations.extend(path_of("notebook_path")),
        "Read" => a.touches.extend(path_of("file_path")),
        "Grep" | "Glob" => a
            .touches
            .push(path_of("path").unwrap_or_else(|| cwd.to_path_buf())),
        "Bash" => {
            if let Some(cmd) = tool_input.get("command").and_then(Value::as_str) {
                analyze_bash(cmd, cwd, &mut a);
            }
        }
        _ => {}
    }
    a
}

/// 絶対パス引数として拾う上限。Bash 1 回あたりの git 呼び出し数を抑える。
const MAX_PATH_ARGS: usize = 8;

fn analyze_bash(cmd: &str, cwd: &Path, a: &mut Analysis) {
    let mut cur = cwd.to_path_buf();
    for seg in split_segments(cmd) {
        // 展開などで静的に語分割できない文は、粗い空白分割で見る。
        let words: Vec<String> = seg
            .words
            .unwrap_or_else(|| seg.text.split_whitespace().map(str::to_string).collect());
        let words = strip_env_assignments(&words);
        let Some(first) = words.first() else {
            continue;
        };
        match first.as_str() {
            "cd" => {
                if let Some(p) = words.get(1) {
                    cur = resolve(&cur, p);
                }
            }
            "git" => match parse_git(words, &cur) {
                Some(g) if is_mutating(&g.sub, &g.args) => a.mutations.push(g.target),
                Some(g) => a.touches.push(g.target),
                None => a.touches.push(cur.clone()),
            },
            _ => {
                a.touches.push(cur.clone());
                a.touches.extend(
                    words[1..]
                        .iter()
                        .filter(|w| w.starts_with('/') || w.starts_with("~/"))
                        .take(MAX_PATH_ARGS)
                        .map(|w| resolve(&cur, w)),
                );
            }
        }
    }
}

/// 先頭の `VAR=value` を読み飛ばす。
fn strip_env_assignments(words: &[String]) -> &[String] {
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

struct GitCall {
    /// `-C` / `--work-tree` を畳み込んだ、git が動く場所。
    target: PathBuf,
    sub: String,
    args: Vec<String>,
}

/// `git [global-opts] <sub> <args…>` を解釈する。サブコマンドが無ければ `None`。
fn parse_git(words: &[String], cwd: &Path) -> Option<GitCall> {
    let mut target = cwd.to_path_buf();
    let mut i = 1;
    while i < words.len() {
        let w = words[i].as_str();
        match w {
            "-C" => {
                target = resolve(&target, words.get(i + 1)?);
                i += 2;
            }
            "--work-tree" => {
                target = resolve(&target, words.get(i + 1)?);
                i += 2;
            }
            "-c" | "--git-dir" | "--namespace" | "--exec-path" | "--super-prefix" => i += 2,
            _ if w.starts_with("--work-tree=") => {
                target = resolve(&target, &w["--work-tree=".len()..]);
                i += 1;
            }
            _ if w.starts_with('-') => i += 1,
            _ => {
                return Some(GitCall {
                    target,
                    sub: w.to_string(),
                    args: words[i + 1..].to_vec(),
                })
            }
        }
    }
    None
}

/// HEAD・ブランチ・index・作業ツリーを動かす git か。読み取り系、`fetch`、
/// `pull --ff-only`、`worktree list|prune` は通す。
pub fn is_mutating(sub: &str, args: &[String]) -> bool {
    let has = |flags: &[&str]| args.iter().any(|a| flags.contains(&a.as_str()));
    match sub {
        "switch" | "checkout" | "commit" | "reset" | "merge" | "rebase" | "cherry-pick"
        | "revert" | "add" | "rm" | "mv" | "restore" | "am" | "apply" | "clean" => true,
        "stash" => !matches!(args.first().map(String::as_str), Some("list" | "show")),
        "pull" => !has(&["--ff-only"]),
        "branch" => {
            let mutating_flag = args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "-m" | "-M"
                        | "-d"
                        | "-D"
                        | "-c"
                        | "-C"
                        | "-f"
                        | "-u"
                        | "-t"
                        | "--move"
                        | "--delete"
                        | "--copy"
                        | "--force"
                        | "--track"
                        | "--edit-description"
                ) || a.starts_with("--set-upstream-to")
                    || a == "--unset-upstream"
            });
            let list_flag = has(&[
                "-l",
                "--list",
                "-a",
                "--all",
                "-r",
                "--remotes",
                "-v",
                "-vv",
                "--show-current",
                "--contains",
                "--merged",
                "--no-merged",
            ]);
            let positional = args.iter().any(|a| !a.starts_with('-'));
            mutating_flag || (positional && !list_flag)
        }
        _ => false,
    }
}

fn resolve(base: &Path, p: &str) -> PathBuf {
    if p == "~" || p.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(p.trim_start_matches('~').trim_start_matches('/'));
        }
    }
    let pb = Path::new(p);
    if pb.is_absolute() {
        pb.to_path_buf()
    } else {
        base.join(pb)
    }
}

/// 実在する最も近い祖先ディレクトリ(ファイルならその親)。
fn nearest_dir(path: &Path) -> Option<PathBuf> {
    let mut p = path;
    loop {
        if p.is_dir() {
            return Some(p.to_path_buf());
        }
        p = p.parent()?;
    }
}

fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8(out.stdout)
            .ok()?
            .trim_end_matches(['\n', '\r'])
            .to_string(),
    )
}

fn same_path(a: &str, b: &str) -> bool {
    let canon = |s: &str| std::fs::canonicalize(s).unwrap_or_else(|_| PathBuf::from(s));
    canon(a) == canon(b)
}

/// `path` が本物の checkout の中なら、その作業ツリーの先頭を返す。
/// git の外・bare・linked worktree は `None`。
pub fn main_checkout_of(path: &Path) -> Option<PathBuf> {
    let dir = nearest_dir(path)?;
    let out = git_out(
        &dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
            "--show-toplevel",
        ],
    )?;
    let mut lines = out.lines();
    let (git_dir, common, top) = (lines.next()?, lines.next()?, lines.next()?);
    same_path(git_dir, common).then(|| PathBuf::from(top))
}

/// checkout の状態の写し。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// 現在のブランチ名。detached は `-`。
    pub branch: String,
    pub sha: String,
    /// `git status --porcelain=v2` の FNV-1a。
    pub status_hash: u64,
    /// クリーンかつ default branch にいる。
    pub pristine: bool,
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn snapshot(top: &Path) -> Option<Snapshot> {
    let status = git_out(top, &["status", "--porcelain=v2"])?;
    let branch = git_out(top, &["symbolic-ref", "--short", "-q", "HEAD"])
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "-".to_string());
    let sha = git_out(top, &["rev-parse", "HEAD"]).unwrap_or_else(|| "-".to_string());
    let on_default = match default_branch(top) {
        Some(d) => branch == d,
        None => branch == "main" || branch == "master",
    };
    Some(Snapshot {
        pristine: status.is_empty() && on_default,
        status_hash: fnv1a(&status),
        branch,
        sha,
    })
}

/// 台帳 1 行 = `top\tbranch\tsha\tstatus_hash\tpristine`。
fn encode(top: &Path, s: &Snapshot) -> Option<String> {
    let top = top.to_str()?;
    if top.contains(['\t', '\n', '\r']) || s.branch.contains(['\t', '\n', '\r']) {
        return None;
    }
    Some(format!(
        "{top}\t{}\t{}\t{}\t{}",
        s.branch,
        s.sha,
        s.status_hash,
        u8::from(s.pristine)
    ))
}

fn decode(line: &str) -> Option<(PathBuf, Snapshot)> {
    let mut f = line.splitn(5, '\t');
    let top = PathBuf::from(f.next()?);
    let branch = f.next()?.to_string();
    let sha = f.next()?.to_string();
    let status_hash = f.next()?.parse().ok()?;
    let pristine = f.next()? == "1";
    Some((
        top,
        Snapshot {
            branch,
            sha,
            status_hash,
            pristine,
        },
    ))
}

/// baseline の置き場所。`MAIN_CHECKOUT_GUARD_STATE_DIR` で差し替えられる(テスト用)。
pub fn ledger() -> Option<SessionLedger> {
    let dir = match std::env::var_os("MAIN_CHECKOUT_GUARD_STATE_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/state/claude/main-checkout"),
    };
    Some(SessionLedger::new(dir, "baseline"))
}

/// このセッションで `top` に初めて触れたなら baseline を記録する。
/// 触れた時点で既に崩れていた(dirty、または default branch 以外)ときは
/// 注意文を返す — block はしない(人の作業や別セッションの残骸をこの
/// セッションの責任にしない)。
pub fn record_touch(ledger: &SessionLedger, session_id: &str, top: &Path) -> Option<String> {
    let already = ledger
        .records(session_id)
        .iter()
        .filter_map(|l| decode(l))
        .any(|(t, _)| t == top);
    if already {
        return None;
    }
    let snap = snapshot(top)?;
    ledger.append(session_id, &encode(top, &snap)?).ok()?;
    (!snap.pristine).then(|| {
        format!(
            "{} is a main checkout that is not clean on the default branch (branch: {}). \
             Do not start a new unit of work here; create a worktree with \
             `herdr worktree create --cwd {} --branch <name>` and work there.",
            top.display(),
            snap.branch,
            top.display(),
        )
    })
}

/// Stop 時の再確認。baseline が pristine だった checkout が動いていれば、
/// その説明を返す。報告済みの checkout は baseline を現在の状態へ更新して
/// 次のターンで繰り返さない。
pub fn check_drift(ledger: &SessionLedger, session_id: &str) -> Vec<String> {
    let mut latest: Vec<(PathBuf, Snapshot)> = Vec::new();
    for (top, snap) in ledger.records(session_id).iter().filter_map(|l| decode(l)) {
        match latest.iter_mut().find(|(t, _)| *t == top) {
            Some(slot) => slot.1 = snap,
            None => latest.push((top, snap)),
        }
    }
    let mut problems = Vec::new();
    for (top, base) in latest {
        if !base.pristine {
            continue;
        }
        let Some(now) = snapshot(&top) else {
            continue;
        };
        let mut what = Vec::new();
        if now.branch != base.branch {
            what.push(format!("branch {} -> {}", base.branch, now.branch));
        }
        if now.status_hash != base.status_hash {
            what.push("working tree or index changed".to_string());
        }
        if now.sha != base.sha
            && git_out(&top, &["merge-base", "--is-ancestor", &base.sha, &now.sha]).is_none()
        {
            what.push(format!(
                "HEAD {} -> {} (not a fast-forward)",
                base.sha, now.sha
            ));
        }
        if what.is_empty() {
            continue;
        }
        problems.push(format!("{}: {}", top.display(), what.join("; ")));
        if let Some(line) = encode(&top, &now) {
            let _ = ledger.append(session_id, &line);
        }
    }
    problems
}

pub fn deny_reason(top: &Path) -> String {
    format!(
        "{} は本物の checkout(main worktree)です。ここは変更せず、作業単位は worktree で切ってください: \
         `herdr worktree create --cwd {} --branch <name>` で作り、以降は worktree の絶対パス、または \
         `git -C <worktree>` で操作します。本物の checkout を直接変更したいときは、本人が `!` 付きのコマンドで \
         操作します。詳細: docs/claude/main-checkout-guard.md",
        top.display(),
        top.display(),
    )
}

pub fn drift_reason(problems: &[String]) -> String {
    format!(
        "A main checkout this session touched moved away from a clean default branch:\n{}\n\
         Do not try to repair it yourself (changes to a main checkout are denied). \
         Tell the user exactly what changed so they can inspect and restore it with `!`-prefixed commands, \
         and continue any further work in a worktree (`herdr worktree create --cwd <repo> --branch <name>`). \
         See docs/claude/main-checkout-guard.md.",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash(cmd: &str) -> Analysis {
        analyze("Bash", &json!({ "command": cmd }), Path::new("/w"))
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn write_tools_are_mutations_and_read_tools_are_touches() {
        let a = analyze("Edit", &json!({"file_path": "/r/a.rs"}), Path::new("/w"));
        assert_eq!(a.mutations, vec![PathBuf::from("/r/a.rs")]);
        let a = analyze(
            "NotebookEdit",
            &json!({"notebook_path": "n.ipynb"}),
            Path::new("/w"),
        );
        assert_eq!(a.mutations, vec![PathBuf::from("/w/n.ipynb")]);
        let a = analyze("Read", &json!({"file_path": "/r/a.rs"}), Path::new("/w"));
        assert_eq!(a.touches, vec![PathBuf::from("/r/a.rs")]);
        assert!(a.mutations.is_empty());
        let a = analyze("Grep", &json!({"pattern": "x"}), Path::new("/w"));
        assert_eq!(a.touches, vec![PathBuf::from("/w")]);
    }

    #[test]
    fn git_c_and_cd_redirect_the_target() {
        assert_eq!(
            bash("git -C /r switch -c x").mutations,
            vec![PathBuf::from("/r")]
        );
        assert_eq!(
            bash("git -C sub commit -m m").mutations,
            vec![PathBuf::from("/w/sub")]
        );
        assert_eq!(
            bash("cd /r && git add .").mutations,
            vec![PathBuf::from("/r")]
        );
        assert_eq!(
            bash("git -c user.name=x --no-pager commit").mutations,
            vec![PathBuf::from("/w")]
        );
    }

    #[test]
    fn read_only_git_is_a_touch() {
        for cmd in [
            "git -C /r log --oneline",
            "git -C /r status",
            "git -C /r fetch origin",
            "git -C /r pull --ff-only",
            "git -C /r branch --list 'x*'",
            "git -C /r stash list",
            "git -C /r worktree list",
        ] {
            let a = bash(cmd);
            assert!(a.mutations.is_empty(), "{cmd}");
            assert_eq!(a.touches, vec![PathBuf::from("/r")], "{cmd}");
        }
    }

    #[test]
    fn compound_and_tab_separated_commands_are_all_seen() {
        let a = bash("echo hi && git\t-C\t/r\tcommit -m m; ls");
        assert_eq!(a.mutations, vec![PathBuf::from("/r")]);
        let a = bash("FOO=1 git -C /r reset --hard");
        assert_eq!(a.mutations, vec![PathBuf::from("/r")]);
    }

    #[test]
    fn non_git_commands_touch_cwd_and_absolute_args() {
        let a = bash("sed -i s/a/b/ /r/x.txt");
        assert_eq!(
            a.touches,
            vec![PathBuf::from("/w"), PathBuf::from("/r/x.txt")]
        );
        assert!(a.mutations.is_empty());
    }

    #[test]
    fn mutating_classification() {
        assert!(is_mutating("checkout", &s(&["-b", "x"])));
        assert!(is_mutating("pull", &s(&[])));
        assert!(!is_mutating("pull", &s(&["--ff-only"])));
        assert!(is_mutating("stash", &s(&[])));
        assert!(is_mutating("stash", &s(&["pop"])));
        assert!(!is_mutating("stash", &s(&["show"])));
        assert!(is_mutating("branch", &s(&["newbranch"])));
        assert!(is_mutating("branch", &s(&["-D", "old"])));
        assert!(!is_mutating("branch", &s(&[])));
        assert!(!is_mutating("branch", &s(&["--show-current"])));
        assert!(!is_mutating("log", &s(&[])));
        assert!(!is_mutating("fetch", &s(&[])));
        assert!(!is_mutating("worktree", &s(&["prune"])));
    }

    #[test]
    fn ledger_line_roundtrip() {
        let snap = Snapshot {
            branch: "main".into(),
            sha: "abc".into(),
            status_hash: 42,
            pristine: true,
        };
        let line = encode(Path::new("/r"), &snap).unwrap();
        assert_eq!(decode(&line), Some((PathBuf::from("/r"), snap)));
        assert!(encode(
            Path::new("/r\tx"),
            &Snapshot {
                branch: "m".into(),
                sha: "a".into(),
                status_hash: 0,
                pristine: true
            }
        )
        .is_none());
    }
}
