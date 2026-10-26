fn main() -> std::process::ExitCode {
    match agent_settings_reconcile::agent_permissions::cli(std::env::args().skip(1).collect()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("agent-permissions: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
