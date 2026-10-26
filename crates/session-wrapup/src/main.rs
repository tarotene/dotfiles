fn main() {
    let path = session_wrapup::self_path();
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let code = match args.first().map(String::as_str) {
        Some("start") => {
            match session_wrapup::session_start(&path, &session_wrapup::read_hook_stdin()) {
                Ok(Some(out)) => {
                    println!("{out}");
                    0
                }
                Ok(None) => 0,
                Err(code) => code,
            }
        }
        Some("stop") => session_wrapup::stop_hook(&path, &session_wrapup::read_hook_stdin()),
        _ => {
            eprintln!("usage: session-wrapup start|stop");
            1
        }
    };
    std::process::exit(code);
}
