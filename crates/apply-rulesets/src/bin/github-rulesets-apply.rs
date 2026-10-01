//! `github-rulesets-apply` — 複数リポジトリへ `apply-rulesets` を回す薄いループ。
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(apply_rulesets::multi::run(&args));
}
