//! main-checkout-guard — 本物の checkout(main worktree)を変更させない
//! PreToolUse / Stop hook の判定エンジン。設計と根拠:
//! docs/agents/main-checkout-guard.md。
//!
//! 2 段で守る:
//!
//! 1. **PreToolUse**: 本物の checkout への Write/Edit/NotebookEdit と、変更系の
//!    git(`-C` / `cd` で本物の checkout を指すもの)を deny する。Read/Grep/Glob
//!    と読み取り系の Bash は通すが、**触れたこと**を baseline として記録する。
//! 2. **Stop**: 記録した checkout だけを再確認し、触れた時点でクリーンな
//!    default branch にいたものがそこから動いていれば block する。Bash の
//!    `sed -i` やリダイレクトなど、字面からは書き込みと分からない迂回(#661)を
//!    捕まえる。ただし block するのは、このセッションが**書きうる操作**(読み取り
//!    専用の許可リストに無い Bash)で触れた checkout だけ。読み取りだけで触れた
//!    checkout の移動は、別セッションや人の操作の可能性が高く、このセッションの
//!    変更として報告しない(注意書きだけ。#898)。
//!
//! 「本物の checkout」は `git rev-parse` の `--git-dir` と `--git-common-dir` が
//! 一致する main worktree。置き場所(`~/.ghr` 等)には依存しない。linked
//! worktree(herdr、`.claude/worktrees/agent-*`)は対象外。

use agent_session::SessionLedger;
use command_inspection::simple_shell::split_segments;
use git_repository::default_branch;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// `tool_input` から取り出した、パスの種別ごとの集合(git は呼ばない純粋な解析結果)。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    /// 変更しようとしている場所。本物の checkout の中なら deny。
    pub mutations: Vec<PathBuf>,
    /// 触れる場所(読み取りも含む全部)。本物の checkout の中なら baseline を記録する。
    pub touches: Vec<PathBuf>,
    /// `touches` のうち、書きうる操作で触れた場所(読み取り専用の許可リストに
    /// 無い Bash、リダイレクトを伴う文)。Stop が block してよいのは、この集合に
    /// 入った checkout だけ(#898)。読み取りは [`is_read_only_command`] と
    /// [`is_read_only_git`] の許可リストで決める。
    pub may_write: Vec<PathBuf>,
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
    let cmd = strip_benign_redirects(cmd);
    // ファイルへ書き出しうるリダイレクト(`>`・`>>`・`&>`)が 1 つでもあれば、
    // 全文を「書きうる」側に倒す。引用符の中の `>` も区別しない(疑わしきは従来
    // どおり block する。Stop の絞り込みは、原因を決めつけないための例外)。
    let writes_files = cmd.contains('>');
    for seg in split_segments(&cmd) {
        // 展開などで静的に語分割できない文は、粗い空白分割で見る。
        let seg_text = seg.text;
        let words: Vec<String> = seg
            .words
            .unwrap_or_else(|| seg_text.split_whitespace().map(str::to_string).collect());
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
                Some(g) => {
                    let read_only = !writes_files && is_read_only_git(&g.sub, &g.args);
                    a.touches.push(g.target.clone());
                    if !read_only {
                        a.may_write.push(g.target);
                    }
                }
                None => {
                    a.touches.push(cur.clone());
                    a.may_write.push(cur.clone());
                }
            },
            _ => {
                let read_only = !writes_files && is_read_only_command(words);
                let mut targets = vec![cur.clone()];
                targets.extend(
                    words[1..]
                        .iter()
                        .filter(|w| w.starts_with('/') || w.starts_with("~/"))
                        .take(MAX_PATH_ARGS)
                        .map(|w| resolve(&cur, w)),
                );
                a.touches.extend(targets.iter().cloned());
                if !read_only {
                    a.may_write.extend(targets);
                }
            }
        }
    }
}

/// 書き込みでないリダイレクト(`2>&1`・`>/dev/null` など)を空白に置き換える。
/// `split_segments` は `&` で文を区切るので、`2>&1` を残すと 2 文に割れて
/// 「リダイレクトを含む文」の判定が崩れる。区切る前に除く。
fn strip_benign_redirects(cmd: &str) -> String {
    let mut t = cmd.to_string();
    for benign in [
        "&>/dev/null",
        "&> /dev/null",
        "2>&1",
        "1>&2",
        ">&2",
        "2>/dev/null",
        "2> /dev/null",
        "1>/dev/null",
        ">/dev/null",
        "> /dev/null",
    ] {
        t = t.replace(benign, " ");
    }
    t
}

