// gpg-prewarm の hook 本体(SessionStart)。stdin は読み捨てる(cwd に依存
// しないので内容を使わない)。縮退はすべて exit 0。

fn main() {
    let gpg = std::env::var("GPG_BIN")
        .ok()
        .filter(|g| !g.is_empty())
        .unwrap_or_else(|| "gpg".to_string());
    let _ = agent_session::entry::read_stdin();
    gpg_prewarm::Prewarm {
        gpg,
        warmup_timeout: gpg_prewarm::WARMUP_TIMEOUT,
    }
    .run();
}
