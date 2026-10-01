//! `git audit-worktrees` — stale な linked-worktree 登録と、残骸の checkout を
//! ghr / Herdr 配下のリポジトリ横断で検出する(`scripts/git-audit-worktrees`
//! の Rust 移植、ADR-0024)。
//!
//! 検出専用で何も削除しない。`git` / `herdr` / `gh` は外部プロセスとして
//! 呼ぶ(git を再実装しない)。出力(TSV の列・日本語メッセージ・終了コード)と
//! state ファイルは bash 版と byte 互換。
//!
//! 互換のために意図して残している bash 版の癖:
//! - `context` の `\n` は bash のダブルクォート内なので改行ではなく
//!   バックスラッシュ+n の 2 文字として JSON に入る。
//! - `$HERDR_BIN notification show` はクォートされていないので空白で分割される。

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub const USAGE: &str = "\
usage: git audit-worktrees [--notify|--context|--porcelain|--evidence]

Without options, report stale findings and exit 1 when any exist. Two
independent classes are detected:
  prunable  registration survives after the checkout directory disappeared
            (git worktree list --porcelain's own \"prunable\" verdict)
  orphaned  checkout is still present, but looks abandoned: clean, not open
            in Herdr, and either [gone] upstream or never pushed with zero
            commits unique to the repo's default branch
--notify sends one Herdr notification per stale-set fingerprint.
--context emits SessionStart hook JSON.
--porcelain emits the raw class-tagged TSV (for git-prune-worktrees).

--evidence emits a separate TSV of worktrees/branches backed by *content-
preservation* evidence, for unattended deletion (git prune-worktrees --auto /
git prune-branches --auto), not upstream-tracking state:
  C1      prunable registration (checkout already gone — nothing to preserve)
  C2      tip is an ancestor of the repo's default branch (already in main)
  C3:#N   tip equals a MERGED or CLOSED PR's head commit (recoverable via
          refs/pull/N/head even after local deletion)
Rows: kind(worktree|branch)  common  repo  path  branch  sha  evidence
Calls `gh api repos/<slug>/commits/<sha>/pulls` once per candidate sha
(only for --evidence, never for the other modes; #586 — the call count
follows the number of candidates, not the number of PRs in the repo); a
repo whose origin isn't github.com, or whose `gh` call fails, is simply
skipped for C3 (fail closed — never asserted, never guessed).

Detection only, never deletes — for either class, run `git prune-worktrees`
or `git prune-branches`.
";

