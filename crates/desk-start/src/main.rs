//! desk-start の hook 本体(SessionStart)。縮退はすべて exit 0
//! (SessionStart は exit 2 でもブロックできない)。詳細は lib.rs と
//! docs/claude/daily-desk.md。

use desk_start::{build_context, jst_date, latest_date, session_start_json, state_dir};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 標準入力は使わないが、読み切って hook の作法に合わせる。
    let _ = hook_io::input::read_stdin();

    // ハブ(手順の正本を持つ private リポジトリ)が未設定の機体では、何もしない。
    let getenv = |k: &str| std::env::var(k).ok();
    let Ok(hub) = hub_resolve::resolve(&hub_resolve::PERFORMANCE_HUB, &getenv) else {
        return;
    };
    // ハブはあるのに手順が無い: 無音だと壊れた設定を隠すので 1 行だけ出す。
    let desk_md = std::path::Path::new(&hub).join("state/reviews/daily/desk.md");
    if !desk_md.is_file() {
        eprintln!("[desk-start] desk.md が読めません: {}", desk_md.display());
        return;
    }
    let Some(dir) = state_dir(
        std::env::var_os("DESK_STATE_DIR"),
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
    ) else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let today = jst_date(now);
    if dir.join(&today).exists() {
        return;
    }
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let last = latest_date(names.iter().map(String::as_str));
    print!(
        "{}",
        session_start_json(&build_context(&today, last.as_deref()))
    );
}
