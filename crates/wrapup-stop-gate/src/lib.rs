//! スコープ外の気づき(wrap-up inbox)を Issue 化させる Stop hook と、その
//! 収集指示を注入する SessionStart hook(#413、ADR-0024 Stage 4d)。
//!
//! 移植元: `config/claude/hooks/wrapup-stop-gate.sh` /
//! `config/claude/hooks/wrapup-session-start.sh`。設計と根拠は
//! docs/claude/wrapup-inbox.md。
//!
//! bash 版との差は 1 点だけ: 指示文(`--procedure`・Stop の `procedure_cmd`・
//! SessionStart の `--add` 例)が示すコマンドの `bash ` 前置を外した。配備物が
//! ELF バイナリになり、`bash <path>` では起動できないため。パスはどちらも
//! 自分の `argv[0]` 由来(symlink を辿らない — 配備先 `~/.claude/hooks/` は nix
//! store への symlink で、指示文には世代を跨いで安定な symlink 側を出したい)。
//!
//! 実行時依存だった jq / flock / awk / grep / find は使わない(flock は
//! `std::fs::File::lock` = flock(2) で、bash 版と同じ `<inbox>.lock` を取るので
//! 両実装が混在しても排他が成立する)。git と gh は外部コマンドのまま。

pub mod json;
pub mod shquote;

use json::J;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// `${NAME:-}` が非空なら値。
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
}

fn home() -> String {
    std::env::var_os("HOME")
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// usage エラー(bash の `${2:?usage: ...}`)。bash は
/// `<script>: line N: 2: usage: ...` を出して exit 1 する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub param: usize,
    pub text: &'static str,
}

/// 実行結果: 終了コード。
pub type Code = i32;

// ---------------------------------------------------------------------------
// 自身のパス

/// bash の `dirname`。
fn dirname(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return if p.starts_with('/') {
            "/".into()
        } else {
            ".".into()
        };
    }
    match t.rfind('/') {
        None => ".".into(),
        Some(i) => {
            let d = t[..i].trim_end_matches('/');
            if d.is_empty() {
                "/".into()
            } else {
                d.into()
            }
        }
    }
}

/// bash の `basename`。
fn basename(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return if p.starts_with('/') {
            "/".into()
        } else {
            String::new()
        };
    }
    t.rsplit('/').next().unwrap_or(t).into()
}

/// `cd <dir> && pwd`(論理パス: `..` は字句的に畳み、symlink は辿らない)。
fn logical_dir(dir: &str) -> String {
    let base: String = if dir.starts_with('/') {
        String::new()
    } else {
        let pwd = std::env::var("PWD")
            .ok()
            .filter(|p| p.starts_with('/'))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "/".into());
        pwd
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in base.split('/').chain(dir.split('/')) {
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

fn on_path(name: &str) -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':').find_map(|d| {
        let d = if d.is_empty() { "." } else { d };
        let p = format!("{d}/{name}");
        is_executable(Path::new(&p)).then_some(p)
    })
}

fn is_executable(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// bash 版 `self_path` の再現: `argv[0]` を symlink を辿らずに絶対化する
/// (`$(cd "$(dirname "$0")" && pwd)/$(basename "$0")`)。PATH 経由で起動された
/// (`argv[0]` に `/` が無い)ときは PATH 上の実体を探す。
pub fn self_path_from_argv0(argv0: &OsStr) -> String {
    let a = argv0.to_string_lossy().into_owned();
    let a = if a.contains('/') {
        a
    } else {
        on_path(&a).unwrap_or(a)
    };
    format!("{}/{}", logical_dir(&dirname(&a)), basename(&a))
}

pub fn self_path() -> String {
    self_path_from_argv0(&std::env::args_os().next().unwrap_or_default())
}

// ---------------------------------------------------------------------------
// inbox のパス

/// `${WRAPUP_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}}/claude/wrapup`
pub fn state_root() -> String {
    let base = env_nonempty("WRAPUP_STATE_DIR")
        .or_else(|| env_nonempty("XDG_STATE_HOME"))
        .unwrap_or_else(|| format!("{}/.local/state", home()));
    format!("{base}/claude/wrapup")
}

/// 絶対パス slug(`tr '/.' '--'`)。
pub fn path_slug(p: &str) -> String {
    p.replace(['/', '.'], "-")
}

/// remote URL → リポジトリ単位 slug(bash の `normalize_remote_url`)。
pub fn normalize_remote_url(u: &str) -> String {
    let mut u = u.strip_suffix('/').unwrap_or(u);
    u = u.strip_suffix(".git").unwrap_or(u);
    if let Some(i) = u.find("://") {
        u = &u[i + 3..];
    }
    if let Some(i) = u.find('@') {
        u = &u[i + 1..];
    }
    let u = u.replacen(':', "/", 1);
    u.to_ascii_lowercase().replace(['/', '.'], "-")
}

/// `git -C <dir> config --get remote.origin.url` を正規化した slug。
pub fn repo_slug(dir: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--get", "remote.origin.url"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    if url.is_empty() {
        return None;
    }
    Some(normalize_remote_url(&url))
}

