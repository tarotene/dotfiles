//! `git audit-worktrees` の bin。ロジックは lib.rs。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(git_audit_worktrees::run(&args));
}
