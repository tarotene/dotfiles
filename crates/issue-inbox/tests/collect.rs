//! `issue-inbox --collect`・`--collect-diff`・`--adjudication-items`(#1421)の、外から見える振る舞い。
//!
//! `gh` は、PATH の先頭に置いた偽の実行ファイル(`GH_STUB_JSON` のファイルを出すだけ)に差し替える。
//! 一時ディレクトリの vault と状態ディレクトリを環境変数で宣言し、実際のバイナリを起動する。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("vault");
        fs::create_dir_all(vault.join(".obsidian")).unwrap();
        fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
        let bin = tmp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let gh = bin.join("gh");
        fs::write(&gh, "#!/bin/sh\ncat \"$GH_STUB_JSON\"\n").unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        Env { tmp }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }
    fn issues(&self, json: &str) {
        fs::write(self.p("issues.json"), json).unwrap();
    }
    fn finding(&self, name: &str, rows: usize) {
        fs::create_dir_all(self.p("vault/inbox")).unwrap();
        let body: String = (0..rows)
            .map(|i| format!("- 2026-11-0{} d\n", i + 1))
            .collect();
        fs::write(
            self.p(&format!("vault/inbox/{name}.md")),
            format!("---\ntitle: \"{name}\"\nrepo: \"o/r\"\nstatus: \"open\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"2026-11-01\"\n---\n{body}"),
        )
        .unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_issue-inbox"))
            .args(args)
            .env("HOME", "/nonexistent-home")
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env("GH_STUB_JSON", self.p("issues.json"))
            .env("OBSIDIAN_VAULT", self.p("vault"))
            .env("WRAPUP_STATE_DIR", self.p("state"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_STATE_HOME")
            .output()
            .unwrap()
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

const ISSUES: &str = r#"[
  {"number": 5, "title": "five", "labels": [], "updatedAt": "2026-11-02T00:00:00Z"},
  {"number": 9, "title": "held", "labels": [{"name": "blocked-by-upstream"}], "updatedAt": "x"}
]"#;

fn dir(e: &Env) -> String {
    e.p("snap").to_string_lossy().into_owned()
}

#[test]
fn collect_saves_the_population_and_the_findings_and_the_diff_reports_only_what_moved() {
    let e = Env::new();
    e.issues(ISSUES);
    e.finding("f1", 1);
    let d = dir(&e);

    let c = e.run(&["--collect", &d, "--repo", "o/r"]);
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    assert!(out(&c).contains("1 issue(s), 1 finding(s)"), "{}", out(&c));
    let pop = fs::read_to_string(e.p("snap/population.json")).unwrap();
    assert!(
        pop.contains("\"number\": 5") && !pop.contains("\"number\": 9"),
        "{pop}"
    );

    // 何も動いていなければ、差分は無い。
    let same = e.run(&["--collect-diff", &d, "--repo", "o/r"]);
    assert_eq!(same.status.code(), Some(0));
    assert_eq!(out(&same).trim(), "no changes");

    // Issue の更新・新しい Issue・気づきの再発。
    e.issues(
        r#"[
      {"number": 5, "title": "five", "labels": [], "updatedAt": "2026-11-09T00:00:00Z"},
      {"number": 20, "title": "twenty", "labels": [], "updatedAt": "2026-11-09T00:00:00Z"}
    ]"#,
    );
    e.finding("f1", 3);
    let diff = e.run(&["--collect-diff", &d, "--repo", "o/r"]);
    assert_eq!(
        diff.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&diff.stderr)
    );
    let text = out(&diff);
    assert!(text.contains("~ #5 "), "{text}");
    assert!(text.contains("+ #20 "), "{text}");
    assert!(
        text.contains("気づき f1") && text.contains("1 -> 3"),
        "{text}"
    );
    // 保存した結果は書き換えない(同じ差分が、もう一度出る)。
    assert_eq!(out(&e.run(&["--collect-diff", &d, "--repo", "o/r"])), text);
}

#[test]
fn a_failed_gh_is_a_usage_level_error_not_an_empty_population() {
    let e = Env::new();
    e.issues("not json");
    let c = e.run(&["--collect", &dir(&e), "--repo", "o/r"]);
    assert_eq!(c.status.code(), Some(1));
    assert!(!e.p("snap/population.json").exists());
}

