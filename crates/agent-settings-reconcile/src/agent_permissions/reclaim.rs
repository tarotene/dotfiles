//! Reclaim of pending allow rules, and the promotion queue they move to.
//!
//! A rule appended by "don't ask again" is tolerated for the session (see
//! [`super::pending`]). At a session boundary it is moved to an append-only queue
//! of promotion candidates and removed from the settings file, so a standing allow
//! never outlives the session that granted it. Whether a candidate becomes part
//! of the shared policy is the owner's decision (`/promote-permissions`); nothing
//! here widens the policy.
//!
//! A rule the shared policy already grants (under the reviewed record, the same
//! baseline the permission hook uses) is not a candidate: it is dropped, so the owner
//! only sees the ones that need a decision. The queue is mirrored into the owner's
//! vault as `decisions/agent-permissions.md` (a full rewrite each time; removed when
//! the queue is empty), so the decision has a place the owner can see (ADR-1136).
use super::pending::{Pending, Revert, ToolRevert};
use super::{assess, digest, load_record, mcp_tool, read, state_path, transaction, Policy};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use vault_note::VaultNotes;

pub fn queue_path(home: &Path) -> PathBuf {
    home.join(".local/state/dotfiles/agent-permissions-pending.jsonl")
}

/// One rule waiting for the owner's decision.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub rule: String,
    pub file: PathBuf,
    pub project: PathBuf,
    pub ts: u64,
}

fn candidate_id(file: &Path, rule: &str) -> String {
    digest(format!("{}\0{rule}", file.display()).as_bytes())[..12].to_owned()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What a candidate rule is, as far as the shared policy can declare it.
enum Key {
    /// A network host (`WebFetch(domain:h)`, Codex `network_rule(host="h", …)`).
    Host(String),
    /// One MCP tool as `mcp__<server>__<tool>` (a Claude rule, or a Codex
    /// `mcp_servers.<server>.tools.<tool>.approval_mode=approve`).
    Mcp(String),
    /// `WebSearch`, `Bash(…)`, `prefix_rule(…)`, an app tool, a server-wide `mcp__<server>`.
    Other,
}

fn classify(rule: &str) -> Key {
    if let Some(host) = rule
        .strip_prefix("WebFetch(domain:")
        .and_then(|r| r.strip_suffix(')'))
    {
        return Key::Host(host.to_owned());
    }
    if let Some(rest) = rule.strip_prefix("network_rule(host=\"") {
        if let Some((host, _)) = rest.split_once('"') {
            return Key::Host(host.to_owned());
        }
    }
    if let Some(inner) = rule
        .strip_prefix("mcp_servers.")
        .and_then(|r| r.strip_suffix(".approval_mode=approve"))
    {
        if let Some((server, tool)) = inner.split_once(".tools.") {
            return Key::Mcp(format!("mcp__{server}__{tool}"));
        }
        return Key::Other;
    }
    if rule.starts_with("mcp__") && mcp_tool(rule).is_some() {
        return Key::Mcp(rule.to_owned());
    }
    Key::Other
}

/// Where in the shared policy a candidate could be declared. The one place that
/// decides this: the decision note and `/promote-permissions` describe the same split.
pub fn promotion_target(rule: &str) -> &'static str {
    match classify(rule) {
        Key::Host(_) => "domains か project_rules.<project>.domains",
        Key::Mcp(_) => "mcp_allow(拡大: inspect と、審査済みの migrate が要る)",
        Key::Other => "宣言なし(捨てる)",
    }
}

/// Whether `policy` already grants `rule` for a session in `project`, by the same
/// test the permission hook applies: a host is known when it is in `domains` or in
/// the `domains` of a `project_rules` entry that contains the project; an MCP tool
/// is known when it is in `mcp_allow`. Everything else has no declaration.
pub fn covered(policy: &Policy, project: &Path, rule: &str) -> bool {
    match classify(rule) {
        Key::Host(host) => {
            policy.domains.contains(&host)
                || policy
                    .project_rules
                    .iter()
                    .any(|(root, r)| project.starts_with(root) && r.domains.contains(&host))
        }
        Key::Mcp(tool) => policy.mcp_allow.contains(&tool),
        Key::Other => false,
    }
}