pub fn inbox_for(project: &str) -> String {
    let slug = repo_slug(project).unwrap_or_else(|| path_slug(project));
    format!("{}/{slug}.jsonl", state_root())
}

// ---------------------------------------------------------------------------
// ロックとファイル操作

/// `( flock 9; ... ) 9>>"$path"`: 追記モードで開いて(無ければ作る)排他ロック。
fn lock(path: &str) -> std::io::Result<File> {
    let f = File::options().create(true).append(true).open(path)?;
    f.lock()?;
    Ok(f)
}

/// `[[ -s file ]]`
fn non_empty_file(p: &str) -> bool {
    fs::metadata(p).is_ok_and(|m| m.len() > 0)
}

/// `wc -l < file`(改行の数)。
fn newline_count(p: &str) -> usize {
    fs::read(p)
        .map(|b| b.iter().filter(|&&c| c == b'\n').count())
        .unwrap_or(0)
}

/// grep/awk の「行」: `\n` 区切りで、末尾が改行で終わっていれば最後の空要素は数えない。
fn split_lines(b: &[u8]) -> Vec<&[u8]> {
    if b.is_empty() {
        return Vec::new();
    }
    let body = b.strip_suffix(b"\n").unwrap_or(b);
    body.split(|&c| c == b'\n').collect()
}

/// `while IFS= read -r line` が読む行(改行で終わる完全な行だけ)。
fn complete_lines(b: &[u8]) -> Vec<&[u8]> {
    let mut v: Vec<&[u8]> = b.split(|&c| c == b'\n').collect();
    v.pop(); // 最後の改行の後ろ(空、または改行無しの末尾行)は読まれない
    v
}

/// 旧(絶対パス slug)の inbox を新(repo slug)の inbox へ append-only で
/// マージする(bash の `migrate_legacy_inbox`、安全性の要点は移植元の
/// コメントと docs/claude/wrapup-inbox.md)。
pub fn migrate_legacy_inbox(project: &str) -> std::io::Result<()> {
    let new = inbox_for(project);
    let legacy = format!("{}/{}.jsonl", state_root(), path_slug(project));
    if new == legacy || !Path::new(&legacy).is_file() {
        return Ok(());
    }
    fs::create_dir_all(state_root())?;
    let _l9 = lock(&format!("{new}.lock"))?;
    let _l8 = lock(&format!("{legacy}.lock"))?;
    if !non_empty_file(&legacy) {
        let _ = fs::remove_file(&legacy);
        return Ok(());
    }
    // touch "$new"
    match File::options().append(true).open(&new) {
        Ok(f) => {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        Err(_) => {
            File::create(&new)?;
        }
    }
    let legacy_bytes = fs::read(&legacy)?;
    let new_bytes = fs::read(&new)?;
    // grep -Fvxf "$new" "$legacy" >>"$new"
    {
        let have: std::collections::HashSet<&[u8]> = split_lines(&new_bytes).into_iter().collect();
        let mut add: Vec<u8> = Vec::new();
        for l in split_lines(&legacy_bytes) {
            if !have.contains(l) {
                add.extend_from_slice(l);
                add.push(b'\n');
            }
        }
        if !add.is_empty() {
            File::options().append(true).open(&new)?.write_all(&add)?;
        }
    }
    let merged = fs::read(&new)?;
    let have: std::collections::HashSet<&[u8]> = split_lines(&merged).into_iter().collect();
    if complete_lines(&legacy_bytes)
        .into_iter()
        .all(|l| have.contains(l))
    {
        let _ = fs::remove_file(&legacy);
    }
    Ok(())
}

/// `--add`: `jq -ce .` でコンパクト化して 1 行(複数値なら値ごとに 1 行)追記する。
pub fn add(inbox: &str, line: &str) -> Code {
    let compact = match json::parse_stream(line) {
        Ok(vals) if vals.last().is_some_and(J::truthy) => {
            vals.iter().map(J::compact).collect::<Vec<_>>().join("\n")
        }
        _ => {
            eprintln!("wrapup-stop-gate: --add: 不正な JSON です");
            return 64;
        }
    };
    let dir = dirname(inbox);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("mkdir: cannot create directory '{dir}': {e}");
        return 1;
    }
    let r = (|| -> std::io::Result<()> {
        let _l = lock(&format!("{inbox}.lock"))?;
        let mut f = File::options().create(true).append(true).open(inbox)?;
        f.write_all(format!("{compact}\n").as_bytes())
    })();
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("wrapup-stop-gate: {inbox}: {e}");
            1
        }
    }
}

