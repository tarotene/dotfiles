//! 気づきの行き先が vault のノートになる経路の統合テスト(ADR-0000-wrapup-inbox-to-vault)。
//!
//! 実機の vault に書かないため、`common::Env` が `OBSIDIAN_VAULT` と `XDG_CONFIG_HOME` を消す。
//! ここでは、テストごとに一時ディレクトリの vault を `OBSIDIAN_VAULT` で明示的に宣言する。

mod common;

use common::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const REMOTE: &str = "https://github.com/example/vaulted.git";

fn vault(e: &Env) -> PathBuf {
    let v = e.path().join("vault");
    std::fs::create_dir_all(v.join(".obsidian")).unwrap();
    std::fs::write(v.join(".obsidian/app.json"), "{}").unwrap();
    v
}

fn project(e: &Env) -> PathBuf {
    repo(&e.path().join("proj"), Some(REMOTE))
}

/// vault を宣言した gate。作業中のプロジェクトは `p`。
fn gate_in(e: &Env, v: &Path, p: &Path) -> Command {
    let mut c = e.gate();
    c.env("OBSIDIAN_VAULT", v).env("CLAUDE_PROJECT_DIR", p);
    c
}

fn jsonl(e: &Env, p: &Path) -> String {
    let r = run_args(e.gate(), &["--inbox-path", p.to_str().unwrap()]);
    assert_eq!(r.code, 0);
    r.stdout
}

fn line(title: &str, detail: &str, ts: &str) -> String {
    serde_json::json!({"ts": ts, "title": title, "detail": detail}).to_string()
}

fn add(e: &Env, v: &Path, p: &Path, l: &str) -> Run {
    let inbox = jsonl(e, p);
    run_args(gate_in(e, v, p), &["--add", &inbox, l])
}

fn note(v: &Path, name: &str) -> String {
    read(&v.join("inbox").join(name))
}

#[test]
fn add_writes_a_note_and_a_recurrence_adds_a_row() {
    let e = Env::new();
    let (v, p) = (vault(&e), project(&e));
    let r = add(
        &e,
        &v,
        &p,
        &line("同じ気づき", "初回", "2026-10-01T09:00:00+09:00"),
    );
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    let r = add(
        &e,
        &v,
        &p,
        &line("同じ気づき", "再発", "2026-10-02T09:00:00+09:00"),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        note(&v, "同じ気づき.md"),
        "---\ntitle: \"同じ気づき\"\nrepo: \"example/vaulted\"\nstatus: \"open\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"2026-10-01\"\n---\n\
         - 2026-10-01 初回\n- 2026-10-02 再発\n"
    );
    // jsonl には書かない。
    assert!(!Path::new(&jsonl(&e, &p)).exists());
    // 同じ日の同じ内容は足さない。
    add(
        &e,
        &v,
        &p,
        &line("同じ気づき", "再発", "2026-10-02T23:00:00+09:00"),
    );
    assert_eq!(note(&v, "同じ気づき.md").lines().count(), 10);
}

#[test]
fn a_broken_vault_declaration_falls_back_to_the_jsonl_loudly() {
    let e = Env::new();
    let p = project(&e);
    let gone = e.path().join("no-such-vault");
    let inbox = jsonl(&e, &p);
    let l = line("退避", "d", "2026-10-01T09:00:00+09:00");
    let r = run_args(gate_in(&e, &gone, &p), &["--add", &inbox, &l]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("vault を使えない"), "{}", r.stderr);
    assert_eq!(read(Path::new(&inbox)), format!("{l}\n"));
}

#[test]
fn a_line_without_a_title_stays_in_the_jsonl_even_with_a_vault() {
    let e = Env::new();
    let (v, p) = (vault(&e), project(&e));
    let l = r#"{"ts":"2026-10-01T09:00:00+09:00","detail":"題名なし"}"#;
    let r = add(&e, &v, &p, l);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("vault に書けない"), "{}", r.stderr);
    assert_eq!(read(Path::new(&jsonl(&e, &p))), format!("{l}\n"));
    assert!(!v.join("inbox").exists());
}

#[test]
fn migrate_and_stop_drain_leftover_jsonl_lines_into_notes() {
    let e = Env::new();
    let (v, p) = (vault(&e), project(&e));
    let inbox = jsonl(&e, &p);
    std::fs::create_dir_all(Path::new(&inbox).parent().unwrap()).unwrap();
    let l1 = line("残り 1", "a", "2026-09-30T09:00:00+09:00");
    let l2 = line("残り 2", "b", "2026-09-30T10:00:00+09:00");
    std::fs::write(&inbox, format!("{l1}\nnot json\n{l2}\n")).unwrap();

    let r = run_args(gate_in(&e, &v, &p), &["--migrate", p.to_str().unwrap()]);
    assert_eq!(
        (r.code, r.stderr.contains("移せない")),
        (0, true),
        "{}",
        r.stderr
    );
    assert!(note(&v, "残り 1.md").contains("repo: \"example/vaulted\""));
    assert!(note(&v, "残り 2.md").ends_with("---\n- 2026-09-30 b\n"));
    // 読めない行だけが jsonl に残り、移した行は退避される。
    assert_eq!(read(Path::new(&inbox)), "not json\n");
    assert_eq!(
        read(Path::new(&format!("{inbox}.filed.jsonl"))),
        format!("{l1}\n{l2}\n")
    );

    // Stop でも移る(移し損ねた行が、次のターンで vault に入る)。
    let l3 = line("残り 3", "c", "2026-10-01T09:00:00+09:00");
    std::fs::write(&inbox, format!("{l3}\n")).unwrap();
    let r = run(gate_in(&e, &v, &p), &hook_input(&p));
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
    assert!(note(&v, "残り 3.md").contains("- 2026-10-01 c"));
    assert_eq!(read(Path::new(&inbox)), "");
}