/// 読み取り専用と見なす Bash コマンド(先頭語)。ファイル・checkout を変えない
/// ものだけを明示する許可リストで、ここに無いものは「書きうる」(#898)。
/// 引数で書き込みに転じるもの(`find -delete`・`sort -o`・`jq` の出力先など)は
/// [`is_read_only_command`] が個別に外す。`sed`・`awk`・`yq`・`tee`・`xargs`・
/// `gh`・`cargo`・`make` などは、書き込みうるので載せない。
const READ_ONLY_COMMANDS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "bat", "nl", "tac", "grep", "egrep", "fgrep", "rg",
    "ag", "ls", "eza", "tree", "stat", "file", "wc", "du", "df", "diff", "cmp", "readlink",
    "realpath", "basename", "dirname", "pwd", "echo", "printf", "true", "false", "test", "[",
    "date", "hostname", "which", "whoami", "jq", "sort", "uniq", "cut", "tr", "column", "find",
    "fd",
];

/// `words`(先頭の `VAR=value` は除去済み)が読み取り専用の Bash コマンドか。
pub fn is_read_only_command(words: &[String]) -> bool {
    let Some(first) = words.first() else {
        return false;
    };
    // `/bin/cat` のような絶対パス指定も同じコマンドとして見る。
    let name = first.rsplit('/').next().unwrap_or(first);
    if !READ_ONLY_COMMANDS.contains(&name) {
        return false;
    }
    let has_arg = |f: &dyn Fn(&str) -> bool| words[1..].iter().any(|a| f(a));
    match name {
        // 削除・実行・ファイル出力に転じる find / fd のフラグ。
        "find" => !has_arg(&|a| {
            matches!(
                a,
                "-delete"
                    | "-exec"
                    | "-execdir"
                    | "-ok"
                    | "-okdir"
                    | "-fprint"
                    | "-fprint0"
                    | "-fprintf"
                    | "-fls"
            )
        }),
        "fd" => !has_arg(&|a| matches!(a, "-x" | "--exec" | "-X" | "--exec-batch")),
        "sort" => !has_arg(&|a| a == "-o" || a.starts_with("--output") || a.starts_with("-o")),
        _ => true,
    }
}

