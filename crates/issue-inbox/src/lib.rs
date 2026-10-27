//! スコープ外の気づき(wrap-up inbox)を Issue 化させる Stop hook と、その
//! 収集指示を注入する SessionStart hook(#413、ADR-0024 Stage 4d)。
//!
//! 移植元: `config/claude/hooks/wrapup-stop-gate.sh` /
//! `config/claude/hooks/wrapup-session-start.sh`。設計と根拠は
//! docs/agents/wrapup-inbox.md。
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

pub use jq_json_compat::query as json;
mod lookup_failure;
pub mod shquote;

use jq_json_compat::jqfmt::J;
use jq_json_compat::jqparse::parse_stream;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// `${NAME:-}` が非空なら値。
pub fn env_nonempty(name: &str) -> Option<String> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
}

pub fn home() -> String {
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
pub fn dirname(p: &str) -> String {
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
pub fn lock(path: &str) -> std::io::Result<File> {
    let f = File::options().create(true).append(true).open(path)?;
    f.lock()?;
    Ok(f)
}

/// `[[ -s file ]]`
pub fn non_empty_file(p: &str) -> bool {
    fs::metadata(p).is_ok_and(|m| m.len() > 0)
}

/// `wc -l < file`(改行の数)。
pub fn newline_count(p: &str) -> usize {
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
/// コメントと docs/agents/wrapup-inbox.md)。
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
    let compact = match parse_stream(line) {
        Ok(vals) if vals.last().is_some_and(json::truthy) => {
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

// ---------------------------------------------------------------------------
// 待ちターンと、/backlog-cleanup の実行中(#854、#820)

/// `/backlog-cleanup` の印が有効な時間。スキルの実行は数時間に及ぶが、セッションが
/// 消えて `--chores-end` が呼ばれなかった印が、ほかのセッションの指示を黙らせ続けない
/// ための上限。
const CHORES_STALE_SECS: u64 = 12 * 3600;

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `/backlog-cleanup` の実行中を示す印のパス(inbox の隣)。セッション ID は、スキルを動かす
/// エージェント自身には分からない(hook の入力にだけ来る)ので、`--add` や `--mark-filed` と
/// 同じく、SessionStart の注入に出ている inbox のパスで引く。
fn chores_mark(inbox: &str) -> String {
    format!("{inbox}.chores")
}

/// `--chores-begin <inbox>`: 印を置く(中身は開始時刻)。
fn chores_begin(inbox: &str) -> Code {
    let mark = chores_mark(inbox);
    if let Some(dir) = Path::new(&mark).parent() {
        let _ = fs::create_dir_all(dir);
    }
    match fs::write(&mark, format!("{}\n", unix_now())) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("wrapup-stop-gate: --chores-begin: {mark}: {e}");
            1
        }
    }
}

/// `--chores-end <inbox>`: 印を外す(無くても成功する)。
fn chores_end(inbox: &str) -> Code {
    let _ = fs::remove_file(chores_mark(inbox));
    0
}

/// 印が有効か。inbox が空になった、または `CHORES_STALE_SECS` を超えた印は、その場で外す。
fn chores_active(inbox: &str) -> bool {
    let mark = chores_mark(inbox);
    let Ok(text) = fs::read_to_string(&mark) else {
        return false;
    };
    let started: u64 = text.trim().parse().unwrap_or(0);
    if !non_empty_file(inbox) || unix_now().saturating_sub(started) > CHORES_STALE_SECS {
        let _ = fs::remove_file(&mark);
        return false;
    }
    true
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

/// `--check-dup <title> [repo]`: 0 = 重複なし / 1 = 同名 open Issue あり
/// (その Issue 番号を stdout に 1 行出す。再発コメントの宛先) /
/// 3 = 判定不能(gh 失敗。理由は stderr、#606)。
pub fn check_dup(self_path: &str, title: &str, repo: &str) -> Code {
    check_dup_with_context(self_path, title, repo, None)
}

fn check_dup_with_context(
    self_path: &str,
    title: &str,
    repo: &str,
    context: Option<lookup_failure::Context>,
) -> Code {
    let result = lookup(self_path, title, repo, std::time::Duration::from_secs(10));
    if let Some(context) = context {
        if let Err(error) = context.record(result == 3) {
            eprintln!("check-dup: cannot update session failure ledger: {error}; inbox retained, deferral update failed");
        }
    }
    result
}

fn lookup(self_path: &str, title: &str, repo: &str, limit: std::time::Duration) -> Code {
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
        "number,title",
    ]);
    // Existing process-group timeout; seekable captures also avoid pipe-buffer stalls.
    let output = (|| -> std::io::Result<_> {
        let mut stdout = tempfile::tempfile()?;
        let mut stderr = tempfile::tempfile()?;
        c.stdin(Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?);
        let mut child = process_control::spawn_group(&mut c)?;
        let status = process_control::wait_with_timeout(&mut child, limit);
        stdout.rewind()?;
        stderr.rewind()?;
        let mut out = String::new();
        let mut err = String::new();
        stdout.read_to_string(&mut out)?;
        stderr.read_to_string(&mut err)?;
        Ok((status, out, err))
    })();
    let (status, stdout, stderr) = match output {
        Ok(output) => output,
        Err(error) => {
            eprintln!("check-dup: gh issue list failed: {self_path}: {error}");
            return 3;
        }
    };
    let Some(status) = status else {
        eprintln!(
            "check-dup: gh issue list timed out after {} seconds; inbox retained",
            limit.as_secs()
        );
        return 3;
    };
    if !status.success() {
        let err = stderr.replace('\n', " ");
        eprintln!("check-dup: gh issue list failed: {err}");
        return 3;
    }
    // jq -e --arg t "$title" 'any(.[]; .title == $t)'
    let Ok(vals) = parse_stream(stdout.trim_end_matches('\n')) else {
        return 0;
    };
    let mut last = None;
    let mut dup_number = None;
    for v in &vals {
        let items: Vec<&J> = match v {
            J::Arr(a) => a.iter().collect(),
            J::Obj(m) => m.iter().map(|(_, x)| x).collect(),
            _ => return 0, // `.[]` できない → jq エラー
        };
        let mut hit = false;
        dup_number = None;
        for it in items {
            match json::index(it, "title") {
                Some(J::Str(s)) if s == title => {
                    hit = true;
                    if let Some(J::Num(n)) = json::index(it, "number") {
                        dup_number = Some(n.clone());
                    }
                    break;
                }
                Some(_) => {}
                None => return 0, // `.title` できない → jq エラー
            }
        }
        last = Some(hit);
    }
    if last == Some(true) {
        if let Some(n) = dup_number {
            println!("{n}");
        }
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
        agent_session::ledger::sanitize_session_id(session_id)
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
    feedback_memories(session_id, true)
}

/// 今セッション中に更新された `type: feedback` の auto memory(`#N` の有無を問わない)。
/// レトロが「更新した feedback memory は行に引かれているか」を突き合わせるのに使う。
pub fn updated_feedback_memories(session_id: &str) -> Vec<String> {
    feedback_memories(session_id, false)
}

fn feedback_memories(session_id: &str, only_unlinked: bool) -> Vec<String> {
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
                if has_feedback_type(&content) && (!only_unlinked || !has_issue_ref(&content)) {
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

pub fn attribution() -> (String, String) {
    (
        env_nonempty("ATTRIBUTION_AGENT_NAME").unwrap_or_else(|| "Claude Code".into()),
        env_nonempty("ATTRIBUTION_AGENT_URL")
            .unwrap_or_else(|| "https://claude.com/claude-code".into()),
    )
}

/// `--procedure <inbox>` の手順書(LLM 向け hook 出力の書式は ADR-625)。
pub fn procedure_text(self_path: &str, inbox: &str) -> String {
    procedure_with_session(self_path, inbox, None)
}

fn procedure_with_session(self_path: &str, inbox: &str, session: Option<&str>) -> String {
    let context = session
        .map(|session| {
            format!(
                " --inbox {} --session-id {}",
                shquote::printf_q(inbox),
                shquote::printf_q(session)
            )
        })
        .unwrap_or_default();
    let eligible = session.map(|session| match lookup_failure::pending_lines(inbox, session) {
        Ok(lines) => format!("\nOnly the following exact inbox lines require filing in this session (unchanged failed rows remain in the inbox). An explicit --check-dup retry always runs:\n{}\n", lines.join("\n")),
        Err(error) => format!("\nCannot read session failure ledger: {error}; process all inbox lines.\n"),
    }).unwrap_or_default();
    let (name, url) = attribution();
    format!(
        r#"<hook-directive source="wrapup-stop-gate" kind="procedure">{eligible}
Each line of {inbox} is one JSONL item (ts/title/detail, optionally repo/go).
Process the lines one by one:
  1. Run '{self_path}' --check-dup "<title>" [repo]{context} (pass repo if the line
     has one; otherwise the target is this project's repository).
     exit 1 means an open Issue with the same title already exists (duplicate);
     its number is printed on stdout (when known) as <N>.
     exit 3 means the check could not be made — skip that line this time and
     leave it in the inbox.
  2. If the line has no "go":"ask" and is not a duplicate, file it with
     gh issue create [-R <repo>] --title "<title>" --body "<body>".
     If it is a duplicate, do not just drop it: record the recurrence on the
     existing Issue with gh issue comment <N> [-R <repo>] --body "<body>"
     (date, what happened this time from detail, and where it was seen), ending
     the comment with the same provenance footer as step 4. If <N> was not
     printed, find it with gh issue list --search "in:title <title>". The count
     of such comments is the signal ADR-543 uses to promote a rule to a script
     or gate. Do this for a "go":"ask" line too, after the AskUserQuestion
     answer of step 3 is to file it.
  3. If the line has "go":"ask" (auto-aggregated from the verdict ledger,
     ADR-478), do not file it right away even when it is not a duplicate.
     Show title, detail, and repo (the default target) via AskUserQuestion with
     the choices 「このまま <repo> に起票する」「別のリポジトリに振り直す」
     「今回は起票しない」, then act on the answer (re-routing only changes the
     -R target). gh-edit-allow may auto-allow gh issue create based on earlier
     creations in the same session, so the absence of a permission prompt is
     not a GO — always confirm with AskUserQuestion.
  4. Before writing the body, check whether the target repository is public:
     gh repo view [-R <repo>] --json visibility. If it is public, write only
     the minimal facts needed to reproduce — no internal repository names,
     project names, or confidential terms from the conversation context
     (the PreToolUse denylist rejects them, costing a round trip).
     Write the body from detail plus the conversation context, and end it with
     this line (the provenance footer, grep-able for inbox-origin Issues, also
     serves as the attribution that attribution-guard.sh requires):
       「🤖 Filed from [{name}]({url}) wrap-up inbox」
  5. Remove only the lines that were filed, commented on as duplicates, or declined
     with 「今回は起票しない」 in step 3, using
     '{self_path}' --mark-filed '{inbox}' '<the line verbatim>'.
     If gh issue create fails, do not call --mark-filed; the line stays in the
     inbox for a retry on the next turn.
Do not edit the inbox directly (always go through --add / --mark-filed).
</hook-directive>
"#
    )
}

fn stop_message(
    self_path: &str,
    inbox: &str,
    unlinked: &[String],
    session: &str,
    pending: usize,
) -> String {
    let mut msg = String::new();
    if non_empty_file(inbox) {
        let (name, url) = attribution();
        let count = pending;
        let context = if session.is_empty() || session == "unknown" {
            String::new()
        } else {
            format!(" --session-id {}", shquote::printf_q(session))
        };
        // エージェントが叩く --procedure にも Stop 起動時と同じフッター(Codex
        // は env で差し替える)が届くよう、env を明示的に前置したコマンドを示す。
        let cmd = format!(
            "ATTRIBUTION_AGENT_NAME={} ATTRIBUTION_AGENT_URL={} {} --procedure {}{context}",
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
             that does not generalize, or sensitive content (credentials, incidents, personal data), is\n\
             exempt.\n\
             </hook-directive>",
            n = unlinked.len()
        ));
    }
    msg
}

// ---------------------------------------------------------------------------
// hook 本体

pub use agent_session::legacy_input::{read_hook_stdin, HookJson};

use git_repository::raw as git_ok;

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
                let session = match arg(2) {
                    "" => None,
                    "--session-id" if args.len() == 4 => Some(need(
                        3,
                        "usage: --procedure <inbox> [--session-id <session>]",
                    )?),
                    _ => {
                        return Err(usage_error(
                            self_path,
                            &Usage {
                                param: 3,
                                text: "usage: --procedure <inbox> [--session-id <session>]",
                            },
                        ))
                    }
                };
                print!("{}", procedure_with_session(self_path, inbox, session));
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
                let mut repo = "";
                let mut inbox = None;
                let mut session = None;
                let mut index = 2;
                while index < args.len() {
                    match arg(index) {
                        "--inbox" if inbox.is_none() => { index += 1; inbox = Some(need(index, "usage: --inbox <path>")?); }
                        "--session-id" if session.is_none() => { index += 1; session = Some(need(index, "usage: --session-id <session>")?); }
                        value if index == 2 && !value.starts_with("--") => repo = value,
                        _ => return Err(usage_error(self_path, &Usage { param: index + 1, text: "usage: --check-dup <title> [repo] [--inbox <path> --session-id <session>]" })),
                    }
                    index += 1;
                }
                let context = match (inbox, session) {
                    (None, None) => None,
                    (Some(inbox), Some(session)) => {
                        match lookup_failure::Context::capture(inbox, session, title, repo) {
                            Ok(context) => Some(context),
                            Err(error) => {
                                eprintln!(
                                    "check-dup: cannot snapshot inbox: {error}; inbox retained"
                                );
                                None
                            }
                        }
                    }
                    _ => {
                        return Err(usage_error(
                            self_path,
                            &Usage {
                                param: 3,
                                text: "--inbox and --session-id must be supplied together",
                            },
                        ))
                    }
                };
                check_dup_with_context(self_path, title, repo, context)
            }
            "--chores-begin" => {
                let inbox = need(1, "usage: wrapup-stop-gate --chores-begin <inbox>")?;
                chores_begin(inbox)
            }
            "--chores-end" => {
                let inbox = need(1, "usage: wrapup-stop-gate --chores-end <inbox>")?;
                chores_end(inbox)
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
            _ => {
                eprintln!("issue-inbox: expected an inbox operation");
                1
            }
        })
    };
    run().unwrap_or_else(|c| c)
}

pub fn stop_check(self_path: &str, input: &str, h: &HookJson) -> Code {
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
    // `/backlog-cleanup` の実行中は、PR を作るまで inbox に行が残るのが正しい状態なので、
    // inbox の起票指示を出さない(#820)。フィードバックの Issue 化の指示は別物で、そのまま出す。
    let pending = match lookup_failure::pending_lines(&inbox, &session_id) {
        Ok(lines) => lines.len(),
        Err(error) => {
            eprintln!("wrapup-stop-gate: cannot read session failure ledger/inbox: {error}; filing requests remain enabled");
            newline_count(&inbox).max(usize::from(non_empty_file(&inbox)))
        }
    };
    let inbox_shown: &str = if chores_active(&inbox) || pending == 0 {
        ""
    } else {
        &inbox
    };
    if !non_empty_file(inbox_shown) && unlinked.is_empty() {
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

    let msg = stop_message(self_path, inbox_shown, &unlinked, &session_id, pending);
    let _ = writeln!(std::io::stderr(), "{msg}");
    2
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