/// 実行時設定(env override)。
pub struct Config {
    pub ghr_dir: String,
    pub herdr_dir: String,
    pub state_dir: String,
    pub herdr_bin: String,
    pub gh_bin: String,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn home() -> Result<String, String> {
    std::env::var("HOME").map_err(|_| "HOME: unbound variable".to_string())
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let ghr_dir = match env_nonempty("GIT_WORKTREE_AUDIT_GHR_DIR") {
            Some(v) => v,
            None => format!("{}/.ghr", home()?),
        };
        let herdr_dir = match env_nonempty("GIT_WORKTREE_AUDIT_HERDR_DIR") {
            Some(v) => v,
            None => format!("{}/.herdr/worktrees", home()?),
        };
        let state_dir = match env_nonempty("GIT_WORKTREE_AUDIT_STATE_DIR") {
            Some(v) => v,
            None => {
                let base = match env_nonempty("XDG_STATE_HOME") {
                    Some(v) => v,
                    None => format!("{}/.local/state", home()?),
                };
                format!("{base}/git-worktree-audit")
            }
        };
        Ok(Self {
            ghr_dir,
            herdr_dir,
            state_dir,
            herdr_bin: env_nonempty("GIT_WORKTREE_AUDIT_HERDR_BIN")
                .unwrap_or_else(|| "herdr".into()),
            gh_bin: env_nonempty("GIT_WORKTREE_AUDIT_GH_BIN").unwrap_or_else(|| "gh".into()),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Report,
    Notify,
    Context,
    Porcelain,
    Evidence,
}

/// 標準出力へ書く(EPIPE で落ちないよう結果は捨てる)。
fn out(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

fn err(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = e.write_all(s.as_bytes());
    let _ = e.flush();
}

/// エントリポイント。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    let mut mode = Mode::Report;
    for a in args {
        match a.as_str() {
            "--notify" => mode = Mode::Notify,
            "--context" => mode = Mode::Context,
            "--porcelain" => mode = Mode::Porcelain,
            "--evidence" => mode = Mode::Evidence,
            "-h" | "--help" => {
                out(USAGE);
                return 0;
            }
            _ => {
                err(USAGE);
                return 2;
            }
        }
    }
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            err(&format!("git-audit-worktrees: {e}\n"));
            return 1;
        }
    };
    let audit = Audit::new(cfg);
    if mode == Mode::Evidence {
        // --evidence は独自の scan(gh を呼ぶ)。1 分タイマーが通る
        // scan 系は gh を間接的にも呼ばない。
        let rows = audit.evidence();
        for r in rows {
            out(&format!("{r}\n"));
        }
        return 0;
    }
    let findings = audit.scan();
    match mode {
        Mode::Report => {
            if !findings.is_empty() {
                out(&render_report(&findings));
                return 1;
            }
            out("stale worktree はありません。\n");
            0
        }
        Mode::Notify => audit.notify(&findings),
        Mode::Context => context(&findings),
        Mode::Porcelain => {
            for r in &findings {
                out(&format!("{r}\n"));
            }
            0
        }
        Mode::Evidence => 0,
    }
}

// ---------------------------------------------------------------------
// 外部コマンド
// ---------------------------------------------------------------------

/// `git <args>` を実行し、成功時の stdout を返す(stderr は捨てる)。
fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// bash の `$(...)` 相当: 末尾の改行だけ剥がす。
fn chomp(s: &str) -> &str {
    s.trim_end_matches('\n')
}

fn git_gd(common: &str, args: &[&str]) -> Option<String> {
    let gd = format!("--git-dir={common}");
    let mut a = vec![gd.as_str()];
    a.extend_from_slice(args);
    git(&a)
}

fn git_c(dir: &str, args: &[&str]) -> Option<String> {
    let mut a = vec!["-C", dir];
    a.extend_from_slice(args);
    git(&a)
}

fn ok_gd(common: &str, args: &[&str]) -> bool {
    git_gd(common, args).is_some()
}

/// `git --git-dir=<common> worktree list --porcelain` の出力 + "\n" を行に割る
/// (bash の `< <(git ...; printf '\n')` と同じ)。失敗時は空出力扱い。
fn porcelain_lines(common: &str) -> Vec<String> {
    let mut s = git_gd(common, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    s.push('\n');
    s.split('\n').map(str::to_string).collect()
}

// ---------------------------------------------------------------------
// bash 互換の小物
// ---------------------------------------------------------------------

/// `IFS=$'\t' read -r a b c ...` の分割。タブは IFS 空白なので連続タブは
/// 1 つの区切りに潰れ、先頭のタブは捨てられ、最後の変数は残り(末尾タブ除去)。
pub fn ifs_tab_split(line: &str, n: usize) -> Vec<String> {
    let mut rest = line.trim_start_matches('\t');
    let mut fields = Vec::with_capacity(n);
    for i in 0..n {
        if i + 1 == n {
            fields.push(rest.trim_end_matches('\t').to_string());
            rest = "";
        } else if let Some(pos) = rest.find('\t') {
            fields.push(rest[..pos].to_string());
            rest = rest[pos..].trim_start_matches('\t');
        } else {
            fields.push(rest.to_string());
            rest = "";
        }
    }
    fields
}

/// jq の文字列エンコード(DEL と制御文字を `\u00xx`、非 ASCII はそのまま)。
fn json_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                o.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// bash の `${s:0:230}` は locale 依存(UTF-8 なら文字、C なら byte)。
fn locale_is_utf8() -> bool {
    for k in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Some(v) = env_nonempty(k) {
            let v = v.to_ascii_lowercase();
            return v.contains("utf-8") || v.contains("utf8");
        }
    }
    false
}

fn truncate_body(body: &str, n: usize) -> OsString {
    if locale_is_utf8() {
        OsString::from(body.chars().take(n).collect::<String>())
    } else {
        let b = body.as_bytes();
        OsString::from_vec(b[..b.len().min(n)].to_vec())
    }
}

/// `jq -e '.. | objects | select(has(K)) | .K == V'` 相当: 出力の最後が
/// true なら真(jq -e の終了コード 0)。パース失敗は偽。
fn jq_last_is_true(text: &str, key: &str, pred: impl Fn(&serde_json::Value) -> bool) -> bool {
    fn walk(
        v: &serde_json::Value,
        key: &str,
        pred: &dyn Fn(&serde_json::Value) -> bool,
        last: &mut Option<bool>,
    ) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(x) = m.get(key) {
                    *last = Some(pred(x));
                }
                for x in m.values() {
                    walk(x, key, pred, last);
                }
            }
            serde_json::Value::Array(a) => {
                for x in a {
                    walk(x, key, pred, last);
                }
            }
            _ => {}
        }
    }
    let mut last = None;
    for doc in serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>() {
        match doc {
            Ok(v) => walk(&v, key, &pred, &mut last),
            Err(_) => return false,
        }
    }
    last == Some(true)
}

