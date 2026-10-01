//! hook エントリポイント。詳細は lib.rs / docs/adr/543-existing-means-and-
//! deterministic-promotion.md。
//!
//! 日次 timer から無引数で呼ばれる。`--dry-run` を付けると inbox へ書かず
//! stdout に1候補1行(JSON)を出す(手動確認・selftest 用)。
//!
//! 環境変数(すべて未設定なら妥当な既定値にフォールバックする — due-remind
//! の `DUE_REMIND_*` と同じ形、テストでの差し替えに使う):
//!   PROMOTION_DETECT_REPO         既定 "tarotene/dotfiles"
//!   PROMOTION_DETECT_GH_BIN       既定 "gh"
//!   PROMOTION_DETECT_WRAPUP_BIN   既定 "$HOME/.claude/hooks/wrapup-stop-gate"
//!   PROMOTION_DETECT_CLAUDE_DIR   既定 "$HOME/.claude"
//!   PROMOTION_DETECT_STATE_DIR    既定 "${XDG_STATE_HOME:-$HOME/.local/state}/claude"
//!   PROMOTION_DETECT_INBOX_PATH   既定 "<state_dir>/wrapup/<repo_slug>.jsonl"

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// pr-gate.sh 以外はすべて `$HOME/.claude/<name>/skip` の形(gh-edit-allow
/// と同じ)。gate 名はここでの `<name>` をそのまま使う。
const SKIP_FILES: &[&str] = &[
    "plan-precedent-gate/skip",
    "plan-scope-gate/skip",
    "plan-fresh-gate/skip",
    "pr-gate/skip",
    "gh-edit-allow/skip",
    "new-tool-guard/skip",
    "stack-base-guard/skip",
    "plan-reviews/skip",
];

fn main() {
    let dry_run = std::env::args().any(|a| a == "--dry-run");

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let repo = env_string("PROMOTION_DETECT_REPO", "tarotene/dotfiles");
    let gh_bin = env_string("PROMOTION_DETECT_GH_BIN", "gh");
    let claude_dir = env_path("PROMOTION_DETECT_CLAUDE_DIR")
        .or_else(|| home.as_ref().map(|h| h.join(".claude")));
    let state_dir = env_path("PROMOTION_DETECT_STATE_DIR").or_else(|| {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".local/state")))?;
        Some(base.join("claude"))
    });
    let wrapup_bin = env_path("PROMOTION_DETECT_WRAPUP_BIN").or_else(|| {
        claude_dir
            .as_ref()
            .map(|d| d.join("hooks/wrapup-stop-gate"))
    });
    let inbox_path = env_path("PROMOTION_DETECT_INBOX_PATH").or_else(|| {
        state_dir.as_ref().map(|d| {
            d.join("wrapup")
                .join(format!("{}.jsonl", promotion_detect::repo_slug(&repo)))
        })
    });

    let (Some(claude_dir), Some(state_dir), Some(wrapup_bin), Some(inbox_path)) =
        (claude_dir, state_dir, wrapup_bin, inbox_path)
    else {
        return; // $HOME が解決できない環境では何もしない(fail-open)
    };

    let mut candidates: Vec<(String, String)> = Vec::new();

    // --- 再発 ---
    let issues = fetch_feedback_issues(&gh_bin, &repo);
    for c in promotion_detect::detect_recurrence(&issues, promotion_detect::RECURRENCE_THRESHOLD) {
        candidates.push((
            promotion_detect::recurrence_title(&c),
            promotion_detect::recurrence_detail(&c),
        ));
    }

    // --- 逐語反復 ---
    let hash_records = read_cmd_hashes(&state_dir.join("cmd-hashes.jsonl"));
    let repeated = promotion_detect::detect_verbatim_hashes(
        &hash_records,
        promotion_detect::VERBATIM_MIN_SESSIONS,
    );
    if !repeated.is_empty() {
        let blocks = scan_skill_blocks(&claude_dir.join("skills"));
        for c in promotion_detect::match_skill_blocks(&blocks, &repeated) {
            candidates.push((
                promotion_detect::verbatim_title(&c),
                promotion_detect::verbatim_detail(&c),
            ));
        }
    }

    // --- 降格: skip ファイルの滞留 ---
    let skip_ages = scan_skip_ages(&claude_dir, SystemTime::now());
    for c in promotion_detect::detect_stale_skips(&skip_ages, promotion_detect::SKIP_STALE_DAYS) {
        candidates.push((
            promotion_detect::stale_skip_title(&c),
            promotion_detect::stale_skip_detail(&c),
        ));
    }

    // --- 降格: gate-events.jsonl の skip 多発 ---
    let gate_events = read_gate_events(&state_dir.join("gate-events.jsonl"));
    for c in promotion_detect::detect_demotion_candidates(
        &gate_events,
        promotion_detect::DEMOTION_SKIP_THRESHOLD,
    ) {
        candidates.push((
            promotion_detect::demotion_title(&c),
            promotion_detect::demotion_detail(&c),
        ));
    }

    for (title, detail) in candidates {
        emit_candidate(&wrapup_bin, &inbox_path, &repo, &title, &detail, dry_run);
    }
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[derive(Debug, Deserialize)]
struct RawGhIssue {
    number: u64,
    body: Option<String>,
}

