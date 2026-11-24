//! 正本 `config/web/variables.css` から、機械可読の値(`config/web/variables.json`)を作って標準出力へ出す。
//!
//!     cargo run -q -p repo-structure-check --example web_variables_json > config/web/variables.json
//!
//! 手で書かない。`web_variables::check_tokens_json_tree` が、ツリーの `variables.json` との一致を検査する。

use std::path::Path;

use repo_structure_check::web_variables::{tokens_json, VARIABLES_PATH};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(VARIABLES_PATH);
    let css = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} を読めない: {e}", path.display()));
    match tokens_json(&css) {
        Ok(json) => print!("{json}"),
        Err(e) => {
            eprintln!("variables.json を作れない: {e}");
            std::process::exit(1);
        }
    }
}