/// awk の `$0 == ENVIRON["TARGET"]`: 両辺が数値に見えれば数値比較、それ以外は文字列比較。
fn awk_eq(a: &[u8], b: &str) -> bool {
    fn num(s: &[u8]) -> Option<f64> {
        let s = std::str::from_utf8(s).ok()?;
        let t = s.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n');
        let body = t.strip_prefix(['+', '-']).unwrap_or(t);
        let ok = !body.is_empty() && {
            let (mant, exp) = match body.find(['e', 'E']) {
                Some(i) => (&body[..i], Some(&body[i + 1..])),
                None => (body, None),
            };
            let mut it = mant.splitn(2, '.');
            let ip = it.next().unwrap_or("");
            let fp = it.next();
            let digits = |x: &str| x.bytes().all(|c| c.is_ascii_digit());
            digits(ip)
                && fp.is_none_or(digits)
                && !(ip.is_empty() && fp.is_none_or(str::is_empty))
                && exp.is_none_or(|e| {
                    let e = e.strip_prefix(['+', '-']).unwrap_or(e);
                    !e.is_empty() && digits(e)
                })
        };
        if ok {
            t.parse().ok()
        } else {
            None
        }
    }
    match (num(a), num(b.as_bytes())) {
        (Some(x), Some(y)) => x == y,
        _ => a == b.as_bytes(),
    }
}

fn tmp_sibling(base: &str) -> std::io::Result<(String, File)> {
    use std::os::unix::fs::OpenOptionsExt;
    const AL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ ((std::process::id() as u64) << 32);
    for _ in 0..100 {
        let mut s = String::new();
        for _ in 0..6 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s.push(AL[(seed >> 33) as usize % AL.len()] as char);
        }
        let p = format!("{base}.{s}");
        match File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)
        {
            Ok(f) => return Ok((p, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("mktemp: too many collisions"))
}

/// `--mark-filed`: 行全体が一致する先頭 1 行だけを削除し、削除行を
/// `<inbox>.filed.jsonl` に退避する(#297)。
pub fn mark_filed(inbox: &str, line: &str) -> Code {
    if !Path::new(inbox).is_file() {
        return 0;
    }
    let r = (|| -> std::io::Result<()> {
        let _l = lock(&format!("{inbox}.lock"))?;
        let orig = fs::read(inbox)?;
        let mut out: Vec<u8> = Vec::with_capacity(orig.len() + 1);
        let mut done = false;
        for rec in split_lines(&orig) {
            if !done && awk_eq(rec, line) {
                done = true;
                continue;
            }
            out.extend_from_slice(rec);
            out.push(b'\n');
        }
        let (tmp, mut f) = tmp_sibling(inbox)?;
        f.write_all(&out)?;
        drop(f);
        if out == orig {
            let _ = fs::remove_file(&tmp);
        } else {
            File::options()
                .create(true)
                .append(true)
                .open(format!("{inbox}.filed.jsonl"))?
                .write_all(format!("{line}\n").as_bytes())?;
            if let Ok(m) = fs::metadata(inbox) {
                let _ = fs::set_permissions(&tmp, m.permissions());
            }
            fs::rename(&tmp, inbox)?;
        }
        Ok(())
    })();
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("wrapup-stop-gate: {inbox}: {e}");
            1
        }
    }
}