/// The project a settings file belongs to: the directory above its `.claude` / `.codex`.
fn project_of(file: &Path) -> PathBuf {
    file.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// The queue is append-only: a line adds a candidate or, with `drop`, withdraws
/// one. Appending needs no read-modify-write, so two sessions cannot lose each
/// other's lines. Candidates are the adds that were not withdrawn afterwards.
pub fn candidates(home: &Path) -> Result<Vec<Candidate>, String> {
    let path = queue_path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut live: BTreeMap<String, Candidate> = BTreeMap::new();
    for (n, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let bad = || format!("{}: malformed queue line {}", path.display(), n + 1);
        let v: Value = serde_json::from_str(line).map_err(|_| bad())?;
        if let Some(id) = v.get("drop").and_then(Value::as_str) {
            live.remove(id);
            continue;
        }
        let rule = v.get("rule").and_then(Value::as_str).ok_or_else(bad)?;
        let file = PathBuf::from(v.get("file").and_then(Value::as_str).ok_or_else(bad)?);
        let id = candidate_id(&file, rule);
        let project = project_of(&file);
        let ts = v.get("ts").and_then(Value::as_u64).unwrap_or(0);
        live.insert(
            id.clone(),
            Candidate {
                id,
                rule: rule.to_owned(),
                file,
                project,
                ts,
            },
        );
    }
    let mut out: Vec<_> = live.into_values().collect();
    out.sort_by(|a, b| (a.ts, &a.id).cmp(&(b.ts, &b.id)));
    Ok(out)
}

fn append(home: &Path, lines: &[String]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if lines.is_empty() {
        return Ok(());
    }
    let path = queue_path(home);
    let dir = path.parent().ok_or("queue has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("refusing symlink queue: {}", path.display()));
    }
    let mut buffer = String::new();
    for line in lines {
        buffer.push_str(line);
        buffer.push('\n');
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(buffer.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Withdraw candidates by id (after they were promoted or discarded). Unknown
/// ids are an error, so a typo is not mistaken for success.
pub fn drop_candidates(home: &Path, ids: &[String]) -> Result<usize, String> {
    let live: BTreeSet<String> = candidates(home)?.into_iter().map(|c| c.id).collect();
    if let Some(unknown) = ids.iter().find(|id| !live.contains(*id)) {
        return Err(format!("no pending candidate with id {unknown}"));
    }
    let lines: Vec<String> = ids
        .iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|id| json!({"v": 1, "ts": now(), "drop": id}).to_string())
        .collect();
    append(home, &lines)?;
    Ok(lines.len())
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Reclaimed {
    /// Candidates newly added to the queue.
    pub queued: usize,
    /// Allow rules removed from settings files.
    pub removed: usize,
    pub files: usize,
    /// Queued candidates the shared policy already grants, withdrawn without a decision.
    pub dropped: usize,
}

/// Withdraw every queued candidate the shared policy already grants. Runs before
/// anything else in [`reclaim`], so a queue that was filled before a rule joined the
/// policy is tidied even when no rule is waiting in any settings file.
pub fn drop_covered(policy: &Policy, home: &Path) -> Result<usize, String> {
    let ids: Vec<String> = candidates(home)?
        .into_iter()
        .filter(|c| covered(policy, &c.project, &c.rule))
        .map(|c| c.id)
        .collect();
    let lines: Vec<String> = ids
        .iter()
        .map(|id| json!({"v": 1, "ts": now(), "drop": id}).to_string())
        .collect();
    append(home, &lines)?;
    Ok(lines.len())
}

/// Move the pending allow rules to the queue, then remove them from their files.
///
/// Order matters: the queue line is written and read back *before* the file is
/// touched, so a crash leaves a rule in both places (harmless; the next run
/// finds the candidate already queued) and never in neither. The file rewrite
/// goes through the migration transaction: exact-byte compare-and-swap, a
/// recovery journal, and a read-back. A structural drift is not reclaimed.
pub fn reclaim(policy: &Policy, home: &Path, projects: &[PathBuf]) -> Result<Reclaimed, String> {
    let dropped = drop_covered(policy, home)?;
    let waiting = assess(policy, home, projects)?;
    if waiting.is_empty() {
        return Ok(Reclaimed {
            dropped,
            ..Reclaimed::default()
        });
    }
    let queued: BTreeSet<String> = candidates(home)?.into_iter().map(|c| c.id).collect();
    let mut lines: Vec<String> = Vec::new();
    let mut want: BTreeSet<String> = BTreeSet::new();
    for pending in &waiting {
        let project = project_of(&pending.path);
        for rule in &pending.rules {
            // The policy already grants it: the rule is still taken out of the file
            // (it was an individual allow), but it is not a decision for the owner.
            if covered(policy, &project, rule) {
                continue;
            }
            let id = candidate_id(&pending.path, rule);
            if !queued.contains(&id) && want.insert(id) {
                lines.push(
                    json!({"v": 1, "ts": now(), "rule": rule, "file": pending.path}).to_string(),
                );
            }
        }
    }
    let added = lines.len();
    append(home, &lines)?;
    let queued: BTreeSet<String> = candidates(home)?.into_iter().map(|c| c.id).collect();
    if let Some(missing) = want.iter().find(|id| !queued.contains(*id)) {
        return Err(format!("queue read-back is missing candidate {missing}"));
    }

    let mut writes = BTreeMap::new();
    let mut expected = BTreeMap::new();
    let mut removed = 0;
    for pending in &waiting {
        let bytes = read(&pending.path)?;
        let (out, count) = reverted(pending, &bytes)?;
        removed += count;
        expected.insert(pending.path.clone(), digest(&bytes));
        writes.insert(pending.path.clone(), Some(out));
    }
    let files = writes.len();
    // The ownership record is unchanged: reclaiming returns each file to what it records.
    transaction::commit(home, writes, &expected, || read(&state_path(home)))?;
    let left = assess(policy, home, projects)?;
    let reappeared = waiting.iter().any(|w| {
        left.iter()
            .any(|l| l.path == w.path && l.rules.iter().any(|r| w.rules.contains(r)))
    });
    if reappeared {
        return Err("reclaim read-back: a removed rule is still present".into());
    }
    Ok(Reclaimed {
        queued: added,
        removed,
        files,
        dropped,
    })
}

/// The file content with this file's pending changes undone, and how many there were.
fn reverted(pending: &Pending, bytes: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let shown = pending.path.display();
    match &pending.revert {
        Revert::Allow => {
            let mut value: Value =
                serde_json::from_slice(bytes).map_err(|e| format!("{shown}: {e}"))?;
            let allow = value
                .pointer_mut("/permissions/allow")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| format!("{shown}: permissions.allow vanished"))?;
            let before = allow.len();
            allow.retain(|r| {
                r.as_str()
                    .is_none_or(|r| !pending.rules.iter().any(|x| x == r))
            });
            let count = before - allow.len();
            let mut out = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
            out.push(b'\n');
            Ok((out, count))
        }
        // The file only grew since the record, so the recorded text is the answer.
        Revert::Rules(recorded) => Ok((recorded.clone().into_bytes(), pending.rules.len())),
        Revert::Tools(items) => Ok((revert_tools(bytes, items, &shown.to_string())?, items.len())),
    }
}

/// Put Codex tool approvals back in `config.toml` without touching anything else:
/// toml_edit keeps comments, order and formatting. A key that did not exist is
/// removed, and so are the tables that became empty because of it.
fn revert_tools(bytes: &[u8], items: &[ToolRevert], shown: &str) -> Result<Vec<u8>, String> {
    use toml_edit::{DocumentMut, Item, TableLike};
    fn table_mut<'a>(
        root: &'a mut dyn TableLike,
        keys: &[String],
    ) -> Option<&'a mut dyn TableLike> {
        let mut table = root;
        for key in keys {
            table = table.get_mut(key)?.as_table_like_mut()?;
        }
        Some(table)
    }
    /// Remove `keys` below `table` when the table at the end of them is empty.
    fn prune(table: &mut dyn TableLike, keys: &[String]) -> bool {
        if let Some((first, rest)) = keys.split_first() {
            let empty = table
                .get_mut(first)
                .and_then(Item::as_table_like_mut)
                .is_some_and(|child| prune(child, rest));
            if empty {
                table.remove(first);
            }
        }
        table.is_empty()
    }
    let text = std::str::from_utf8(bytes).map_err(|e| format!("{shown}: {e}"))?;
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("{shown}: {e}"))?;
    for item in items {
        let (last, parents) = item
            .path
            .split_last()
            .ok_or_else(|| format!("{shown}: empty approval path"))?;
        let table = table_mut(doc.as_table_mut(), parents)
            .ok_or_else(|| format!("{shown}: {} vanished", parents.join(".")))?;
        match &item.was {
            Some(mode) => {
                table.insert(last, toml_edit::value(mode.as_str()));
            }
            None => {
                table.remove(last);
            }
        }
        if item.was.is_none() {
            prune(doc.as_table_mut(), parents);
        }
    }
    Ok(doc.to_string().into_bytes())
}

