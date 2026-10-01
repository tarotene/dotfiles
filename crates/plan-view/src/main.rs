//! plan-view の入口。引数があれば CLI、無ければ hook として stdin を読む。

use std::io::Read;

fn main() {
    plan_view::restrict_umask();
    let mut argv = std::env::args_os();
    let argv0 = argv.next();
    let args: Vec<String> = argv.map(|a| a.to_string_lossy().into_owned()).collect();
    let cfg = plan_view::Config::from_env(argv0);

    if !args.is_empty() {
        std::process::exit(plan_view::run_cli(&cfg, &args));
    }

    // hook モード: 何があっても stdout に書かず exit 0 する(stdout の JSON は
    // permissionDecision として解釈され、承認フローに干渉してしまう)。
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    let stdin = String::from_utf8_lossy(&buf).into_owned();
    let _ = std::panic::catch_unwind(|| plan_view::run_hook(&cfg, &stdin));
    std::process::exit(0);
}
