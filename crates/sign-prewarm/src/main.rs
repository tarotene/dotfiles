//! sign-prewarm の hook 本体(SessionStart)。stdin は読み捨てる(cwd に依存
//! しないので内容を使わない)。縮退はすべて exit 0。

use std::io::Read;

fn main() {
    let gpg = std::env::var("GPG_BIN")
        .ok()
        .filter(|g| !g.is_empty())
        .unwrap_or_else(|| "gpg".to_string());
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
    sign_prewarm::Prewarm {
        gpg,
        warmup_timeout: sign_prewarm::WARMUP_TIMEOUT,
    }
    .run();
}
