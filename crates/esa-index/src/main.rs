//! esa-index の hook 本体(SessionStart)。縮退はすべて exit 0
//! (SessionStart は exit 2 でもブロックできない)。詳細は lib.rs と
//! docs/claude/esa-index.md。

use esa_index::{
    build_context, config_path, freshness, org_allowed, owner_of, parse_config, session_start_json,
};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 発火条件に当たらないとき(設定なし・org 不一致・repo 外)は無音。
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    let Some(project) = input.project_dir() else {
        return;
    };
    let Some(cfg_path) = config_path(
        std::env::var_os("ESA_INDEX_CONFIG"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    ) else {
        return;
    };
    let Some(cfg) = std::fs::read_to_string(cfg_path)
        .ok()
        .and_then(|s| parse_config(&s))
    else {
        return;
    };
    let Some(nwo) = hook_io::git::origin_nwo(&project) else {
        return;
    };
    if !org_allowed(owner_of(&nwo), &cfg.orgs) {
        return;
    }
    // 設定はあるのにミラーが無い: 無音だと壊れた設定を隠すので 1 行だけ出す。
    if !cfg.mirror_path.is_dir() {
        eprintln!(
            "[esa-index] mirror path does not exist: {}",
            cfg.mirror_path.display()
        );
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let fresh = Command::new("git")
        .arg("-C")
        .arg(&cfg.mirror_path)
        .args(["log", "-1", "--format=%cs %ct"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| freshness(&String::from_utf8_lossy(&o.stdout), now));
    let today = hook_io::proc::date("%Y-%m-%d");
    let ctx = build_context(&cfg.mirror_path, fresh.as_ref(), &today);
    print!("{}", session_start_json(&ctx));
}
