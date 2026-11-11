//! 実際の Chrome に対する試験(`#[ignore]`: google-chrome が PATH に要る)。
//! `cargo test -p browser-settings -- --ignored` で、手元で回す。ログインが要るサービスには触れず、
//! ローカルの HTML(`file://`)の fixture だけを開く。

use browser_settings::{chrome, compare, parse_services, Verdict};
use std::path::Path;

fn fixture_service(dir: &Path, field: &str) -> browser_settings::Service {
    let html = dir.join("settings.html");
    std::fs::write(
        &html,
        r#"<!doctype html><html><body>
<button class="open" onclick="document.getElementById('panel').hidden=false">開く</button>
<div id="panel" hidden>
  <textarea id="t" rows="4"></textarea>
  <div id="c" contenteditable="true"></div>
  <button id="save" onclick="document.title='saved'">保存</button>
</div></body></html>"#,
    )
    .unwrap();
    let toml = format!(
        "[service.local]\nurl = \"file://{}\"\nsteps = [ {{ click = \"button.open\" }}, {{ wait = \"#panel textarea\" }} ]\nfield = \"{field}\"\n",
        html.display()
    );
    parse_services(&toml).unwrap().remove("local").unwrap()
}

#[test]
#[ignore = "google-chrome が要る"]
fn paste_fills_a_textarea_and_reads_it_back_without_saving() {
    let d = tempfile::tempdir().unwrap();
    let profile = d.path().join("profile");
    let service = fixture_service(d.path(), "#t");
    let text = "1 行目\n\"2 行目\" <b>と</b> 記号 ' \\ \n3 行目";
    let got = chrome::paste(&profile, &service, text).unwrap().unwrap();
    assert_eq!(compare(text, &got), Verdict::Match, "{got:?}");
}

#[test]
#[ignore = "google-chrome が要る"]
fn paste_into_a_contenteditable_and_read_it_back() {
    let d = tempfile::tempdir().unwrap();
    let service = fixture_service(d.path(), "#c");
    let text = "a\nb";
    let got = chrome::paste(&d.path().join("p"), &service, text)
        .unwrap()
        .unwrap();
    assert_eq!(compare(text, &got), Verdict::Match, "{got:?}");
}

#[test]
#[ignore = "google-chrome が要る"]
fn a_missing_field_is_an_error_not_a_silent_success() {
    let d = tempfile::tempdir().unwrap();
    let mut service = fixture_service(d.path(), "#t");
    service.field = "#no-such-field".into();
    // 項目が出るまで待って、見つからないので失敗する(10 秒ほどかかる)。
    let err = chrome::read(&d.path().join("p"), &service).unwrap_err();
    assert!(err.contains("#no-such-field"), "{err}");
}
