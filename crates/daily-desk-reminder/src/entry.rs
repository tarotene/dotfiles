// daily-desk-reminder の hook 本体(SessionStart)。縮退はすべて exit 0
// (SessionStart は exit 2 でもブロックできない)。詳細は lib.rs と
// docs/agents/my-desk.md。

use agent_session::{Agent, HookEvent, Reply};
use daily_desk_reminder::{build_context, jst_date, latest_date, state_dir};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 標準入力は使わないが、読み切って hook の作法に合わせる。
    let _ = agent_session::input::read_stdin();

    // 机の正本を持つリポジトリ(この機体の `desk-hub`、無ければ従来の `performance-hub`)が
    // 未設定の機体では、何もしない。
    let getenv = |k: &str| std::env::var(k).ok();
    match hub_resolve::resolve_desk(&getenv) {
        Ok(_) => {}
        Err(hub_resolve::DeskError::Unset(_)) => return,
        // ハブはあるのに手順が解決できない: 無音だと壊れた設定を隠すので 1 行だけ出す。
        Err(hub_resolve::DeskError::Broken(msg)) => {
            eprintln!("[daily-desk-reminder] {}", msg.trim_end());
            return;
        }
    }
    let Some(dir) = state_dir(
        std::env::var_os("DESK_STATE_DIR"),
        process_env::Xdg::State.resolve(),
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
        Reply::context(build_context(&today, last.as_deref()))
            .render(Agent::Claude, HookEvent::SessionStart)
    );
}
