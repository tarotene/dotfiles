//! pr-checks-wait の入口。設計は pr_gate::checks_wait と docs/agents/pr-gate.md。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(pr_gate::checks_wait::run(&args));
}
