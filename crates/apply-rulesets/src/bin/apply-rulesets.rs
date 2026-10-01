//! `apply-rulesets` — 対象リポジトリ自身の宣言を live な branch ruleset に適用する。
//! home-manager が `~/.local/bin/apply-rulesets.sh` としても配備する(各
//! `*-repo-governance` skill の seed.sh と文書が PATH 上のその名前を参照するため)。
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(apply_rulesets::run(&args));
}
