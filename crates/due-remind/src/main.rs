//! due-remind [--dry-run]
//!
//! 別の private リポジトリが持つ `docs/adr/333-due-index-contract.md`
//! (`docs/adr/528-due-remind-timer.md` 参照)が定める
//! `${XDG_STATE_HOME:-~/.local/state}/claude/<domain>/<repo-slug>/due.jsonl` を
//! すべて読み、7日以内(超過側は無期限)の期限があれば `herdr notification show`
//! で1件のトーストにまとめて出す。systemd --user / launchd のタイマー
//! (home/modules/due-remind.nix)から09-19時の毎正時に呼ばれる。1日1回
//! `shown: true` を得られたらその日は何もしない — herdr が前面に無い時間帯は
//! 次の時刻に黙って再試行する。パースと組み立ては lib.rs(`cargo test` で検証)
//! にあり、このファイルはファイル探索・herdr/date の起動・exit code だけを担う
//! (herdr-issue-counts と同じ分離)。
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use due_remind::{
    build_message, days_until, exit_code, find_shown_reason, format_row, parse_row, Outcome,
};

const USAGE: &str = "usage: due-remind [--dry-run]\n\n\
Reads every claude due-index (another repo's docs/adr/333-due-index-\n\
contract.md) and posts one herdr toast when something is due within 7\n\
days or already overdue. At most once per calendar day.\n\n\
--dry-run: print the message that would be shown, without calling herdr and\n\
without touching the once-a-day state file.\n\n\
Exit code reflects delivery: 0 = nothing due, already shown today, shown\n\
just now, or a transient reason to retry later; 1 = herdr's own toast\n\
delivery is disabled in config.toml (systemctl --user --failed material).";

fn main() -> ExitCode {
    let mut dry_run = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown option: {other}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::from(exit_code(run(dry_run)))
}

fn xdg_state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").expect("HOME must be set");
            PathBuf::from(home).join(".local/state")
        })
}

fn state_dir() -> PathBuf {
    std::env::var_os("DUE_REMIND_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_state_home().join("due-remind"))
}

fn claude_dir() -> PathBuf {
    std::env::var_os("DUE_REMIND_CLAUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_state_home().join("claude"))
}

fn herdr_bin() -> String {
    std::env::var("DUE_REMIND_HERDR_BIN").unwrap_or_else(|_| "herdr".to_string())
}

/// 今日の日付(`YYYY-MM-DD`、ローカルタイムゾーン)。テストは
/// `DUE_REMIND_TODAY` で固定する — プロセス起動を経路から外すことで lib.rs
/// 側のテストと同じ純粋関数のまま検証できる。本番は外部 `date` コマンドに
/// 委ねる(タイムゾーン変換をこのクレートで再実装しない — このユニットの
/// PATH には `coreutils` を渡す、home/modules/due-remind.nix)。
fn today() -> Result<String, String> {
    if let Ok(t) = std::env::var("DUE_REMIND_TODAY") {
        return Ok(t);
    }
    let out = Command::new("date")
        .arg("+%F")
        .output()
        .map_err(|e| format!("date の実行に失敗: {e}"))?;
    if !out.status.success() {
        return Err(format!("date が終了コード {} を返した", out.status));
    }
    String::from_utf8(out.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("date の出力が UTF-8 でない: {e}"))
}

/// `<claude_dir>/*/*/due.jsonl` をすべて読み、`(domain, Row)` の一覧にする。
/// 存在しないディレクトリ・読めないファイルは黙って読み飛ばす(未使用の
/// domain・repo-slug は恒久的にあり得る通常状態)。
fn collect_rows(claude_dir: &Path) -> Vec<(String, due_remind::Row)> {
    let mut found = Vec::new();
    let Ok(domains) = fs::read_dir(claude_dir) else {
        return found;
    };
    for domain_entry in domains.flatten() {
        let Ok(domain_type) = domain_entry.file_type() else {
            continue;
        };
        if !domain_type.is_dir() {
            continue;
        }
        let domain = domain_entry.file_name().to_string_lossy().into_owned();
        let Ok(repos) = fs::read_dir(domain_entry.path()) else {
            continue;
        };
        for repo_entry in repos.flatten() {
            let idx = repo_entry.path().join("due.jsonl");
            let Ok(text) = fs::read_to_string(&idx) else {
                continue;
            };
            for line in text.lines() {
                if let Some(row) = parse_row(line) {
                    found.push((domain.clone(), row));
                }
            }
        }
    }
    found
}

fn run(dry_run: bool) -> Outcome {
    let state_dir = state_dir();
    let last_shown_path = state_dir.join("last-shown");

    let today = match today() {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("due-remind: 今日の日付を取得できない(retry): {msg}");
            return Outcome::NotShownRetry;
        }
    };

    if !dry_run {
        if let Ok(prev) = fs::read_to_string(&last_shown_path) {
            if prev.trim() == today {
                return Outcome::AlreadyShownToday;
            }
        }
    }

    let mut entries: Vec<(String, due_remind::Row, i64)> = collect_rows(&claude_dir())
        .into_iter()
        .filter_map(|(domain, row)| days_until(&row.due, &today).map(|days| (domain, row, days)))
        .collect();
    entries.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.slug.cmp(&b.1.slug))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    let lines: Vec<String> = entries
        .iter()
        .map(|(domain, row, days)| format_row(domain, row, *days))
        .collect();

    let Some(message) = build_message(&lines) else {
        return Outcome::NothingDue;
    };

    if dry_run {
        println!("{}件:", entries.len());
        for line in &lines {
            println!("  {line}");
        }
        println!("---\n{message}");
        return Outcome::NothingDue; // dry-run は state も herdr も触らない
    }

    let title = format!("期限リマインド ({}件)", entries.len());
    let response = match Command::new(herdr_bin())
        .args([
            "notification",
            "show",
            &title,
            "--body",
            &message,
            "--sound",
            "request",
        ])
        .output()
    {
        Ok(out) => out,
        Err(msg) => {
            eprintln!("due-remind: herdr の実行に失敗(retry): {msg}");
            return Outcome::NotShownRetry;
        }
    };
    let stdout = String::from_utf8_lossy(&response.stdout);
    let value: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("due-remind: herdr の応答が JSON でない(retry): {stdout}");
            return Outcome::NotShownRetry;
        }
    };
    let (shown, reason) = find_shown_reason(&value);

    match (shown, reason.as_deref()) {
        (Some(true), _) => {
            if let Err(e) = fs::create_dir_all(&state_dir) {
                eprintln!("due-remind: state ディレクトリを作成できない: {e}");
            }
            let tmp = last_shown_path.with_extension("tmp");
            if fs::write(&tmp, &today)
                .and_then(|_| fs::rename(&tmp, &last_shown_path))
                .is_err()
            {
                eprintln!("due-remind: last-shown を書き込めない({today} は表示済み)");
            }
            Outcome::Shown
        }
        (_, Some("disabled")) => {
            eprintln!(
                "due-remind: herdr の通知配信が無効化されている(config.toml [ui.toast] delivery)"
            );
            Outcome::Disabled
        }
        _ => Outcome::NotShownRetry,
    }
}
