use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

use agent_mode::{Action, Outcome};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match agent_mode::run(&args, &|name| std::env::var_os(name)) {
        Outcome::Exit(code) => ExitCode::from(code),
        Outcome::Print(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Outcome::Fail(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
        Outcome::Exec(Action { program, args, env }) => {
            let mut command = Command::new(&program);
            command.args(&args).envs(env);
            // exec は成功すると戻らない。戻ったときは起動に失敗している。
            let err = command.exec();
            eprintln!("agent-mode: {} を起動できません: {err}", program.display());
            ExitCode::from(127)
        }
    }
}
