// esa-mirror-context の hook 本体(SessionStart)。縮退はすべて exit 0
// (SessionStart は exit 2 でもブロックできない)。詳細は lib.rs と
// docs/agents/esa-mirror-context.md。

use agent_session::{Agent, HookEvent, Reply};
use esa_mirror_context::{
    build_context, config_path, freshness, org_allowed, owner_of, parse_config,
};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 発火条件に当たらないとき(設定なし・org 不一致・repo 外)は無音。
    let Some(input) = agent_session::input::read_stdin() else {
        return;
    };
    let Some(project) = input.project_dir() else {
        return;
    };
    let Some(cfg_path) = config_path(
        std::env::var_os("ESA_INDEX_CONFIG"),
        process_env::Xdg::Config.resolve(),
    ) else {
        return;
    };
    let Some(cfg) = std::fs::read_to_string(cfg_path)
        .ok()
        .and_then(|s| parse_config(&s))
    else {
        return;
    };
    let Some(nwo) = git_repository::origin_nwo(&project) else {
        return;
    };
    if !org_allowed(owner_of(&nwo), &cfg.orgs) {
        return;
    }
    // 設定はあるのにミラーが無い: 無音だと壊れた設定を隠すので 1 行だけ出す。
    if !cfg.mirror_path.is_dir() {
        eprintln!(
            "[esa-mirror-context] mirror path does not exist: {}",
            cfg.mirror_path.display()
        );
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let fresh = git_repository::out(&cfg.mirror_path, &["log", "-1", "--format=%cs %ct"])
        .and_then(|s| freshness(&s, now));
    let today = process_control::date("%Y-%m-%d");
    let ctx = build_context(&cfg.mirror_path, fresh.as_ref(), &today);
    print!(
        "{}",
        Reply::context(ctx).render(Agent::Claude, HookEvent::SessionStart)
    );
}