fn fetch_feedback_issues(gh_bin: &str, repo: &str) -> Vec<promotion_detect::FeedbackIssue> {
    let out = std::process::Command::new(gh_bin)
        .args([
            "issue",
            "list",
            "-R",
            repo,
            "--label",
            "feedback",
            "--state",
            "all",
            "--json",
            "number,body",
            "--limit",
            "200",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let Ok(text) = String::from_utf8(out.stdout) else {
        return Vec::new();
    };
    let Ok(raw): Result<Vec<RawGhIssue>, _> = serde_json::from_str(&text) else {
        return Vec::new();
    };
    raw.into_iter()
        .map(|r| promotion_detect::FeedbackIssue {
            number: r.number,
            target: r.body.as_deref().and_then(promotion_detect::parse_target),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawCmdHash {
    session_id: String,
    hash: String,
}

fn read_cmd_hashes(path: &Path) -> Vec<promotion_detect::CmdHashRecord> {
    read_jsonl::<RawCmdHash>(path)
        .into_iter()
        .map(|r| promotion_detect::CmdHashRecord {
            session_id: r.session_id,
            hash: r.hash,
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawGateEvent {
    gate: String,
    decision: String,
}

fn read_gate_events(path: &Path) -> Vec<promotion_detect::GateEventRecord> {
    read_jsonl::<RawGateEvent>(path)
        .into_iter()
        .map(|r| promotion_detect::GateEventRecord {
            gate: r.gate,
            decision: r.decision,
        })
        .collect()
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Vec<T> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn scan_skill_blocks(skills_dir: &Path) -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(path.join("SKILL.md")) else {
            continue;
        };
        let blocks = promotion_detect::extract_shell_code_blocks(&content);
        if !blocks.is_empty() {
            out.insert(name.to_string(), blocks);
        }
    }
    out
}

fn scan_skip_ages(claude_dir: &Path, now: SystemTime) -> Vec<(String, u64)> {
    SKIP_FILES
        .iter()
        .filter_map(|rel| {
            let path = claude_dir.join(rel);
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            let age = now.duration_since(modified).ok()?.as_secs();
            let gate = rel.trim_end_matches("/skip").to_string();
            Some((gate, age))
        })
        .collect()
}

fn emit_candidate(
    wrapup_bin: &Path,
    inbox_path: &Path,
    repo: &str,
    title: &str,
    detail: &str,
    dry_run: bool,
) {
    let line = promotion_detect::inbox_line(title, detail);
    if dry_run {
        println!("{line}");
        return;
    }
    if !wrapup_bin.is_file() {
        return;
    }
    let is_dup = std::process::Command::new(wrapup_bin)
        .args(["--check-dup", title, repo])
        .status()
        .map(|s| s.code() == Some(1))
        .unwrap_or(false);
    if is_dup {
        return;
    }
    let _ = std::process::Command::new(wrapup_bin)
        .arg("--add")
        .arg(inbox_path)
        .arg(&line)
        .status();
}
