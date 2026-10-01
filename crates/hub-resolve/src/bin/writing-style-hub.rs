//! writing-style-hub — writing-style pointer skill 用に private スタイルガイドハブの
//! 絶対パスを解決する(#115)。詳細は `hub_resolve` の lib.rs。
fn main() -> std::process::ExitCode {
    hub_resolve::run(&hub_resolve::WRITING_STYLE_HUB)
}
