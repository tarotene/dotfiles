// 使い方: git-checkout-sync <path> [<path> ...]
// (systemd timer から。usage エラー以外は常に exit 0)

use std::process::exit;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: git-checkout-sync <path> [<path> ...]");
        exit(64);
    }
    for p in &paths {
        git_checkout_sync::process_one(p);
    }
}
