//! wrap-up inbox の Stop hook とサブコマンド(`--add` / `--check-dup` /
//! `--mark-filed` / `--procedure` / `--inbox-path` / `--migrate` /
//! `--stamp-feedback-session`)。CLI は bash 版 `wrapup-stop-gate.sh` と同じ。

fn main() {
    let self_path = wrapup_stop_gate::self_path();
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    std::process::exit(wrapup_stop_gate::gate_main(&self_path, &args));
}
