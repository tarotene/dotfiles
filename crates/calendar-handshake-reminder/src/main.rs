// calendar-handshake-reminder の hook 本体(SessionStart)。勤務先の機体でだけ
// 登録される(dotfiles.calendarHandshakeReminder.enable、home/modules/agents.nix)ので、
// ここは機体を判定しない。SessionStart は exit 2 でもブロックできないので、
// 縮退はすべて exit 0。詳細は lib.rs と docs/agents/calendar-handshake.md。

use calendar_handshake_reminder::{build_context, session_start_json, tokens};

fn main() {
    // 標準入力は使わないが、読み切って hook の作法に合わせる。
    let _ = agent_session::input::read_stdin();
    let (blocked, secured) = tokens();
    print!("{}", session_start_json(&build_context(&blocked, &secured)));
}