fn list(e: &Env, v: &Path, p: &Path, extra: &[&str]) -> Vec<Value> {
    let mut args = vec!["--list-open", "--json"];
    args.extend_from_slice(extra);
    let r = run_args(gate_in(e, v, p), &args);
    assert_eq!(r.code, 0, "{}", r.stderr);
    serde_json::from_str::<Value>(r.stdout.trim())
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn list_open_orders_by_recurrence_and_set_status_closes_the_cycle() {
    let e = Env::new();
    let (v, p) = (vault(&e), project(&e));
    add(
        &e,
        &v,
        &p,
        &line("一度だけ", "a", "2026-10-01T09:00:00+09:00"),
    );
    add(
        &e,
        &v,
        &p,
        &line("何度も", "a", "2026-10-02T09:00:00+09:00"),
    );
    add(
        &e,
        &v,
        &p,
        &line("何度も", "b", "2026-10-03T09:00:00+09:00"),
    );

    let l = list(&e, &v, &p, &[]);
    let titles: Vec<_> = l.iter().map(|n| n["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["何度も", "一度だけ"]);
    assert_eq!(l[0]["count"], 2);
    assert_eq!(l[0]["source"], "vault");
    assert_eq!(l[0]["note"], "inbox/何度も.md");
    // 絶対パスは出さない。
    assert!(!serde_json::to_string(&l)
        .unwrap()
        .contains(v.to_str().unwrap()));
    // text 形式は 回数 / repo / 題名。
    let r = run_args(gate_in(&e, &v, &p), &["--list-open"]);
    assert_eq!(
        r.stdout,
        "2\texample/vaulted\t何度も\n1\texample/vaulted\t一度だけ\n"
    );

    let r = run_args(
        gate_in(&e, &v, &p),
        &[
            "--set-status",
            "何度も",
            "fixed",
            "https://example.test/pull/9",
        ],
    );
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (0, "inbox/何度も.md\n"),
        "{}",
        r.stderr
    );
    let n = note(&v, "何度も.md");
    assert!(n.contains("status: \"fixed\"") && n.contains("pr: \"https://example.test/pull/9\""));
    assert_eq!(list(&e, &v, &p, &[]).len(), 1);

    // 直したはずのものが再発したら、open に戻る。
    add(
        &e,
        &v,
        &p,
        &line("何度も", "c", "2026-10-09T09:00:00+09:00"),
    );
    assert_eq!(list(&e, &v, &p, &[])[0]["count"], 3);

    // --repo で絞る。
    assert_eq!(list(&e, &v, &p, &["--repo", "example/vaulted"]).len(), 2);
    assert!(list(&e, &v, &p, &["--repo", "example/other"]).is_empty());

    // 該当なしは失敗。
    let r = run_args(gate_in(&e, &v, &p), &["--set-status", "無い", "dropped"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("該当するノートが無い"), "{}", r.stderr);
}

#[test]
fn without_a_vault_the_jsonl_is_listed_and_set_status_removes_the_line() {
    let e = Env::new();
    let p = project(&e);
    let inbox = jsonl(&e, &p);
    let l1 = line("行 1", "a", "2026-10-01T09:00:00+09:00");
    let l2 = line("行 2", "b", "2026-10-02T09:00:00+09:00");
    for l in [&l1, &l2] {
        let mut c = e.gate();
        c.env("CLAUDE_PROJECT_DIR", &p);
        assert_eq!(run_args(c, &["--add", &inbox, l]).code, 0);
    }
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &p);
    let r = run_args(c, &["--list-open", "--json"]);
    let arr: Vec<Value> = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["source"], "jsonl");
    assert_eq!(arr[0]["repo"], "example/vaulted");
    assert_eq!(arr[0]["line"], l1);

    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &p);
    let r = run_args(
        c,
        &[
            "--set-status",
            "行 1",
            "issued",
            "https://example.test/issues/1",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(read(Path::new(&inbox)), format!("{l2}\n"));
    assert_eq!(
        read(Path::new(&format!("{inbox}.filed.jsonl"))),
        format!("{l1}\n")
    );
}

#[test]
fn session_start_counts_open_notes_and_leftover_lines_for_this_project() {
    let e = Env::new();
    let (v, p) = (vault(&e), project(&e));
    add(&e, &v, &p, &line("A", "a", "2026-10-01T09:00:00+09:00"));
    add(&e, &v, &p, &line("A", "b", "2026-10-02T09:00:00+09:00"));
    add(&e, &v, &p, &line("B", "a", "2026-10-01T09:00:00+09:00"));
    run_args(gate_in(&e, &v, &p), &["--set-status", "B", "dropped"]);
    let mut c = e.session_start();
    c.env("OBSIDIAN_VAULT", &v).env("CLAUDE_PROJECT_DIR", &p);
    let r = run(c, &hook_input(&p));
    assert_eq!(r.code, 0, "{}", r.stderr);
    // 件数は「ノート」の数(再発の行数ではない)。drop したノートは数えない。
    assert!(
        r.stdout.contains("There are 1 unadjudicated finding(s)"),
        "{}",
        r.stdout
    );
}