/// `--check-dup <title> [repo]`: 0 = 重複なし / 1 = 同名 open Issue あり /
/// 3 = 判定不能(gh 失敗。理由は stderr、#606)。
pub fn check_dup(self_path: &str, title: &str, repo: &str) -> Code {
    let mut c = Command::new("gh");
    c.args(["issue", "list"]);
    if !repo.is_empty() {
        c.args(["-R", repo]);
    }
    c.args([
        "--state",
        "open",
        "--search",
        &format!("in:title {title}"),
        "--json",
        "title",
    ]);
    let out = match c.stdin(Stdio::inherit()).output() {
        Ok(o) => o,
        Err(_) => {
            eprintln!("check-dup: gh issue list failed: {self_path}: gh: command not found ");
            return 3;
        }
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).replace('\n', " ");
        eprintln!("check-dup: gh issue list failed: {err}");
        return 3;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // jq -e --arg t "$title" 'any(.[]; .title == $t)'
    let Ok(vals) = json::parse_stream(stdout.trim_end_matches('\n')) else {
        return 0;
    };
    let mut last = None;
    for v in &vals {
        let items: Vec<&J> = match v {
            J::Arr(a) => a.iter().collect(),
            J::Obj(m) => m.iter().map(|(_, x)| x).collect(),
            _ => return 0, // `.[]` できない → jq エラー
        };
        let mut hit = false;
        for it in items {
            match it.index("title") {
                Ok(J::Str(s)) if s == title => {
                    hit = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => return 0,
            }
        }
        last = Some(hit);
    }
    if last == Some(true) {
        1
    } else {
        0
    }
}

/// `--stamp-feedback-session <session_id>`: セッション境界の stamp を作る(#328)。
pub fn stamp_feedback_session(session_id: &str) {
    let dir = feedback_stamp_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let _ = File::create(feedback_stamp_file(session_id));
}

fn feedback_stamp_dir() -> String {
    env_nonempty("WRAPUP_FEEDBACK_STAMP_DIR")
        .unwrap_or_else(|| format!("{}/.claude/wrapup-stop-gate/feedback-session", home()))
}

fn feedback_memory_root() -> String {
    env_nonempty("WRAPUP_FEEDBACK_MEMORY_DIR")
        .unwrap_or_else(|| format!("{}/.claude/projects", home()))
}

pub fn feedback_stamp_file(session_id: &str) -> String {
    format!(
        "{}/{}.stamp",
        feedback_stamp_dir(),
        hook_io::ledger::sanitize_session_id(session_id)
    )
}

/// `find ROOT -type f -path '*/memory/*.md' -newer STAMP` の `-path` 部分。
fn memory_md_path(p: &str) -> bool {
    // `*` は `/` にも一致する: 最初の "/memory/" の後ろに ".md" で終わる残りがあればよい
    p.ends_with(".md") && p.find("/memory/").is_some_and(|i| i + 8 + 3 <= p.len())
}

/// `^[[:space:]]*type:[[:space:]]*feedback[[:space:]]*$` に一致する行があるか。
fn has_feedback_type(content: &[u8]) -> bool {
    let sp = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\x0b' | b'\x0c');
    split_lines(content).into_iter().any(|l| {
        let mut i = 0;
        while i < l.len() && sp(&l[i]) {
            i += 1;
        }
        let Some(rest) = l[i..].strip_prefix(b"type:") else {
            return false;
        };
        let mut j = 0;
        while j < rest.len() && sp(&rest[j]) {
            j += 1;
        }
        let Some(tail) = rest[j..].strip_prefix(b"feedback") else {
            return false;
        };
        tail.iter().all(sp)
    })
}

/// `#[0-9]+` を含むか。
fn has_issue_ref(content: &[u8]) -> bool {
    content
        .windows(2)
        .any(|w| w[0] == b'#' && w[1].is_ascii_digit())
}

/// 今セッション中に更新され `#N` 参照を持たない `type: feedback` の auto memory
/// を、find と同じ順(ディレクトリの読み出し順・前順)で列挙する。stamp が無い・
/// root が無ければ空(判定不能は黙って何もしない側に倒す)。
pub fn unlinked_feedback_memories(session_id: &str) -> Vec<String> {
    let stamp = feedback_stamp_file(session_id);
    if !Path::new(&stamp).is_file() {
        return Vec::new();
    }
    let root = feedback_memory_root();
    if !Path::new(&root).is_dir() {
        return Vec::new();
    }
    let Ok(since) = fs::metadata(&stamp).and_then(|m| m.modified()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(&root, &mut |path, md| {
        if md.is_file() && memory_md_path(path) && md.modified().is_ok_and(|t| t > since) {
            if let Ok(content) = fs::read(path) {
                if has_feedback_type(&content) && !has_issue_ref(&content) {
                    out.push(path.to_string());
                }
            }
        }
    });
    out
}

/// find の前順走査(symlink は辿らない、読めないディレクトリは黙って飛ばす)。
fn walk(path: &str, f: &mut dyn FnMut(&str, &fs::Metadata)) {
    let Ok(md) = fs::symlink_metadata(path) else {
        return;
    };
    f(path, &md);
    if !md.is_dir() {
        return;
    }
    let Ok(rd) = fs::read_dir(path) else {
        return;
    };
    for e in rd.flatten() {
        let child = format!(
            "{}/{}",
            path.strip_suffix('/').unwrap_or(path),
            e.file_name().to_string_lossy()
        );
        walk(&child, f);
    }
}

// ---------------------------------------------------------------------------
// 指示文

fn attribution() -> (String, String) {
    (
        env_nonempty("ATTRIBUTION_AGENT_NAME").unwrap_or_else(|| "Claude Code".into()),
        env_nonempty("ATTRIBUTION_AGENT_URL")
            .unwrap_or_else(|| "https://claude.com/claude-code".into()),
    )
}

/// `--procedure <inbox>` の手順書(LLM 向け hook 出力の書式は ADR-625)。
pub fn procedure_text(self_path: &str, inbox: &str) -> String {
    let (name, url) = attribution();
    format!(
        r#"<hook-directive source="wrapup-stop-gate" kind="procedure">
Each line of {inbox} is one JSONL item (ts/title/detail, optionally repo/go).
Process the lines one by one:
  1. Run '{self_path}' --check-dup "<title>" [repo] (pass repo if the line
     has one; otherwise the target is this project's repository).
     exit 1 means an open Issue with the same title already exists (duplicate).
     exit 3 means the check could not be made — skip that line this time and
     leave it in the inbox.
  2. If the line has no "go":"ask" and is not a duplicate, file it with
     gh issue create [-R <repo>] --title "<title>" --body "<body>".
  3. If the line has "go":"ask" (auto-aggregated from the verdict ledger,
     ADR-478), do not file it right away even when it is not a duplicate.
     Show title, detail, and repo (the default target) via AskUserQuestion with
     the choices 「このまま <repo> に起票する」「別のリポジトリに振り直す」
     「今回は起票しない」, then act on the answer (re-routing only changes the
     -R target). gh-edit-allow may auto-allow gh issue create based on earlier
     creations in the same session, so the absence of a permission prompt is
     not a GO — always confirm with AskUserQuestion.
  4. Write the body from detail plus the conversation context, and end it with
     this line (the provenance footer, grep-able for inbox-origin Issues, also
     serves as the attribution that attribution-guard.sh requires):
       「🤖 Filed from [{name}]({url}) wrap-up inbox」
  5. Remove only the lines that were filed, skipped as duplicates, or declined
     with 「今回は起票しない」 in step 3, using
     '{self_path}' --mark-filed '{inbox}' '<the line verbatim>'.
     If gh issue create fails, do not call --mark-filed; the line stays in the
     inbox for a retry on the next turn.
Do not edit the inbox directly (always go through --add / --mark-filed).
</hook-directive>
"#
    )
}

fn stop_message(self_path: &str, inbox: &str, unlinked: &[String]) -> String {
    let mut msg = String::new();
    if non_empty_file(inbox) {
        let (name, url) = attribution();
        let count = newline_count(inbox);
        // エージェントが叩く --procedure にも Stop 起動時と同じフッター(Codex
        // は env で差し替える)が届くよう、env を明示的に前置したコマンドを示す。
        let cmd = format!(
            "ATTRIBUTION_AGENT_NAME={} ATTRIBUTION_AGENT_URL={} {} --procedure {}",
            shquote::printf_q(&name),
            shquote::printf_q(&url),
            shquote::printf_q(self_path),
            shquote::printf_q(inbox),
        );
        msg.push_str(&format!(
            "<hook-directive source=\"wrapup-stop-gate\" event=\"Stop\">\n\
             {count} unfiled item(s) in the wrap-up inbox: {inbox}\n\
             Run `{cmd}` and follow its output.\n\
             </hook-directive>"
        ));
    }
    if !unlinked.is_empty() {
        if !msg.is_empty() {
            msg.push_str("\n\n");
        }
        let list: String = unlinked.iter().map(|p| format!("  - {p}\n")).collect();
        msg.push_str(&format!(
            "<hook-directive source=\"wrapup-stop-gate\" kind=\"feedback-memory\">\n\
             {n} type: feedback auto memory file(s) updated in this session have no Issue\n\
             reference (#N):\n\
             {list}\
             File general working-policy feedback (not project-specific, not sensitive) as a\n\
             GitHub Issue by default, then add #N to the body of the memory file (shared\n\
             AGENTS.md 「ユーザーからのフィードバックは不可視なローカルメモに閉じ込めない」,\n\
             config/claude/CLAUDE.md 「フィードバックの Issue 化」). Project-specific content\n\
             that does not generalize, or sensitive content (security, personal data), is\n\
             exempt.\n\
             </hook-directive>",
            n = unlinked.len()
        ));
    }
    msg
}

// ---------------------------------------------------------------------------
// hook 本体

/// hook の stdin(bash の `input="$(cat)"`: 末尾の改行を落とす)。
pub fn read_hook_stdin() -> String {
    let mut b = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut b);
    let s = String::from_utf8_lossy(&b).into_owned();
    s.trim_end_matches('\n').to_string()
}

/// 解析済みの hook 入力。不正 JSON・object でない入力は jq と同じくエラー扱い。
pub struct HookJson {
    values: Result<Vec<J>, json::ParseError>,
}

impl HookJson {
    pub fn parse(input: &str) -> Self {
        let values = json::parse_stream(input);
        if values.is_err() {
            eprintln!("wrapup-stop-gate: hook input is not valid JSON");
        }
        HookJson { values }
    }

    fn get(&self, key: &str, alt: Option<&str>) -> Result<String, json::ParseError> {
        let v = self.values.as_ref().map_err(|e| e.clone())?;
        json::jq_r_alt(v, key, alt)
    }

    /// `jq -r '.stop_hook_active // false'` が `true`。
    pub fn stop_hook_active(&self) -> bool {
        self.get("stop_hook_active", Some("false")).as_deref() == Ok("true")
    }

    /// `${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty')}`。jq が失敗すると
    /// bash は `set -e` で jq の終了コード 5 のまま抜けていた。
    pub fn project(&self) -> Result<String, Code> {
        if let Some(p) = env_nonempty("CLAUDE_PROJECT_DIR") {
            return Ok(p);
        }
        self.get("cwd", None).map_err(|_| 5)
    }

    /// `jq -r '.session_id // "unknown"' 2>/dev/null || session_id="unknown"`
    pub fn session_id(&self) -> String {
        self.get("session_id", Some("unknown"))
            .unwrap_or_else(|_| "unknown".into())
    }
}

fn git_ok(project: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// Stop hook 本体。0 = 素通り、2 = ゲート(stderr に指示)。
pub fn stop_hook(self_path: &str, input: &str) -> Code {
    let h = HookJson::parse(input);
    if h.stop_hook_active() {
        return 0;
    }
    let project = match h.project() {
        Ok(p) => p,
        Err(c) => return c,
    };
    if project.is_empty() {
        return 0;
    }
    let session_id = h.session_id();

    if let Err(e) = migrate_legacy_inbox(&project) {
        eprintln!("wrapup-stop-gate: --migrate: {e}");
        return 1;
    }
    let inbox = inbox_for(&project);

    // 判定レッジャーの集約(ADR-478)は inbox 読み取りより前に逐次呼ぶ。
    let ve = env_nonempty("WRAPUP_VERDICT_ESCALATE_BIN")
        .unwrap_or_else(|| format!("{}/verdict-escalate", dirname(self_path)));
    if fs::metadata(&ve).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
        if let Ok(mut child) = Command::new(&ve)
            .arg("--inbox")
            .arg(&inbox)
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(input.as_bytes());
            }
            let _ = child.wait();
        }
    }

    let unlinked = unlinked_feedback_memories(&session_id);
    if !non_empty_file(&inbox) && unlinked.is_empty() {
        return 0;
    }
    if on_path("gh").is_none() {
        return 0;
    }
    if git_ok(&project, &["rev-parse", "--is-inside-work-tree"]).is_none() {
        return 0;
    }
    match git_ok(&project, &["remote", "-v"]) {
        Some(out) if out.windows(7).any(|w| w == b"github.") => {}
        _ => return 0,
    }

    let msg = stop_message(self_path, &inbox, &unlinked);
    let _ = writeln!(std::io::stderr(), "{msg}");
    2
}

