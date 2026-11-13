//! desk-hub — my-desk skill 用に、この機体の机の正本を持つリポジトリ(ハブ)と、
//! 正本のファイルの絶対パスを解決する。引数なしでハブ、`--canon` で正本のファイル。
//! 詳細は `hub_resolve` の lib.rs(`resolve_desk`)。
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    hub_resolve::run_desk(&args)
}