// ---------------------------------------------------------------- vault の裁定ノート

/// vault 内のパス。キューの写しで、書くのはこのモジュールだけ。
pub const NOTE_REL: &str = "decisions/agent-permissions.md";

/// 2 つの `|` を壊さないように、表の 1 マスにする。
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn jst_date(unix: u64) -> String {
    let (y, m, d) = agent_session::time::civil_from_days(((unix + 9 * 3600) / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `$HOME` を `~` にする(ノートの表を短く保つ。ノートは private な vault にあるので、
/// リポジトリ名はそのまま書いてよい)。
fn tilde(home: &Path, p: &Path) -> String {
    p.strip_prefix(home).map_or_else(
        |_| p.display().to_string(),
        |r| format!("~/{}", r.display()),
    )
}

/// キューの写し。内容はキューだけで決まる(日付など、呼ぶたびに変わる値を入れない。
/// 変わらないときは書き直さないので、LiveSync に無駄な同期を走らせない)。
pub fn render_note(home: &Path, queue: &[Candidate]) -> String {
    let mut out = format!(
        "---\ncount: \"{}\"\n---\n\
         # agent-permissions: 昇格待ちの許可\n\n\
         `/promote-permissions` で裁定する(昇格する・捨てる)。この表は、回収(reclaim)がキューを書くたびに\n\
         丸ごと書き直す写しで、直接編集しない。裁定して昇格の PR ができたら、\n\
         `agent-permissions pending --drop <id>` で取り下げる(共有ポリシーに既にある規則は、回収のときに自動で取り下げる)。\n\n\
         | id | 規則 | プロジェクト | 日付(JST) | 昇格先 |\n|---|---|---|---|---|\n",
        queue.len()
    );
    for c in queue {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} | {} |\n",
            c.id,
            cell(&c.rule),
            cell(&tilde(home, &c.project)),
            jst_date(c.ts),
            promotion_target(&c.rule)
        ));
    }
    out
}

/// キューを vault のノートに写す。キューが空ならノートを消す。内容が同じなら何もしない。
/// 変えたら `true`。
pub fn sync_note(home: &Path, vault: &VaultNotes) -> Result<bool, String> {
    let queue = candidates(home)?;
    let g = vault.lock().map_err(|e| e.to_string())?;
    if queue.is_empty() {
        return g.remove(NOTE_REL).map_err(|e| e.to_string());
    }
    g.replace(NOTE_REL, &render_note(home, &queue))
        .map_err(|e| e.to_string())
}

/// 環境から vault を解決して [`sync_note`] する。vault を宣言していない機体では何もしない。
pub fn sync_note_from_env(home: &Path) -> Result<(), String> {
    match VaultNotes::open(&vault_note::state_dir()) {
        None => Ok(()),
        Some(Err(e)) => Err(format!("vault を使えない: {e}")),
        Some(Ok(vault)) => sync_note(home, &vault).map(|_| ()),
    }
}

/// The text a session boundary hook prints. It never fails: a hook that errors
/// at SessionStart would only add noise, and the permission check itself is the
/// enforcement. Returns nothing when the permission policy is not deployed.
pub fn hook_output(event: &str, stdin: &str) -> String {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return String::new();
    };
    if std::env::var_os("CODEX_HOME").is_some() || std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
        return String::new();
    }
    let policy_path = home.join(".config/agent-permissions/policy.json");
    if !policy_path.exists() || !state_path(&home).exists() {
        return String::new();
    }
    // Same baseline as the permission hook: the reviewed record's policy, so a
    // deployed-but-unreviewed expansion does not switch reclaim off (#1060). A
    // damaged record falls back to the deployed file, and reclaim reports it.
    let Ok(policy) = load_record(&home)
        .map(|record| record.policy)
        .or_else(|_| Policy::load(&policy_path))
    else {
        return String::new();
    };
    let cwd = serde_json::from_str::<Value>(stdin)
        .ok()
        .and_then(|v| v.get("cwd").and_then(Value::as_str).map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok());
    let projects: Vec<PathBuf> = cwd.into_iter().collect();
    let outcome = reclaim(&policy, &home, &projects);
    // キューの写しを vault のノートに置く(宣言の無い機体では何もしない)。失敗は握りつぶさず、
    // セッションの開始のときだけ 1 行で伝える。
    let note = sync_note_from_env(&home);
    if event != "session-start" {
        return String::new();
    }
    let mut notes: Vec<String> = Vec::new();
    match &outcome {
        Ok(done) if done.removed > 0 => notes.push(format!(
            "前のセッションが残した恒久の許可 {} 件を取り除き、共有ポリシーへの昇格候補に移しました。",
            done.removed
        )),
        Ok(_) => {}
        Err(reason) => notes.push(format!(
            "権限の記録とずれています。ツールの呼び出しが拒否されることがあります。\n{}\n\
             診断は `agent-permissions check`(読み取り専用)で全件を列挙できます。\
             復旧(migrate)は本人の端末で行ってください。",
            reason.lines().take(6).collect::<Vec<_>>().join("\n")
        )),
    }
    if let Ok(queue) = candidates(&home) {
        if !queue.is_empty() {
            notes.push(format!(
                "承認済みの許可 {} 件が、共有ポリシーへの昇格候補として溜まっています(`agent-permissions pending`)。\
                 本人が望むときにだけ `/promote-permissions` で、昇格する・捨てるを裁定してください。\
                 このセッションの依頼を先に進めてかまいません。\
                 キューが記録の正本なので、セッションの終わりに報告・起票・wrap-up inbox への追加はしないでください。",
                queue.len()
            ));
        }
    }
    if let Err(reason) = note {
        notes.push(format!(
            "昇格待ちの許可を vault のノートに写せませんでした({reason})。キューは残っています。"
        ));
    }
    // ADR-625: only the wrap-up hooks wrap their output in the directive frame; every
    // other SessionStart prints plain Japanese lines with its own tag.
    notes
        .iter()
        .flat_map(|note| note.lines())
        .map(|line| format!("[agent-permissions] {line}\n"))
        .collect()
}
