use std::{fs, process::Command};

#[test]
fn empty_observations_render_without_exposing_private_fields() {
    let home = tempfile::tempdir().unwrap();
    let output = home.path().join("data.json");
    let html = home.path().join("page.html");
    let bin = env!("CARGO_BIN_EXE_model-equivalence");
    let status = Command::new(bin)
        .env("HOME", home.path())
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .args(["observations", "--days", "14", "--out"])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let text = fs::read_to_string(&output).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["codex"]["days"], 14);
    assert_eq!(doc["claude"]["threads_read"], 0);
    for forbidden in ["repo", "thread_id", "url", "prompt"] {
        assert!(!text.contains(forbidden));
    }
    let status = Command::new(bin)
        .env("HOME", home.path())
        .args(["render", "--observations"])
        .arg(&output)
        .arg("--out")
        .arg(&html)
        .status()
        .unwrap();
    assert!(status.success());
    let page = fs::read_to_string(html).unwrap();
    assert!(page.contains("実地観測"));
    assert!(page.contains("未収集または該当する観測なし"));
    let marker = page.find("<div id=\"observations\">").unwrap();
    assert!(marker > page.find("<main>").unwrap());
    assert!(marker < page.find("</main>").unwrap());
}