/// usage エラーを bash に似せて出す(`<self>: <n>: usage: ...`、exit 1)。
pub fn usage_error(self_path: &str, u: &Usage) -> Code {
    eprintln!("{self_path}: {}: {}", u.param, u.text);
    1
}

/// argv(先頭は除く)で分岐する gate の main。
pub fn gate_main(self_path: &str, args: &[String]) -> Code {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let need = |i: usize, text: &'static str| -> Result<&str, Code> {
        let v = arg(i);
        if v.is_empty() {
            Err(usage_error(self_path, &Usage { param: i + 1, text }))
        } else {
            Ok(v)
        }
    };
    let run = || -> Result<Code, Code> {
        Ok(match arg(0) {
            "--inbox-path" => {
                let p = need(1, "usage: wrapup-stop-gate.sh --inbox-path <project-dir>")?;
                print!("{}", inbox_for(p));
                0
            }
            "--procedure" => {
                let inbox = need(1, "usage: wrapup-stop-gate.sh --procedure <inbox>")?;
                print!("{}", procedure_text(self_path, inbox));
                0
            }
            "--migrate" => {
                let p = need(1, "usage: wrapup-stop-gate.sh --migrate <project-dir>")?;
                match migrate_legacy_inbox(p) {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("wrapup-stop-gate: --migrate: {e}");
                        1
                    }
                }
            }
            "--stamp-feedback-session" => {
                let sid = need(
                    1,
                    "usage: wrapup-stop-gate.sh --stamp-feedback-session <session_id>",
                )?;
                stamp_feedback_session(sid);
                0
            }
            "--add" => {
                let inbox = need(1, "usage: wrapup-stop-gate.sh --add <inbox> <json>")?;
                let line = need(2, "usage: wrapup-stop-gate.sh --add <inbox> <json>")?;
                add(inbox, line)
            }
            "--check-dup" => {
                let title = need(1, "usage: wrapup-stop-gate.sh --check-dup <title> [repo]")?;
                check_dup(self_path, title, arg(2))
            }
            "--mark-filed" => {
                let inbox = need(1, "usage: wrapup-stop-gate.sh --mark-filed <inbox> <json>")?;
                let line = need(2, "usage: wrapup-stop-gate.sh --mark-filed <inbox> <json>")?;
                mark_filed(inbox, line)
            }
            "--selftest" => {
                eprintln!(
                    "wrapup-stop-gate: --selftest は `cargo test -p wrapup-stop-gate` に移った(#413)"
                );
                0
            }
            _ => stop_hook(self_path, &read_hook_stdin()),
        })
    };
    run().unwrap_or_else(|c| c)
}

