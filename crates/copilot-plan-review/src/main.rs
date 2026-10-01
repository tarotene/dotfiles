fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(copilot_plan_review::run(&args));
}
