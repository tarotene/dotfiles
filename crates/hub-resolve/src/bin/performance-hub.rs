//! performance-hub — performance-planning skill 用に private person-state
//! リポジトリの絶対パスを解決する。詳細は `hub_resolve` の lib.rs。
fn main() -> std::process::ExitCode {
    hub_resolve::run(&hub_resolve::PERFORMANCE_HUB)
}