// ---------------------------------------------------------------------------
// SessionStart

#[derive(serde::Serialize)]
struct SessionStartOut<'a> {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: SessionStartInner<'a>,
}

#[derive(serde::Serialize)]
struct SessionStartInner<'a> {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'a str,
    #[serde(rename = "additionalContext")]
    additional_context: &'a str,
}

/// SessionStart hook 本体。注入 JSON を返す(何も注入しないときは `None`)。
/// `self_path` は session-start 自身のパスで、指示文には同じディレクトリの
/// `wrapup-stop-gate` を示す。
pub fn session_start(self_path: &str, input: &str) -> Result<Option<String>, Code> {
    let h = HookJson::parse(input);
    let gate = format!("{}/wrapup-stop-gate", dirname(self_path));
    let project = h.project()?;
    if project.is_empty() {
        return Ok(None);
    }
    let _ = migrate_legacy_inbox(&project);
    let inbox = inbox_for(&project);
    let sid = h.session_id();
    // bash 版は `bash "$gate" --stamp-feedback-session "$session_id" || true`
    // だったので、空の session_id は usage エラーで何もしない。
    if !sid.is_empty() {
        stamp_feedback_session(&sid);
    }
    let pending = if non_empty_file(&inbox) {
        newline_count(&inbox)
    } else {
        0
    };
    let mut ctx = format!(
        "<hook-directive source=\"wrapup-session-start\" event=\"SessionStart\">\n\
         wrap-up inbox for this project: {inbox}\n\
         When something outside the current task's scope is worth an Issue (a sign of a\n\
         bug, debt, an improvement idea), append it right then as one line per finding:\n\
         \x20 '{gate}' --add '{inbox}' '{{\"ts\": \"<ISO8601>\", \"title\": \"<Issue title>\", \"detail\": \"<what and why>\"}}'\n\
         Do not edit the inbox directly (always go through --add). The Stop hook at the\n\
         end of the turn points to the filing procedure for appended items."
    );
    if pending > 0 {
        ctx.push_str(&format!(
            "\nThe inbox currently holds {pending} unprocessed item(s) (including leftovers from past sessions)."
        ));
    }
    ctx.push_str("\n</hook-directive>");
    let out = SessionStartOut {
        hook_specific_output: SessionStartInner {
            hook_event_name: "SessionStart",
            additional_context: &ctx,
        },
    };
    Ok(Some(
        serde_json::to_string_pretty(&out).expect("serialize session-start output"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_examples() {
        for u in [
            "https://github.com/tarotene/dotfiles.git",
            "git@github.com:Tarotene/dotfiles",
            "ssh://git@github.com/tarotene/dotfiles",
            "HTTPS://GitHub.com/tarotene/dotfiles/",
        ] {
            assert_eq!(
                normalize_remote_url(u),
                "github-com-tarotene-dotfiles",
                "{u}"
            );
        }
    }

    #[test]
    fn dirname_basename_like_coreutils() {
        assert_eq!(dirname("/a/b"), "/a");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a"), ".");
        assert_eq!(dirname("a/b/"), "a");
        assert_eq!(basename("/a/b"), "b");
        assert_eq!(basename("a/b/"), "b");
    }

    #[test]
    fn logical_dir_folds_dotdot() {
        assert_eq!(logical_dir("/a/b/../c/./"), "/a/c");
        assert_eq!(logical_dir("/"), "/");
        assert_eq!(
            self_path_from_argv0(OsStr::new("/x/crates/f/../../hooks/g")),
            "/x/hooks/g"
        );
    }

    #[test]
    fn memory_path_pattern() {
        assert!(memory_md_path("/r/p/memory/x.md"));
        assert!(memory_md_path("/r/p/memory/sub/x.md"));
        assert!(!memory_md_path("/r/p/notes/x.md"));
        assert!(!memory_md_path("/r/p/memory/x.txt"));
        assert!(!memory_md_path("/r/memory.md"));
    }

    #[test]
    fn feedback_type_line() {
        assert!(has_feedback_type(b"---\n  type:   feedback  \n"));
        assert!(has_feedback_type(b"type:feedback"));
        assert!(!has_feedback_type(b"type: feedbacks\n"));
        assert!(!has_feedback_type(b"x type: feedback\n"));
        assert!(has_issue_ref(b"see #12"));
        assert!(!has_issue_ref(b"# heading"));
    }

    #[test]
    fn awk_numeric_compare() {
        assert!(awk_eq(b"1", "1.0"));
        assert!(awk_eq(b"abc", "abc"));
        assert!(!awk_eq(b"{\"a\":1}", "{\"a\":1.0}"));
    }
}