#[test]
fn the_items_input_is_built_from_the_saved_files_and_the_page_builder_accepts_it() {
    let e = Env::new();
    e.issues(ISSUES);
    e.finding("f1", 2);
    let d = dir(&e);
    assert!(e.run(&["--collect", &d, "--repo", "o/r"]).status.success());
    fs::write(
        e.p("rec.json"),
        r#"{"i5": {"rec_option": "do-agent", "rec": "着手する", "why": "理由", "when": "今回"}}"#,
    )
    .unwrap();
    let r = e.run(&[
        "--adjudication-items",
        &d,
        "--rec",
        e.p("rec.json").to_str().unwrap(),
        "--label",
        "テストの回",
    ]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let items: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(e.p("snap/items.json")).unwrap()).unwrap();
    assert_eq!(items["run_label"], "テストの回");
    assert_eq!(items["items"].as_array().unwrap().len(), 2);

    // 既存のページの生成器が、この入力を受け入れる(検証してから HTML を出す)。
    let build = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/agents/skills/inbox-adjudication/reference/verdict-page/build.sh");
    let page = e.p("page.html");
    let b = Command::new("bash")
        .arg(&build)
        .arg("--data")
        .arg(e.p("snap/items.json"))
        .arg("--out")
        .arg(&page)
        .output()
        .unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    assert!(fs::read_to_string(page).unwrap().contains("テストの回"));
}

#[test]
fn a_classification_becomes_the_recommendations_and_a_hand_written_rec_wins_per_id() {
    let e = Env::new();
    e.issues(ISSUES);
    let d = dir(&e);
    assert!(e.run(&["--collect", &d, "--repo", "o/r"]).status.success());
    fs::write(
        e.p("cls.json"),
        r#"{"i5": {"disposition": "implement", "automatable": true, "bundle": "束A", "why": "局所"}}"#,
    )
    .unwrap();
    let cls = e.p("cls.json");
    let items_of = |e: &Env| -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(e.p("snap/items.json")).unwrap()).unwrap()
    };
    let i5 = |items: &serde_json::Value| {
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == "i5")
            .unwrap()
            .clone()
    };

    // 分類だけ: 語彙の閉じた表から推奨ができる。
    let r = e.run(&[
        "--adjudication-items",
        &d,
        "--classify",
        cls.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let item = i5(&items_of(&e));
    assert_eq!(item["rec_option"], "do-agent");
    assert_eq!(item["rec"], "おまかせで進める(束: 束A)");
    assert_eq!(item["why"], "局所");

    // 同じ id に手で書いた --rec があれば、そちらが勝つ。
    fs::write(
        e.p("rec.json"),
        r#"{"i5": {"rec_option": "later", "rec": "手で直した", "why": "あとで見る", "when": "来週"}}"#,
    )
    .unwrap();
    let r = e.run(&[
        "--adjudication-items",
        &d,
        "--classify",
        cls.to_str().unwrap(),
        "--rec",
        e.p("rec.json").to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let item = i5(&items_of(&e));
    assert_eq!(item["rec_option"], "later");
    assert_eq!(item["rec"], "手で直した");

    // 既存のページの生成器が、この入力を受け入れる。
    let build = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/agents/skills/inbox-adjudication/reference/verdict-page/build.sh");
    let b = Command::new("bash")
        .arg(&build)
        .arg("--data")
        .arg(e.p("snap/items.json"))
        .arg("--out")
        .arg(e.p("page.html"))
        .output()
        .unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
}

#[test]
fn a_classification_word_outside_the_vocabulary_is_an_error_and_writes_no_items() {
    let e = Env::new();
    e.issues(ISSUES);
    let d = dir(&e);
    assert!(e.run(&["--collect", &d, "--repo", "o/r"]).status.success());
    fs::write(e.p("cls.json"), r#"{"i5": {"disposition": "do-it"}}"#).unwrap();
    let r = e.run(&[
        "--adjudication-items",
        &d,
        "--classify",
        e.p("cls.json").to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("語彙の外"));
    assert!(!e.p("snap/items.json").exists());
}