fn lexical_clean(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    format!("/{}", parts.join("/"))
}

/// `realpath -m`: 存在する部分は symlink 解決、無い部分は字句的に正規化。
fn realpath_m(p: &str) -> String {
    if let Ok(c) = fs::canonicalize(p) {
        return c.to_string_lossy().into_owned();
    }
    let cleaned = lexical_clean(p);
    // 存在する最長の祖先を解決してから残りを足す。
    let mut cur = cleaned.as_str();
    let mut tail: Vec<&str> = Vec::new();
    loop {
        if let Ok(c) = fs::canonicalize(cur) {
            let mut base = c.to_string_lossy().into_owned();
            for t in tail.iter().rev() {
                if !base.ends_with('/') {
                    base.push('/');
                }
                base.push_str(t);
            }
            return base;
        }
        match cur.rfind('/') {
            Some(0) | None => return cleaned,
            Some(i) => {
                tail.push(&cur[i + 1..]);
                cur = &cur[..i];
            }
        }
    }
}

// ---------------------------------------------------------------------
// 報告
// ---------------------------------------------------------------------

/// 検出行(common, repo, path, branch, class, reason の TSV)を人間向けに。
pub fn render_report(findings: &[String]) -> String {
    let mut o = String::new();
    let mut count = 0usize;
    for row in findings {
        // 連続タブを潰さない単純な分割。branch 列が空の行(detached の
        // prunable)でも列がずれない(#637)。
        let f: Vec<&str> = row.splitn(6, '\t').collect();
        let [_, repo, path, branch, class, reason] = f[..] else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        count += 1;
        let branch = if branch.is_empty() {
            "(detached)"
        } else {
            branch
        };
        o.push_str(&format!(
            "stale worktree: repo={repo} path={path} branch={branch} class={class} reason={reason}\n"
        ));
    }
    if count > 0 {
        o.push_str(&format!("total: {count} stale worktree finding(s)\n"));
    }
    o
}

fn context(findings: &[String]) -> i32 {
    if findings.is_empty() {
        return 0;
    }
    let report = render_report(findings);
    // bash のダブルクォート内の `\n` は改行ではなく 2 文字のまま渡る。
    let ctx = format!(
        "[git-worktree-audit] 存在しない checkout の登録、または開いていない残骸 checkout があります。自動削除せず、git prune-worktrees で確認してください。\\n{}",
        chomp(&report)
    );
    out(&format!(
        "{{\n  \"hookSpecificOutput\": {{\n    \"hookEventName\": \"SessionStart\",\n    \"additionalContext\": {}\n  }}\n}}\n",
        json_string(&ctx)
    ));
    0
}

// ---------------------------------------------------------------------
// 監査本体
// ---------------------------------------------------------------------

enum GhCache {
    Ok(String),
    Fail,
}

/// `gh_closed_pr` の戻り(bash の return 0/1/2)。
enum GhPr {
    Found(String),
    NoPr,
    Failed,
}

pub struct Audit {
    cfg: Config,
    gh_cache: std::cell::RefCell<HashMap<String, GhCache>>,
}

