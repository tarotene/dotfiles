use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(u8::try_from(pr_contract_check::contexts::run(&args)).unwrap_or(1))
}
