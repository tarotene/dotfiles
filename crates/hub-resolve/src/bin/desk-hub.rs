//! desk-hub — my-desk skill 用に、この機体の机の正本を持つ person-state リポジトリ(ハブ)の
//! 絶対パスを解決する。机の正本は、ハブの `state/reviews/daily/desk.md`(個人と会社で同じ)。
//! 詳細は `hub_resolve` の lib.rs(`resolve_desk`)。
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    hub_resolve::run_desk(&args)
}