impl Audit {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            gh_cache: Default::default(),
        }
    }

    /// (common-dir, 使える worktree) を発見順に返す。
    fn discover_repositories(&self) -> Vec<(String, String)> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut res = Vec::new();
        for root in [&self.cfg.ghr_dir, &self.cfg.herdr_dir] {
            if !Path::new(root).is_dir() {
                continue;
            }
            let mut found = Vec::new();
            find_dotgit(root.trim_end_matches('/'), 1, &mut found);
            for dotgit in found {
                let repo = dotgit.strip_suffix("/.git").unwrap_or(&dotgit).to_string();
                let Some(common) = git_c(
                    &repo,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                ) else {
                    continue;
                };
                let common = realpath_m(chomp(&common));
                if !seen.insert(common.clone()) {
                    continue;
                }
                res.push((common, repo));
            }
        }
        res
    }

    fn resolve_default_branch(&self, common: &str) -> String {
        if let Some(r) = git_gd(
            common,
            &["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"],
        ) {
            let r = chomp(&r);
            if !r.is_empty() {
                return r.strip_prefix("origin/").unwrap_or(r).to_string();
            }
        }
        for b in ["main", "master"] {
            if ok_gd(
                common,
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{b}"),
                ],
            ) {
                return b.to_string();
            }
        }
        String::new()
    }

    fn is_shelved(&self, wt: &str, common: &str) -> bool {
        let tag = format!("shelve:{wt}:");
        match git_gd(common, &["stash", "list", "--format=%gs"]) {
            // stash list 自体が失敗したら fail-closed(shelved 扱い)。
            None => true,
            Some(list) => list.contains(&tag),
        }
    }

    /// Herdr が開いていると報告する worktree パス。到達不能なら Err。
    fn herdr_open_paths(&self, repo: &str) -> Result<HashSet<String>, ()> {
        let o = Command::new(&self.cfg.herdr_bin)
            .args(["worktree", "list", "--cwd", repo])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| ())?;
        if !o.status.success() {
            return Err(());
        }
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let mut paths = HashSet::new();
        for doc in serde_json::Deserializer::from_str(&text).into_iter::<serde_json::Value>() {
            let v = doc.map_err(|_| ())?;
            let result = match &v {
                serde_json::Value::Null => &serde_json::Value::Null,
                serde_json::Value::Object(m) => m.get("result").unwrap_or(&serde_json::Value::Null),
                _ => return Err(()),
            };
            let wts = match result {
                serde_json::Value::Null => continue,
                serde_json::Value::Object(m) => {
                    m.get("worktrees").unwrap_or(&serde_json::Value::Null)
                }
                _ => return Err(()),
            };
            let items: Vec<&serde_json::Value> = match wts {
                serde_json::Value::Array(a) => a.iter().collect(),
                serde_json::Value::Object(m) => m.values().collect(),
                _ => continue,
            };
            for it in items {
                let serde_json::Value::Object(m) = it else {
                    return Err(());
                };
                if !m.contains_key("open_workspace_id") {
                    continue;
                }
                let p = match m.get("path") {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => "null".to_string(),
                };
                for l in p.split('\n') {
                    if !l.is_empty() {
                        paths.insert(l.to_string());
                    }
                }
            }
        }
        Ok(paths)
    }

    /// 開いておらず・clean・shelve 無しの、候補になりうる worktree か。
    /// scan_orphaned と evidence_worktrees の共通ガード(単一正本)。
    /// `herdr_open` が `None` なら Herdr 到達不能(fail-closed)。
    fn worktree_closed_and_clean(
        &self,
        wt: &str,
        common: &str,
        main_root: &str,
        herdr_open: Option<&HashSet<String>>,
    ) -> bool {
        if wt == main_root {
            return false;
        }
        if !Path::new(wt).is_dir() {
            return false;
        }
        let Some(open) = herdr_open else {
            return false;
        };
        if open.contains(wt) {
            return false;
        }
        let br = git_c(wt, &["branch", "--show-current"]).unwrap_or_default();
        if chomp(&br) == "HERDR" {
            return false;
        }
        match git_c(wt, &["status", "--porcelain"]) {
            Some(s) if s.is_empty() => {}
            _ => return false,
        }
        !self.is_shelved(wt, common)
    }

    fn scan_prunable(&self, common: &str, repo: &str) -> Vec<String> {
        let mut rows = Vec::new();
        let (mut path, mut branch, mut reason) = (String::new(), String::new(), String::new());
        for line in porcelain_lines(common) {
            if line.is_empty() {
                if !reason.is_empty() {
                    let b = branch.strip_prefix("refs/heads/").unwrap_or(&branch);
                    rows.push(format!("{common}\t{repo}\t{path}\t{b}\tprunable\t{reason}"));
                }
                path.clear();
                branch.clear();
                reason.clear();
                continue;
            }
            if let Some(v) = line.strip_prefix("worktree ") {
                path = v.to_string();
            } else if let Some(v) = line.strip_prefix("branch ") {
                branch = v.to_string();
            } else if let Some(v) = line.strip_prefix("prunable ") {
                reason = v.to_string();
            }
        }
        rows
    }

    fn scan_orphaned(&self, common: &str, repo: &str) -> Vec<String> {
        let mut rows = Vec::new();
        let main_root = common.strip_suffix("/.git").unwrap_or(common);
        let default_branch = self.resolve_default_branch(common);
        let herdr = self.herdr_open_paths(repo).ok();
        let wts: Vec<String> = git_gd(common, &["worktree", "list", "--porcelain"])
            .unwrap_or_default()
            .split('\n')
            .filter_map(|l| l.strip_prefix("worktree ").map(str::to_string))
            .collect();
        for wt in wts {
            if wt.is_empty() {
                continue;
            }
            if !self.worktree_closed_and_clean(&wt, common, main_root, herdr.as_ref()) {
                continue;
            }
            let br = git_c(&wt, &["branch", "--show-current"]).unwrap_or_default();
            let br = chomp(&br).to_string();
            if br.is_empty() {
                continue; // detached HEAD
            }
            let fer = git_c(
                &wt,
                &[
                    "for-each-ref",
                    "--format=%(upstream)%09%(upstream:track)",
                    &format!("refs/heads/{br}"),
                ],
            )
            .unwrap_or_default();
            let first = fer.split('\n').next().unwrap_or("");
            let f = ifs_tab_split(first, 2);
            let (upstream, track) = (&f[0], &f[1]);
            if track == "[gone]" {
                rows.push(format!(
                    "{common}\t{repo}\t{wt}\t{br}\torphaned\tupstream ブランチが削除済み([gone])"
                ));
                continue;
            }
            if !upstream.is_empty() {
                continue;
            }
            if default_branch.is_empty() {
                continue;
            }
            // shallow の祖先グラフは信用できない。
            let shallow = git_c(&wt, &["rev-parse", "--is-shallow-repository"]).unwrap_or_default();
            if chomp(&shallow) == "true" {
                continue;
            }
            let mut base = format!("refs/remotes/origin/{default_branch}");
            if git_c(&wt, &["rev-parse", "--verify", "-q", &base]).is_none() {
                base = format!("refs/heads/{default_branch}");
            }
            if git_c(&wt, &["rev-parse", "--verify", "-q", &base]).is_none() {
                continue;
            }
            let unique = git_c(
                &wt,
                &["rev-list", "--count", &format!("{base}..refs/heads/{br}")],
            )
            .unwrap_or_default();
            if chomp(&unique) == "0" {
                rows.push(format!(
                    "{common}\t{repo}\t{wt}\t{br}\torphaned\tupstream 未設定・{default_branch} に対して unique commit 0"
                ));
            }
        }
        rows
    }

    /// TSV: common, repo, path, branch, class, reason
    pub fn scan(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for (common, repo) in self.discover_repositories() {
            rows.extend(self.scan_prunable(&common, &repo));
            rows.extend(self.scan_orphaned(&common, &repo));
        }
        rows
    }

    fn resolve_compare_base(&self, common: &str, default_branch: &str) -> Option<String> {
        [
            format!("refs/remotes/origin/{default_branch}"),
            format!("refs/heads/{default_branch}"),
        ]
        .into_iter()
        .find(|r| ok_gd(common, &["rev-parse", "--verify", "-q", r]))
    }

    fn resolve_github_slug(&self, common: &str) -> Option<String> {
        let url = git_gd(common, &["remote", "get-url", "origin"])?;
        let url = chomp(&url);
        let rest = url
            .strip_prefix("git@github.com:")
            .or_else(|| url.strip_prefix("ssh://git@github.com/"))
            .or_else(|| url.strip_prefix("https://github.com/"))?;
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        (!rest.is_empty()).then(|| rest.to_string())
    }

    /// `slug@sha` ごとに gh を 1 回だけ呼ぶ(#586)。
    fn gh_closed_pr(&self, slug: &str, sha: &str) -> GhPr {
        let valid = (7..=64).contains(&sha.len())
            && sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !valid {
            return GhPr::Failed;
        }
        let key = format!("{slug}@{sha}");
        if !self.gh_cache.borrow().contains_key(&key) {
            let jq =
                format!(".[] | select(.state != \"open\" and .head.sha == \"{sha}\") | .number");
            let res = Command::new(&self.cfg.gh_bin)
                .args([
                    "api",
                    &format!("repos/{slug}/commits/{sha}/pulls"),
                    "--paginate",
                    "--jq",
                    &jq,
                ])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            let entry = match res {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout).into_owned();
                    let first = chomp(&s).split('\n').next().unwrap_or("").to_string();
                    GhCache::Ok(first)
                }
                _ => GhCache::Fail,
            };
            self.gh_cache.borrow_mut().insert(key.clone(), entry);
        }
        match &self.gh_cache.borrow()[&key] {
            GhCache::Fail => GhPr::Failed,
            GhCache::Ok(d) if d.is_empty() => GhPr::NoPr,
            GhCache::Ok(d) => GhPr::Found(d.clone()),
        }
    }

    /// `C2` / `C3:#N`、どちらにも当たらなければ None。common-dir だけを読む。
    fn classify_evidence(&self, common: &str, sha: &str, default_branch: &str) -> Option<String> {
        if !default_branch.is_empty() {
            let shallow =
                git_gd(common, &["rev-parse", "--is-shallow-repository"]).unwrap_or_default();
            if chomp(&shallow) != "true" {
                if let Some(base) = self.resolve_compare_base(common, default_branch) {
                    if ok_gd(common, &["merge-base", "--is-ancestor", sha, &base]) {
                        return Some("C2".into());
                    }
                }
            }
        }
        let slug = self.resolve_github_slug(common)?;
        match self.gh_closed_pr(&slug, sha) {
            GhPr::Found(n) => Some(format!("C3:#{n}")),
            _ => None,
        }
    }

    fn evidence_worktrees(&self, common: &str, repo: &str) -> Vec<String> {
        let mut rows = Vec::new();
        let main_root = common.strip_suffix("/.git").unwrap_or(common);
        let herdr = self.herdr_open_paths(repo).ok();
        let default_branch = self.resolve_default_branch(common);
        let (mut path, mut head, mut branch, mut reason) =
            (String::new(), String::new(), String::new(), String::new());
        let mut locked = false;
        for line in porcelain_lines(common) {
            if line.is_empty() {
                if !path.is_empty() {
                    if !reason.is_empty() {
                        rows.push(format!(
                            "worktree\t{common}\t{repo}\t{path}\t{branch}\t{head}\tC1"
                        ));
                    } else if !locked
                        && self.worktree_closed_and_clean(&path, common, main_root, herdr.as_ref())
                    {
                        if let Some(ev) = self.classify_evidence(common, &head, &default_branch) {
                            rows.push(format!(
                                "worktree\t{common}\t{repo}\t{path}\t{branch}\t{head}\t{ev}"
                            ));
                        }
                    }
                }
                path.clear();
                head.clear();
                branch.clear();
                reason.clear();
                locked = false;
                continue;
            }
            if let Some(v) = line.strip_prefix("worktree ") {
                path = v.to_string();
            } else if let Some(v) = line.strip_prefix("HEAD ") {
                head = v.to_string();
            } else if let Some(v) = line.strip_prefix("branch ") {
                branch = v.strip_prefix("refs/heads/").unwrap_or(v).to_string();
            } else if line.starts_with("locked") {
                locked = true;
            } else if let Some(v) = line.strip_prefix("prunable ") {
                reason = v.to_string();
            }
        }
        rows
    }

    /// 各 live worktree が作られた「持ち主」ブランチ(HEAD reflog の最初の
    /// `checkout: moving from X to …` の X、および herdr の
    /// `worktree-<name>` ↔ `worktree/<name>` 命名規約の和集合)。
    fn worktree_owner_branches(&self, common: &str) -> Vec<String> {
        let mut owners = Vec::new();
        let mut logs: Vec<String> = Vec::new();
        if let Ok(rd) = fs::read_dir(format!("{common}/worktrees")) {
            for e in rd.flatten() {
                logs.push(format!("{}/logs/HEAD", e.path().to_string_lossy()));
            }
        }
        logs.sort();
        const MARK: &str = "checkout: moving from ";
        for log in logs {
            let Ok(bytes) = fs::read(&log) else { continue };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let hit = text.split('\n').find(|l| {
                l.match_indices(MARK).any(|(i, _)| {
                    let rest = &l[i + MARK.len()..];
                    let tok = rest.find(' ').unwrap_or(rest.len());
                    tok > 0 && rest[tok..].starts_with(" to ")
                })
            });
            let Some(line) = hit else { continue };
            let Some((_, after)) = line.split_once(MARK) else {
                continue;
            };
            let from = after.split(" to ").next().unwrap_or(after);
            owners.push(from.to_string());
        }
        if let Some(out) = git_gd(common, &["worktree", "list", "--porcelain"]) {
            for line in out.split('\n') {
                if let Some(path) = line.strip_prefix("worktree ") {
                    let base = path.rsplit('/').next().unwrap_or(path);
                    if let Some(name) = base.strip_prefix("worktree-") {
                        if !name.is_empty() {
                            owners.push(format!("worktree/{name}"));
                        }
                    }
                }
            }
        }
        owners
    }

    fn evidence_branches(&self, common: &str, repo: &str) -> Vec<String> {
        let mut rows = Vec::new();
        let default_branch = self.resolve_default_branch(common);
        let mut checked_out: HashSet<String> = HashSet::new();
        if let Some(out) = git_gd(common, &["worktree", "list", "--porcelain"]) {
            for line in out.split('\n') {
                if let Some(b) = line.strip_prefix("branch refs/heads/") {
                    checked_out.insert(b.to_string());
                }
            }
        }
        // #610: 持ち主ブランチは別ブランチに switch 済みで checkout されて
        // いなくても保護する。
        for o in self.worktree_owner_branches(common) {
            if !o.is_empty() {
                checked_out.insert(o);
            }
        }
        let branches = git_gd(
            common,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )
        .unwrap_or_default();
        for b in branches.split('\n') {
            if b.is_empty() || b == default_branch || checked_out.contains(b) {
                continue;
            }
            let Some(sha) = git_gd(
                common,
                &["rev-parse", "-q", "--verify", &format!("refs/heads/{b}")],
            ) else {
                continue;
            };
            let sha = chomp(&sha);
            let Some(ev) = self.classify_evidence(common, sha, &default_branch) else {
                continue;
            };
            rows.push(format!("branch\t{common}\t{repo}\t\t{b}\t{sha}\t{ev}"));
        }
        rows
    }

    /// TSV: kind, common, repo, path, branch, sha, evidence
    pub fn evidence(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for (common, repo) in self.discover_repositories() {
            rows.extend(self.evidence_worktrees(&common, &repo));
            rows.extend(self.evidence_branches(&common, &repo));
        }
        rows
    }

    // -----------------------------------------------------------------
    // --notify
    // -----------------------------------------------------------------

    fn state_path(&self) -> String {
        format!("{}/state.json", self.cfg.state_dir)
    }

    fn write_state(&self, fp: &str, first_seen: &str, notified: bool) -> Result<(), String> {
        use std::os::unix::fs::OpenOptionsExt;
        fs::create_dir_all(&self.cfg.state_dir).map_err(|e| format!("mkdir: {e}"))?;
        let body = format!(
            "{{\n  \"fingerprint\": {},\n  \"first_seen\": {},\n  \"notified\": {}\n}}\n",
            json_string(fp),
            json_string(first_seen),
            notified
        );
        let pid = std::process::id();
        for n in 0..1000u32 {
            let tmp = format!("{}/state.json.{pid:06}{n:03}", self.cfg.state_dir);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)
            {
                Ok(mut f) => {
                    f.write_all(body.as_bytes())
                        .map_err(|e| format!("write: {e}"))?;
                    drop(f);
                    fs::rename(&tmp, self.state_path()).map_err(|e| format!("mv: {e}"))?;
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("mktemp: {e}")),
            }
        }
        Err("mktemp: exhausted".into())
    }

    fn fingerprint(findings: &[String]) -> Result<String, String> {
        let mut sorted: Vec<&String> = findings.iter().collect();
        sorted.sort();
        let mut input = String::new();
        for l in sorted {
            input.push_str(l);
            input.push('\n');
        }
        let mut child = Command::new("sha256sum")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("sha256sum: {e}"))?;
        child
            .stdin
            .take()
            .ok_or("sha256sum: no stdin")?
            .write_all(input.as_bytes())
            .map_err(|e| format!("sha256sum: {e}"))?;
        let o = child
            .wait_with_output()
            .map_err(|e| format!("sha256sum: {e}"))?;
        if !o.status.success() {
            return Err("sha256sum failed".into());
        }
        Ok(String::from_utf8_lossy(&o.stdout)
            .split(' ')
            .next()
            .unwrap_or("")
            .to_string())
    }

    fn notify(&self, findings: &[String]) -> i32 {
        match self.notify_inner(findings) {
            Ok(code) => code,
            Err(e) => {
                err(&format!("git-audit-worktrees: {e}\n"));
                1
            }
        }
    }

    fn notify_inner(&self, findings: &[String]) -> Result<i32, String> {
        use serde_json::Value;
        fs::create_dir_all(&self.cfg.state_dir).map_err(|e| format!("mkdir: {e}"))?;
        let lock = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(format!("{}/lock", self.cfg.state_dir))
            .map_err(|e| format!("lock: {e}"))?;
        if lock.try_lock().is_err() {
            return Ok(0); // 別プロセスが実行中
        }
        if findings.is_empty() {
            let _ = fs::remove_file(self.state_path());
            return Ok(0);
        }
        let fp = Self::fingerprint(findings)?;
        let state: Option<Value> = fs::read_to_string(self.state_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        let field = |k: &str| -> Option<Value> { state.as_ref().and_then(|v| v.get(k)).cloned() };
        // jq の `// empty`: null / false / 欠落は空。
        let stringish = |v: Option<Value>| -> String {
            match v {
                None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
                Some(Value::String(s)) => s,
                Some(other) => other.to_string(),
            }
        };
        let old_fp = stringish(field("fingerprint"));
        let first_seen;
        let notified;
        if old_fp == fp {
            first_seen = stringish(field("first_seen"));
            notified = match field("notified") {
                None | Some(Value::Null) | Some(Value::Bool(false)) => "false".to_string(),
                Some(Value::String(s)) => s,
                Some(other) => other.to_string(),
            };
        } else {
            let d = Command::new("date")
                .arg("+%Y-%m-%dT%H:%M:%S%z")
                .stdin(Stdio::null())
                .output()
                .map_err(|e| format!("date: {e}"))?;
            if !d.status.success() {
                return Err("date failed".into());
            }
            first_seen = chomp(&String::from_utf8_lossy(&d.stdout)).to_string();
            notified = "false".to_string();
            self.write_state(&fp, &first_seen, false)?;
            err(&format!(
                "git-worktree-audit: first detected at {first_seen}\n"
            ));
            err(&render_report(findings));
        }
        if notified == "true" {
            return Ok(0);
        }
        let count = findings.len();
        let report = render_report(findings);
        let body = truncate_body(chomp(&report), 230);
        // `$HERDR_BIN` は bash 版でクォートされていない: 空白で分割される。
        let mut words = self.cfg.herdr_bin.split_whitespace();
        let Some(prog) = words.next() else {
            return Ok(0);
        };
        let resp = Command::new(prog)
            .args(words)
            .arg("notification")
            .arg("show")
            .arg(format!("stale worktree を検出 ({count}件)"))
            .arg("--body")
            .arg(&body)
            .arg("--sound")
            .arg("request")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let resp = match resp {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => return Ok(0),
        };
        if jq_last_is_true(&resp, "shown", |v| *v == Value::Bool(true)) {
            self.write_state(&fp, &first_seen, true)?;
            return Ok(0);
        }
        // reason=="disabled" は恒常的な設定不備(#89): unit を fail させる。
        if jq_last_is_true(&resp, "reason", |v| *v == Value::String("disabled".into())) {
            err(&format!(
                "git-worktree-audit: herdr notification delivery is disabled (reason=disabled) — {count}件の stale worktree が無音で未通知です。config/herdr/config.toml の [ui.toast] delivery を設定してください。\n"
            ));
            return Ok(1);
        }
        Ok(0)
    }
}

/// `find <root> -mindepth 1 -maxdepth 4 -name .git` 相当(symlink は辿らない)。
fn find_dotgit(dir: &str, depth: u32, found: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = format!("{dir}/{name}");
        if name == ".git" {
            found.push(p);
            continue; // .git の中は探さない
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && depth < 4 {
            find_dotgit(&p, depth + 1, found);
        }
    }
}