/// 読み取り専用と見なす git のサブコマンド。`is_mutating` が false でも、ここに
/// 無いもの(`gc`・`tag`・`push`・`config` の書き込み形など)は「書きうる」。
/// `fetch` は remote-tracking ref を動かすだけで HEAD・作業ツリーに触れない
/// ので読み取りとして扱う(`pull` は HEAD を進めるので含めない)。
pub fn is_read_only_git(sub: &str, args: &[String]) -> bool {
    let first = args.first().map(String::as_str);
    match sub {
        "log" | "show" | "diff" | "status" | "ls-files" | "ls-tree" | "ls-remote" | "rev-parse"
        | "rev-list" | "cat-file" | "blame" | "grep" | "describe" | "shortlog" | "show-ref"
        | "show-branch" | "for-each-ref" | "name-rev" | "merge-base" | "diff-tree"
        | "diff-index" | "diff-files" | "whatchanged" | "check-ignore" | "count-objects"
        | "verify-commit" | "fetch" | "version" => true,
        "reflog" => matches!(first, None | Some("show")),
        "stash" => matches!(first, Some("list" | "show")),
        "worktree" => matches!(first, Some("list")),
        "branch" => !is_mutating(sub, args),
        "remote" => matches!(first, None | Some("-v" | "show" | "get-url")),
        "config" => args.iter().any(|a| {
            matches!(
                a.as_str(),
                "--get" | "--get-all" | "--get-regexp" | "--list" | "-l"
            )
        }),
        "tag" => args.iter().any(|a| matches!(a.as_str(), "-l" | "--list")),
        _ => false,
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

/// 成功時の stdout。UTF-8 として読めなければ `None`、末尾の `\n` / `\r` は落とす。
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let s = String::from_utf8(git_repository::raw(dir, args)?).ok()?;
    Some(s.trim_end_matches(['\n', '\r']).to_string())
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

/// 台帳 1 行 = `top\tbranch\tsha\tstatus_hash\tpristine\tmay_write`。
///
/// 6 列目 `may_write` は、このセッションが**書きうる操作**でその checkout に
/// 触れたか(#898)。旧い書式の行(5 列)にはこの列が無く、`decode` は `true`
/// (従来どおり block の対象)として読む。
fn encode(top: &Path, s: &Snapshot, may_write: bool) -> Option<String> {
    let top = top.to_str()?;
    if top.contains(['\t', '\n', '\r']) || s.branch.contains(['\t', '\n', '\r']) {
        return None;
    }
    Some(format!(
        "{top}\t{}\t{}\t{}\t{}\t{}",
        s.branch,
        s.sha,
        s.status_hash,
        u8::from(s.pristine),
        u8::from(may_write)
    ))
}

/// 台帳の 1 行を読んだ結果。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    top: PathBuf,
    snap: Snapshot,
    may_write: bool,
}

fn decode(line: &str) -> Option<Entry> {
    let mut f = line.splitn(6, '\t');
    let top = PathBuf::from(f.next()?);
    let branch = f.next()?.to_string();
    let sha = f.next()?.to_string();
    let status_hash = f.next()?.parse().ok()?;
    let pristine = f.next()? == "1";
    // 列が無い旧い書式は「書きうる」として読む(従来の挙動を保つ)。
    let may_write = f.next().is_none_or(|v| v != "0");
    Some(Entry {
        top,
        snap: Snapshot {
            branch,
            sha,
            status_hash,
            pristine,
        },
        may_write,
    })
}

/// 台帳を `top` ごとの最新の行に畳む(出現順を保つ)。
fn latest_entries(ledger: &SessionLedger, session_id: &str) -> Vec<Entry> {
    let mut latest: Vec<Entry> = Vec::new();
    for e in ledger.records(session_id).iter().filter_map(|l| decode(l)) {
        match latest.iter_mut().find(|x| x.top == e.top) {
            Some(slot) => *slot = e,
            None => latest.push(e),
        }
    }
    latest
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
///
/// `may_write` は、この触れ方が書きうる操作か。読み取りだけで記録した
/// checkout に、後から書きうる操作で触れたときは、**その時点の状態**で
/// baseline を取り直す(書きうる操作の前の状態が、Stop の比較の基準になる。
/// それ以前の移動はこのセッションの書き込みの結果ではない)。
pub fn record_touch(
    ledger: &SessionLedger,
    session_id: &str,
    top: &Path,
    may_write: bool,
) -> Option<String> {
    let known = latest_entries(ledger, session_id)
        .into_iter()
        .find(|e| e.top == top);
    match known {
        // 既に書きうる操作で記録済み、または今回も読み取りだけ。
        Some(e) if e.may_write || !may_write => return None,
        // 読み取りだけで記録した checkout に、初めて書きうる操作で触れた。
        Some(_) => {
            let snap = snapshot(top)?;
            ledger.append(session_id, &encode(top, &snap, true)?).ok()?;
            return None;
        }
        None => {}
    }
    let snap = snapshot(top)?;
    ledger
        .append(session_id, &encode(top, &snap, may_write)?)
        .ok()?;
    // `origin/HEAD` が無く(既定ブランチが分からない)、main / master でもない所に
    // いる checkout は、使い捨ての clone(例: `git clone --branch gh-pages` で
    // 読むだけ)とみなし、助言を出さない(#845)。助言の前提は「既定ブランチで
    // clean でない親 checkout から worktree を切ると、土台がずれる」ことで、
    // `origin/<default>` が参照できない clone は herdr の土台になり得ない。
    // deny は変わらない(助言の対象外)。
    let scratch = default_branch(top).is_none() && snap.branch != "main" && snap.branch != "master";
    (!snap.pristine && !scratch).then(|| {
        format!(
            "{} is a main checkout that is not clean on the default branch (branch: {}). \
             Do not start a new unit of work here; create a worktree with \
             `herdr worktree create --cwd {} --branch <name>` and work there. The worktree is based on the \
             parent checkout's HEAD, so if the parent is not on the default branch, check \
             after creating it: \
             `git merge-base --is-ancestor origin/<default> HEAD`, and align it with `origin/<default>` if it fails.",
            top.display(),
            snap.branch,
            top.display(),
        )
    })
}

/// Stop 時の再確認の結果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Drift {
    /// このセッションが書きうる操作で触れた checkout の移動。Stop を block する。
    pub blocking: Vec<String>,
    /// 読み取りだけで触れた checkout の移動。原因はこのセッションとは限らない
    /// (別セッション・人の操作)ので、block せず注意書きだけにする(#898)。
    pub notices: Vec<String>,
}

/// baseline からの変化の説明。変化が無ければ空。
fn describe_change(top: &Path, base: &Snapshot, now: &Snapshot) -> Vec<String> {
    let mut what = Vec::new();
    if now.branch != base.branch {
        what.push(format!("branch {} -> {}", base.branch, now.branch));
    }
    if now.status_hash != base.status_hash {
        what.push("working tree or index changed".to_string());
    }
    if now.sha != base.sha
        && git_out(top, &["merge-base", "--is-ancestor", &base.sha, &now.sha]).is_none()
    {
        what.push(format!(
            "HEAD {} -> {} (not a fast-forward)",
            base.sha, now.sha
        ));
    }
    what
}

/// Stop 時の再確認。baseline が pristine だった checkout が動いていれば、
/// その説明を返す。書きうる操作で触れた checkout の移動は `blocking`、読み取り
/// だけで触れた checkout の移動は `notices`。報告済みの checkout は baseline を
/// 現在の状態へ更新して次のターンで繰り返さない。
pub fn check_drift(ledger: &SessionLedger, session_id: &str) -> Drift {
    let mut drift = Drift::default();
    for Entry {
        top,
        snap: base,
        may_write,
    } in latest_entries(ledger, session_id)
    {
        if !base.pristine {
            continue;
        }
        let Some(now) = snapshot(&top) else {
            continue;
        };
        let what = describe_change(&top, &base, &now);
        if what.is_empty() {
            continue;
        }
        let line = format!("{}: {}", top.display(), what.join("; "));
        if may_write {
            drift.blocking.push(line);
        } else {
            drift.notices.push(line);
        }
        if let Some(line) = encode(&top, &now, may_write) {
            let _ = ledger.append(session_id, &line);
        }
    }
    drift
}

pub fn deny_reason(top: &Path) -> String {
    format!(
        "{} は本物の checkout(main worktree)です。ここは変更せず、作業単位は worktree で切ってください: \
         `herdr worktree create --cwd {} --branch <name>` で作り、以降は worktree の絶対パス、または \
         `git -C <worktree>` で操作します。worktree の土台は親 checkout の HEAD なので、親が default ブランチでない\
         ときは作成後に `git merge-base --is-ancestor origin/<default> HEAD` で確かめ、外れていれば \
         `origin/<default>` に揃えます。本物の checkout を直接変更したいときは、本人が `!` 付きのコマンドで \
         操作します。詳細: docs/agents/main-checkout-guard.md",
        top.display(),
        top.display(),
    )
}

pub fn drift_reason(problems: &[String]) -> String {
    format!(
        "A main checkout this session may have written to moved away from a clean default branch:\n{}\n\
         Do not try to repair it yourself (changes to a main checkout are denied). \
         Tell the user exactly what changed so they can inspect and restore it with `!`-prefixed commands, \
         and continue any further work in a worktree (`herdr worktree create --cwd <repo> --branch <name>`). \
         See docs/agents/main-checkout-guard.md.",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Stop を block しない注意書き(読み取りだけで触れた checkout の移動)。ユーザーに
/// だけ見せる(`systemMessage`)。モデルへ報告・復元を促さない(#898)。
pub fn notice_text(notices: &[String]) -> String {
    format!(
        "main-checkout-guard: a main checkout this session only read moved (cause unknown, \
         likely another session or a manual operation; not reported as this session's change):\n{}",
        notices
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// PreToolUse の本体(`pre` サブコマンド)。mutate が本物の checkout に及べば deny、そうでなければ
/// 触った本物の checkout を台帳に記録し、初めて触ったときの注意を `Context` で返す
/// (PreToolUse の additionalContext は Claude Code のみ。Codex には出さない)。
///
/// `main` と、ルータ(`crates/hook-router`)が同じ関数を呼ぶ。
pub fn pre_hook(
    agent: agent_session::Agent,
    input: &agent_session::HookInput,
) -> Option<agent_session::Outcome> {
    let cwd = input.cwd.clone().or_else(|| std::env::current_dir().ok())?;
    let analysis = analyze(&input.tool_name, &input.tool_input, &cwd);

    for p in &analysis.mutations {
        if let Some(top) = main_checkout_of(p) {
            return Some(agent_session::Outcome::Deny(deny_reason(&top)));
        }
    }

    let ledger = ledger()?;
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut notices = Vec::new();
    for p in &analysis.touches {
        let Some(top) = main_checkout_of(p) else {
            continue;
        };
        if seen.contains(&top) {
            continue;
        }
        // 同じ checkout を指す touch のうち 1 つでも書きうる操作なら、書きうるものとして扱う。
        let may_write = analysis
            .may_write
            .iter()
            .any(|w| main_checkout_of(w).as_ref() == Some(&top));
        if let Some(n) = record_touch(&ledger, &input.session_id, &top, may_write) {
            notices.push(n);
        }
        seen.push(top);
    }
    (!notices.is_empty() && agent == agent_session::Agent::Claude)
        .then(|| agent_session::Outcome::Context(notices.join("\n")))
}

/// [`pre_hook`] を、生の入力(標準入力の JSON)から呼ぶ。Claude 向け(ルータ用)。
pub fn hook(raw: &str) -> Option<agent_session::Outcome> {
    let input = agent_session::HookInput::parse(raw)?;
    pre_hook(agent_session::Agent::Claude, &input)
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
        for may_write in [true, false] {
            let line = encode(Path::new("/r"), &snap, may_write).unwrap();
            assert_eq!(
                decode(&line),
                Some(Entry {
                    top: PathBuf::from("/r"),
                    snap: snap.clone(),
                    may_write
                })
            );
        }
        assert!(encode(
            Path::new("/r\tx"),
            &Snapshot {
                branch: "m".into(),
                sha: "a".into(),
                status_hash: 0,
                pristine: true
            },
            true
        )
        .is_none());
    }

    /// 旧い書式(5 列。`may_write` 列が無い)の行は、従来どおり block の対象
    /// (`may_write = true`)として読む。
    #[test]
    fn legacy_ledger_line_without_may_write_column_reads_as_may_write() {
        let e = decode("/r\tmain\tabc\t42\t1").unwrap();
        assert!(e.may_write);
        assert!(e.snap.pristine);
        assert_eq!(e.top, PathBuf::from("/r"));
        // 列が `0` のときだけ読み取り専用。
        assert!(!decode("/r\tmain\tabc\t42\t1\t0").unwrap().may_write);
        assert!(decode("/r\tmain\tabc\t42\t1\t1").unwrap().may_write);
    }

    #[test]
    fn read_only_bash_is_a_touch_but_not_may_write() {
        for cmd in [
            "cat /r/a.txt",
            "grep -rn foo /r/src",
            "ls -la /r",
            "head -n 3 /r/x 2>/dev/null",
            "git -C /r log --oneline",
            "git -C /r status",
            "git -C /r fetch origin",
            "git -C /r branch --list",
            "git -C /r stash list",
            "git -C /r worktree list",
            "find /r -name '*.rs'",
            "cd /r && git log -1 && cat a.txt",
        ] {
            let a = bash(cmd);
            assert!(a.mutations.is_empty(), "{cmd}");
            assert!(!a.touches.is_empty(), "{cmd}");
            assert!(a.may_write.is_empty(), "{cmd}: {:?}", a.may_write);
        }
    }

    #[test]
    fn unlisted_or_redirecting_bash_may_write() {
        for cmd in [
            "sed -i s/a/b/ /r/x.txt",
            "cat /r/a.txt > /r/b.txt",
            "echo x >> /r/log",
            "ls /r | tee /r/out",
            "git -C /r pull --ff-only",
            "git -C /r gc",
            "git -C /r config user.name x",
            "find /r -name '*.tmp' -delete",
            "sort -o /r/x /r/y",
            "touch /r/new",
            "rm -rf /r/x",
        ] {
            let a = bash(cmd);
            assert!(a.mutations.is_empty(), "{cmd}");
            assert!(
                a.may_write
                    .iter()
                    .any(|p| p.starts_with("/r") || p == Path::new("/w")),
                "{cmd}: {:?}",
                a.may_write
            );
        }
        // 書き込まないリダイレクト(`2>&1`、`/dev/null`)は読み取りのまま。
        assert!(bash("ls /r 2>&1").may_write.is_empty());
        assert!(bash("ls /r > /dev/null").may_write.is_empty());
    }

    #[test]
    fn read_only_command_allowlist_boundaries() {
        let w = |c: &str| s(&c.split_whitespace().collect::<Vec<_>>());
        assert!(is_read_only_command(&w("/bin/cat x")));
        assert!(is_read_only_command(&w("find . -name x")));
        assert!(!is_read_only_command(&w("find . -exec rm {} ;")));
        assert!(!is_read_only_command(&w("sed -n 1p x")));
        assert!(!is_read_only_command(&w("rm x")));
        assert!(!is_read_only_command(&[]));
        assert!(is_read_only_git("log", &s(&[])));
        assert!(is_read_only_git("remote", &s(&["-v"])));
        assert!(!is_read_only_git("remote", &s(&["add", "x", "y"])));
        assert!(is_read_only_git("config", &s(&["--get", "user.name"])));
        assert!(!is_read_only_git("config", &s(&["user.name", "x"])));
        assert!(!is_read_only_git("pull", &s(&["--ff-only"])));
        assert!(!is_read_only_git("push", &s(&[])));
    }
}
