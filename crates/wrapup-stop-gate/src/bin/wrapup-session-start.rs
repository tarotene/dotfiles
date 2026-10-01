//! wrap-up inbox の収集指示を additionalContext で注入する SessionStart hook。

fn main() {
    let self_path = wrapup_stop_gate::self_path();
    let input = wrapup_stop_gate::read_hook_stdin();
    match wrapup_stop_gate::session_start(&self_path, &input) {
        Ok(Some(out)) => println!("{out}"),
        Ok(None) => {}
        Err(code) => std::process::exit(code),
    }
}
