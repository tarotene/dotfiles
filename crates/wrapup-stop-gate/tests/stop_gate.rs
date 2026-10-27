//! bash 版 `wrapup-stop-gate.sh --selftest` の全ケースを、実バイナリ越しに
//! 再現する(#413、docs/rust-migration.md の段 1)。bash の selftest は 1 本の
//! 長いシナリオだったが、ここではケースごとに隔離した一時環境を作る。
//!
//! selftest の `check` 名 → テスト関数の対応はそれぞれの doc コメントに書く。

mod common;

use common::*;
use std::path::Path;

const LINE1: &str = r#"{"ts":"2026-08-25T00:00:00+09:00","title":"dup title","detail":"a"}"#;
const LINE2: &str = r#"{"ts":"2026-08-25T00:00:00+09:00","title":"other","detail":"b"}"#;

fn add(e: &Env, inbox: &Path, line: &str) -> Run {
    run_args(e.gate(), &["--add", inbox.to_str().unwrap(), line])
}

fn inbox_path(e: &Env, project: &Path) -> String {
    let r = run_args(e.gate(), &["--inbox-path", project.to_str().unwrap()]);
    assert_eq!(r.code, 0);
    r.stdout
}

fn stop(e: &Env, project: &Path) -> Run {
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", project);
    run(c, &hook_input(project))
}

fn feedback_block(paths: &[&Path]) -> String {
    let list: String = paths
        .iter()
        .map(|p| format!("  - {}\n", p.display()))
        .collect();
    format!(
        "<hook-directive source=\"wrapup-stop-gate\" kind=\"feedback-memory\">\n\
         {n} type: feedback auto memory file(s) updated in this session have no Issue\n\
         reference (#N):\n\
         {list}\
         File general working-policy feedback (not project-specific, not sensitive) as a\n\
         GitHub Issue by default, then add #N to the body of the memory file (shared\n\
         AGENTS.md 「ユーザーからのフィードバックは不可視なローカルメモに閉じ込めない」,\n\
         config/claude/CLAUDE.md 「フィードバックの Issue 化」). Project-specific content\n\
         that does not generalize, or sensitive content (credentials, incidents, personal data), is\n\
         exempt.\n\
         </hook-directive>",
        n = paths.len()
    )
}

/// selftest: 「stop_hook_active で素通り」
#[test]
fn stop_hook_active_passes() {
    let e = Env::new();
    let r = run(e.gate(), r#"{"cwd":"","stop_hook_active":true}"#);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
}

/// selftest: 「inbox 不在で素通り」
#[test]
fn absent_inbox_passes() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let r = stop(&e, &repo);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

/// selftest: 「--add で 2 行になる」「--add は不正 JSON を拒否」
/// 「--add は pretty-print 入力を 1 行に圧縮する」「圧縮後の行は改行を含まない」
#[test]
fn add_appends_compacts_and_rejects() {
    let e = Env::new();
    let inbox = e.path().join("deep/dir/x.jsonl");
    assert_eq!(add(&e, &inbox, LINE1).code, 0);
    assert_eq!(add(&e, &inbox, LINE2).code, 0);
    assert_eq!(read(&inbox), format!("{LINE1}\n{LINE2}\n"));
    assert!(inbox.with_extension("jsonl.lock").exists());

    let r = add(&e, &inbox, "not-json");
    assert_eq!(r.code, 64);
    assert!(
        r.stderr
            .ends_with("wrapup-stop-gate: --add: 不正な JSON です\n"),
        "{}",
        r.stderr
    );
    assert_eq!(lines(&inbox), 2);

    let pretty_inbox = e.path().join("pretty.jsonl");
    let pretty = "{\n  \"ts\": \"2026-08-25T00:00:00+09:00\",\n  \"title\": \"pretty\",\n  \"detail\": \"c\"\n}";
    assert_eq!(add(&e, &pretty_inbox, pretty).code, 0);
    assert_eq!(
        read(&pretty_inbox),
        "{\"ts\":\"2026-08-25T00:00:00+09:00\",\"title\":\"pretty\",\"detail\":\"c\"}\n"
    );
}

/// `jq -ce .` の細部(キー順の保持・重複キー・数値リテラル・文字列の再エスケープ・
/// `-e` による null/false の拒否・複数値)。bash 版の出力に固定する。
#[test]
fn add_matches_jq_compact() {
    let e = Env::new();
    let inbox = e.path().join("jq.jsonl");
    let cases: &[(&str, Option<&str>)] = &[
        (
            r#"{"z":1,"a":[1, 2.50, 1e2, -0, 0.1e1],"z":3}"#,
            Some(r#"{"z":3,"a":[1,2.50,1E+2,-0,1]}"#),
        ),
        (
            r#"{"s":"\u00e9\/\u001f\u007f\t\"\\"}"#,
            Some(r#"{"s":"é/\u001f\u007f\t\"\\"}"#),
        ),
        ("  [ ]  ", Some("[]")),
        ("1 2", Some("1\n2")),
        ("true", Some("true")),
        ("null", None),
        ("false", None),
        ("{} null", None),
        ("{", None),
        ("[1,]", None),
    ];
    for (input, want) in cases {
        let _ = std::fs::remove_file(&inbox);
        let r = add(&e, &inbox, input);
        match want {
            Some(w) => {
                assert_eq!(r.code, 0, "{input}: {}", r.stderr);
                assert_eq!(read(&inbox), format!("{w}\n"), "{input}");
            }
            None => {
                assert_eq!(r.code, 64, "{input}");
                assert!(!inbox.exists(), "{input}");
            }
        }
    }
}

/// 気づきの inbox は、Stop の指示を出さない(起票は backlog-cleanup の裁定、
/// ADR-0000-wrapup-inbox-to-vault)。gh も GitHub remote もあり、行が溜まっていても素通りする。
#[test]
fn a_non_empty_inbox_never_blocks_the_stop() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let inbox = inbox_path(&e, &repo);
    add(&e, Path::new(&inbox), LINE1);
    add(&e, Path::new(&inbox), LINE2);
    let r = stop(&e, &repo);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    // 行は消えず、裁定を待つ。
    assert_eq!(read(Path::new(&inbox)), format!("{LINE1}\n{LINE2}\n"));
}

/// 手順書(`--procedure`)と実行中の印(`--chores-*`)は無くなった。
#[test]
fn the_filing_procedure_and_chores_marks_are_gone() {
    let e = Env::new();
    for args in [
        &["--procedure", "/x/y.jsonl"][..],
        &["--chores-begin", "/x/y.jsonl"][..],
        &["--chores-end", "/x/y.jsonl"][..],
    ] {
        let r = run_args(e.gate(), args);
        assert_eq!(r.code, 1, "{args:?}");
        assert!(r.stderr.contains("was removed"), "{}", r.stderr);
    }
}

/// selftest: 「gh 不在で素通り」
#[test]
fn no_gh_passes() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    arm_feedback(&e, "nogh");
    let mut control = e.gate();
    control.env("CLAUDE_PROJECT_DIR", &repo);
    assert_eq!(
        run(control, &hook_input_sid(&repo, "nogh")).code,
        2,
        "対照: gh があれば指示が出る"
    );
    let nogh = e.path().join("nogh-bin");
    std::fs::create_dir_all(&nogh).unwrap();
    for c in [
        "jq", "git", "grep", "wc", "tr", "dirname", "basename", "cat",
    ] {
        let found = std::env::var("PATH")
            .unwrap()
            .split(':')
            .map(|d| Path::new(d).join(c))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("{c} not on PATH"));
        std::os::unix::fs::symlink(found, nogh.join(c)).unwrap();
    }
    let mut c = if is_oracle() {
        // bash 版は "$BASH" で起動していた(PATH に bash が無いため)
        let mut c = std::process::Command::new(
            std::env::var("PATH")
                .unwrap()
                .split(':')
                .map(|d| Path::new(d).join("bash"))
                .find(|p| p.is_file())
                .unwrap(),
        );
        c.arg(gate_path());
        c
    } else {
        std::process::Command::new(gate_path())
    };
    c.env("HOME", e.home())
        .env("WRAPUP_STATE_DIR", e.state())
        .env("WRAPUP_FEEDBACK_STAMP_DIR", e.stamp_dir())
        .env("WRAPUP_FEEDBACK_MEMORY_DIR", e.memory_dir())
        .env("CLAUDE_PROJECT_DIR", &repo)
        .env("PATH", &nogh);
    let r = run(c, &hook_input_sid(&repo, "nogh"));
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

/// selftest: 「GitHub remote なしで素通り」「git repo 外で素通り」(指示が出る条件は、
/// #N の無い feedback memory)。
#[test]
fn no_github_remote_or_no_repo_passes() {
    let e = Env::new();
    arm_feedback(&e, "sid");
    let github = repo(
        &e.path().join("github"),
        Some("https://github.com/example/example.git"),
    );
    let norepo = repo(&e.path().join("norepo"), None);
    let plain = e.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let gitlab = repo(
        &e.path().join("gitlab"),
        Some("https://gitlab.com/example/example.git"),
    );
    let stop_in = |p: &Path| {
        let mut c = e.gate();
        c.env("CLAUDE_PROJECT_DIR", p);
        run(c, &hook_input_sid(p, "sid"))
    };
    assert_eq!(
        stop_in(&github).code,
        2,
        "対照: GitHub の repo なら指示が出る"
    );
    for p in [&norepo, &plain, &gitlab] {
        let r = stop_in(p);
        assert_eq!((r.code, r.stderr.as_str()), (0, ""), "{}", p.display());
    }
}

/// selftest: 「scp 形式・大文字表記が同一 inbox に正規化される」
/// 「ssh:// 形式も同一 inbox に正規化される」「--inbox-path は remote なしで
/// 絶対パス slug を返す」「--inbox-path は git repo 外で絶対パス slug を返す」
#[test]
fn inbox_path_slugs() {
    let e = Env::new();
    let want = e
        .inbox_for_slug("github-com-example-example")
        .display()
        .to_string();
    for (name, url) in [
        ("https", "https://github.com/example/example.git"),
        ("scp", "git@github.com:Example/Example.git"),
        ("ssh", "ssh://git@github.com/example/example"),
        ("upper", "HTTPS://GitHub.com/example/example/"),
        ("token", "https://user:tok@github.com/example/example.git"),
    ] {
        let r = repo(&e.path().join(name), Some(url));
        assert_eq!(inbox_path(&e, &r), want, "{url}");
    }
    let norepo = repo(&e.path().join("norepo"), None);
    assert_eq!(
        inbox_path(&e, &norepo),
        e.legacy_inbox(&norepo).display().to_string()
    );
    let plain = e.path().join("pl.ain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        inbox_path(&e, &plain),
        e.legacy_inbox(&plain).display().to_string()
    );
    // 実在しないディレクトリも絶対パス slug(相対パスはそのまま置換)
    assert_eq!(inbox_path(&e, Path::new("rel/a.b")), {
        e.inbox_for_slug("rel-a-b").display().to_string()
    });
}

/// state root の解決順: WRAPUP_STATE_DIR > XDG_STATE_HOME > $HOME/.local/state。
#[test]
fn state_root_resolution() {
    let e = Env::new();
    let mut c = e.gate();
    c.env_remove("WRAPUP_STATE_DIR")
        .env("XDG_STATE_HOME", "/xdg");
    assert_eq!(
        run_args(c, &["--inbox-path", "/p"]).stdout,
        "/xdg/claude/wrapup/-p.jsonl"
    );
    let mut c = e.gate();
    c.env("WRAPUP_STATE_DIR", "").env("XDG_STATE_HOME", "");
    assert_eq!(
        run_args(c, &["--inbox-path", "/p"]).stdout,
        format!("{}/.local/state/claude/wrapup/-p.jsonl", e.home().display())
    );
}

/// selftest: 「--migrate 後、新 inbox は旧の全行を(重複排除して)含む」
/// 「--migrate 後、旧 inbox は消滅する」「--migrate 後も旧 lock は orphan の
/// まま残る」「--migrate は冪等(2 回目は無変化)」
/// 「--migrate は 0 バイトの旧 inbox を削除する」
#[test]
fn migrate_merges_legacy_inbox() {
    let e = Env::new();
    let mig = repo(
        &e.path().join("mig_repo"),
        Some("https://github.com/example/migrated.git"),
    );
    let new = e.inbox_for_slug("github-com-example-migrated");
    let legacy = e.legacy_inbox(&mig);
    let l1 = r#"{"ts":"2026-08-25T00:00:00+09:00","title":"legacy-only","detail":"a"}"#;
    let l2 = r#"{"ts":"2026-08-25T00:00:00+09:00","title":"shared","detail":"b"}"#;
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, format!("{l1}\n{l2}\n")).unwrap();
    add(&e, &new, l2);
    let r = run_args(e.gate(), &["--migrate", mig.to_str().unwrap()]);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    assert_eq!(read(&new), format!("{l2}\n{l1}\n"));
    assert!(!legacy.exists());
    assert!(Path::new(&format!("{}.lock", legacy.display())).exists());
    run_args(e.gate(), &["--migrate", mig.to_str().unwrap()]);
    assert_eq!(read(&new), format!("{l2}\n{l1}\n"));

    let empty = repo(
        &e.path().join("empty_repo"),
        Some("https://github.com/example/emptylegacy.git"),
    );
    let empty_legacy = e.legacy_inbox(&empty);
    std::fs::write(&empty_legacy, "").unwrap();
    run_args(e.gate(), &["--migrate", empty.to_str().unwrap()]);
    assert!(!empty_legacy.exists());
    assert!(!e.inbox_for_slug("github-com-example-emptylegacy").exists());

    // remote なし(新 == 旧)は何もしない
    let norepo = repo(&e.path().join("norepo"), None);
    let same = e.legacy_inbox(&norepo);
    std::fs::write(&same, "x\n").unwrap();
    run_args(e.gate(), &["--migrate", norepo.to_str().unwrap()]);
    assert_eq!(read(&same), "x\n");
}

/// 旧 inbox に重複行・改行無し末尾行がある場合の bash の挙動
/// (`grep -Fvxf` は旧側の重複を両方足す、`while read` は改行無し末尾行を
/// 照合しない)を固定する。
#[test]
fn migrate_edge_lines() {
    let e = Env::new();
    let mig = repo(
        &e.path().join("m"),
        Some("https://github.com/example/edge.git"),
    );
    let new = e.inbox_for_slug("github-com-example-edge");
    let legacy = e.legacy_inbox(&mig);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "a\na\nb").unwrap();
    std::fs::write(&new, "c\n").unwrap();
    run_args(e.gate(), &["--migrate", mig.to_str().unwrap()]);
    assert_eq!(read(&new), "c\na\na\nb\n");
    assert!(!legacy.exists());
}

/// selftest: 「--mark-filed は先頭一致 1 行だけ削除」「--mark-filed 後も同一
/// 内容のもう 1 行は残る」「--mark-filed は削除行を tombstone に退避する」
/// 「--mark-filed 後も inbox のパーミッションは維持される」「--mark-filed は
/// 不一致行では無変更」「--mark-filed は no-op 時に tombstone を増やさない」
#[test]
fn mark_filed_removes_first_match() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::new();
    let inbox = e.path().join("s/in.jsonl");
    add(&e, &inbox, LINE1);
    add(&e, &inbox, LINE2);
    std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o640)).unwrap();
    add(&e, &inbox, LINE1);
    let filed = e.path().join("s/in.jsonl.filed.jsonl");
    let r = run_args(e.gate(), &["--mark-filed", inbox.to_str().unwrap(), LINE1]);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    assert_eq!(read(&inbox), format!("{LINE2}\n{LINE1}\n"));
    assert_eq!(read(&filed), format!("{LINE1}\n"));
    let mode = std::fs::metadata(&inbox).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);

    let before = read(&inbox);
    run_args(
        e.gate(),
        &[
            "--mark-filed",
            inbox.to_str().unwrap(),
            r#"{"ts":"x","title":"nomatch","detail":"x"}"#,
        ],
    );
    assert_eq!(read(&inbox), before);
    assert_eq!(lines(&filed), 1);
    // 一時ファイルが残らない
    let leftovers: Vec<_> = std::fs::read_dir(e.path().join("s"))
        .unwrap()
        .map(|d| d.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let mut leftovers = leftovers;
    leftovers.sort();
    assert_eq!(
        leftovers,
        ["in.jsonl", "in.jsonl.filed.jsonl", "in.jsonl.lock"]
    );

    // inbox 不在は何もしない(lock も作らない)
    let none = e.path().join("none/in.jsonl");
    let r = run_args(e.gate(), &["--mark-filed", none.to_str().unwrap(), LINE1]);
    assert_eq!(r.code, 0);
    assert!(!none.parent().unwrap().exists());
}

/// bash の既知の癖を固定する: 末尾改行の無い inbox は、不一致でも awk が
/// 改行を足すため「変化あり」と判定され、tombstone に対象行が書かれる。
#[test]
fn mark_filed_missing_trailing_newline_quirk() {
    let e = Env::new();
    let inbox = e.path().join("q.jsonl");
    std::fs::write(&inbox, "a\nb").unwrap();
    run_args(e.gate(), &["--mark-filed", inbox.to_str().unwrap(), "zzz"]);
    assert_eq!(read(&inbox), "a\nb\n");
    assert_eq!(read(&e.path().join("q.jsonl.filed.jsonl")), "zzz\n");
}

/// selftest: 「--check-dup はヒット時 exit 1」「--check-dup は非ヒット時 exit 0」
/// 「--check-dup は gh 失敗時 exit 3」「--check-dup は gh 失敗の理由を stderr に
/// 出す」「--check-dup は repo 指定でも非ヒット時 exit 0」「--check-dup は repo
/// を -R として gh issue list に渡す」
#[test]
fn check_dup() {
    let e = Env::new();
    let mut c = e.gate();
    c.env("WRAPUP_STUB_DUP", "1");
    // 重複時は一致した Issue 番号を stdout に出す(再発コメントの宛先)
    let r = run_args(c, &["--check-dup", "dup title"]);
    assert_eq!((r.code, r.stdout.as_str()), (1, "42\n"));
    let mut c = e.gate();
    c.env("WRAPUP_STUB_DUP", "1");
    assert_eq!(run_args(c, &["--check-dup", "dup"]).code, 0);
    let r = run_args(e.gate(), &["--check-dup", "dup title"]);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    assert_eq!(
        read(&e.gh_log()),
        "issue list --state open --search in:title dup title --json number,title\n\
         issue list --state open --search in:title dup --json number,title\n\
         issue list --state open --search in:title dup title --json number,title\n"
    );

    let mut c = e.gate();
    c.env("WRAPUP_STUB_FAIL", "1");
    let r = run_args(c, &["--check-dup", "dup title"]);
    assert_eq!(r.code, 3);
    assert_eq!(
        r.stderr,
        "check-dup: gh issue list failed: GraphQL: API rate limit already exceeded \n"
    );

    std::fs::write(e.gh_log(), "").unwrap();
    let r = run_args(e.gate(), &["--check-dup", "dup title", "acme/bleep"]);
    assert_eq!(r.code, 0);
    assert_eq!(
        read(&e.gh_log()),
        "issue list -R acme/bleep --state open --search in:title dup title --json number,title\n"
    );
    // repo が空文字なら -R を付けない
    std::fs::write(e.gh_log(), "").unwrap();
    run_args(e.gate(), &["--check-dup", "t", ""]);
    assert_eq!(
        read(&e.gh_log()),
        "issue list --state open --search in:title t --json number,title\n"
    );
}

/// gh の出力が想定外(不正 JSON・配列でない)なら重複なし扱い(jq の失敗 → exit 0)。
#[test]
fn check_dup_odd_gh_output() {
    let e = Env::new();
    for (out, want) in [
        ("not json", 0),
        (r#"{"a":{"title":"t"}}"#, 1),
        (r#"[1,{"title":"t"}]"#, 0),
        (r#"[{"title":"t"},1]"#, 1),
        (r#"[null,{"title":"t"}]"#, 1),
        ("", 0),
    ] {
        write_exec(
            &e.bin_dir().join("gh"),
            &format!("#!/bin/sh\ncat <<'X'\n{out}\nX\n"),
        );
        let r = run_args(e.gate(), &["--check-dup", "t"]);
        assert_eq!(r.code, want, "{out}");
    }
}

/// verdict-escalate は Stop のたびに呼ばれ、追記した行は inbox に残る。行があっても Stop は
/// 素通りする(裁定は backlog-cleanup)。verdict-escalate 未配備でも素通り。
#[test]
fn verdict_escalate_wiring() {
    let e = Env::new();
    let ve = e.path().join("ve-bin/verdict-escalate");
    write_exec(
        &ve,
        r#"#!/usr/bin/env bash
inbox=""
while [[ $# -gt 0 ]]; do
  [[ "$1" == "--inbox" ]] && inbox="$2"
  shift
done
cat >"$(dirname "$0")/stdin.txt"
[[ -n "$inbox" ]] && printf '%s\n' '{"ts":"x","title":"escalated","detail":"d","repo":"tarotene/bleep","go":"ask"}' >>"$inbox"
echo ve-stdout
echo ve-stderr >&2
"#,
    );
    let repo1 = repo(
        &e.path().join("ve_repo"),
        Some("https://github.com/example/verdict-escalate-test.git"),
    );
    // stub は親ディレクトリを作らない(実物は --add 経由で作る)
    std::fs::create_dir_all(e.state().join("claude/wrapup")).unwrap();
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &repo1)
        .env("WRAPUP_VERDICT_ESCALATE_BIN", &ve);
    let input = format!("{}\n\n", hook_input(&repo1));
    let r = run(c, &input);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains("ve-stderr"));
    // verdict-escalate の stdout は hook の stdout にそのまま流れる
    assert_eq!(r.stdout, "ve-stdout\n");
    // stdin は `$(cat)` 由来で末尾改行が落ちたものが渡る
    assert_eq!(read(&e.path().join("ve-bin/stdin.txt")), hook_input(&repo1));
    let inbox = inbox_path(&e, &repo1);
    assert!(read(Path::new(&inbox)).contains(r#""title":"escalated""#));

    let repo2 = repo(
        &e.path().join("ve_repo2"),
        Some("https://github.com/example/verdict-escalate-test2.git"),
    );
    let r = stop(&e, &repo2);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

/// selftest: 「feedback: stamp 未設定・空 inbox は素通り」
/// 「--stamp-feedback-session はスタンプファイルを作る」「feedback: #N 無しは
/// ゲート発動(空 inbox でも)」「feedback: メッセージが feedback-memory を含む」
/// 「feedback: #N ありは素通り」「feedback: type!=feedback は対象外」
#[test]
fn feedback_memory_check() {
    let e = Env::new();
    let fb = repo(
        &e.path().join("fb_repo"),
        Some("https://github.com/example/feedback-test.git"),
    );
    let input = format!(
        r#"{{"cwd":"{}","session_id":"fbsid","stop_hook_active":false}}"#,
        fb.display()
    );
    let mem = e.memory_dir().join("proj1/memory");
    std::fs::create_dir_all(&mem).unwrap();
    let stop_fb = || {
        let mut c = e.gate();
        c.env("CLAUDE_PROJECT_DIR", &fb);
        run(c, &input)
    };
    let r = stop_fb();
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));

    let r = run_args(e.gate(), &["--stamp-feedback-session", "fbsid"]);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    let stamp = e.stamp_dir().join("fbsid.stamp");
    assert!(stamp.is_file());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(e.stamp_dir())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    backdate(&stamp);

    let unlinked = mem.join("unlinked.md");
    std::fs::write(
        &unlinked,
        "---\nname: unlinked\ndescription: test\nmetadata:\n  type: feedback\n---\n\n本文に Issue 番号が無い。\n",
    )
    .unwrap();
    let r = stop_fb();
    assert_eq!(r.code, 2);
    assert_eq!(r.stderr, format!("{}\n", feedback_block(&[&unlinked])));

    std::fs::write(
        &unlinked,
        "---\nname: unlinked\ndescription: test\nmetadata:\n  type: feedback\n---\n\n対応: #123 起票済み。\n",
    )
    .unwrap();
    let r = stop_fb();
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));

    std::fs::write(
        mem.join("other.md"),
        "---\nname: other\ndescription: test\nmetadata:\n  type: project\n---\n\nfeedback ではない。\n",
    )
    .unwrap();
    let r = stop_fb();
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

/// inbox に行があっても、Stop の出力は feedback の節だけ。memory の
/// 探索は `-path '*/memory/*.md'`(下位ディレクトリも含む)・`-type f`・
/// `-newer stamp` に従う。session_id はサニタイズされる。
#[test]
fn feedback_only_even_with_an_inbox() {
    let e = Env::new();
    let fb = repo(
        &e.path().join("fb"),
        Some("https://github.com/example/combined.git"),
    );
    let sid = "a/b c";
    run_args(e.gate(), &["--stamp-feedback-session", sid]);
    let stamp = e.stamp_dir().join("a_b_c.stamp");
    assert!(stamp.is_file());
    backdate(&stamp);
    let deep = e.memory_dir().join("p/memory/sub");
    std::fs::create_dir_all(&deep).unwrap();
    let body = "  type:   feedback  \nno ref\n";
    let f1 = deep.join("x.md");
    std::fs::write(&f1, body).unwrap();
    // 対象外: memory/ 配下でない・.md でない・old(stamp より古い)
    std::fs::create_dir_all(e.memory_dir().join("p/notes")).unwrap();
    std::fs::write(e.memory_dir().join("p/notes/y.md"), body).unwrap();
    std::fs::write(e.memory_dir().join("p/memory/z.txt"), body).unwrap();
    let old = e.memory_dir().join("p/memory/old.md");
    std::fs::write(&old, body).unwrap();
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(t)
        .unwrap();

    let inbox = inbox_path(&e, &fb);
    add(&e, Path::new(&inbox), LINE1);
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &fb);
    let r = run(
        c,
        &format!(r#"{{"cwd":"/nonexistent","session_id":"{sid}"}}"#),
    );
    assert_eq!(r.code, 2);
    assert_eq!(r.stderr, format!("{}\n", feedback_block(&[&f1])));
}

/// project の解決: CLAUDE_PROJECT_DIR が無ければ `.cwd`、両方無ければ素通り。
#[test]
fn project_from_cwd_and_empty() {
    let e = Env::new();
    let r = run(e.gate(), r#"{"stop_hook_active":false}"#);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/cwd.git"),
    );
    arm_feedback(&e, "cwd-sid");
    let r = run(e.gate(), &hook_input_sid(&repo, "cwd-sid"));
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(r.stderr.contains("type: feedback"), "{}", r.stderr);
}

/// usage エラー(`${2:?usage: ...}`): exit 1、stderr 末尾に usage 文言。
#[test]
fn usage_errors() {
    let e = Env::new();
    for (args, usage) in [
        (
            &["--inbox-path"][..],
            "usage: wrapup-stop-gate.sh --inbox-path <project-dir>",
        ),
        (
            &["--set-status"][..],
            "usage: issue-inbox --set-status <title> <fixed|issued|dropped> [url] [--repo R]",
        ),
        (
            &["--migrate", ""][..],
            "usage: wrapup-stop-gate.sh --migrate <project-dir>",
        ),
        (
            &["--stamp-feedback-session"][..],
            "usage: wrapup-stop-gate.sh --stamp-feedback-session <session_id>",
        ),
        (
            &["--add", "/x"][..],
            "usage: wrapup-stop-gate.sh --add <inbox> <json>",
        ),
        (
            &["--add"][..],
            "usage: wrapup-stop-gate.sh --add <inbox> <json>",
        ),
        (
            &["--check-dup"][..],
            "usage: wrapup-stop-gate.sh --check-dup <title> [repo]",
        ),
        (
            &["--mark-filed", "/x", ""][..],
            "usage: wrapup-stop-gate.sh --mark-filed <inbox> <json>",
        ),
    ] {
        let r = run_args(e.gate(), args);
        assert_eq!(r.code, 1, "{args:?}");
        assert_eq!(r.stdout, "", "{args:?}");
        assert!(
            r.stderr.ends_with(&format!(": {usage}\n")),
            "{args:?}: {}",
            r.stderr
        );
    }
}

/// `--add` / `--mark-filed` は `$inbox.lock` の flock(2) で排他する
/// (bash 版の `flock 9` と同じロックなので、両実装が混在しても排他が成立する)。
#[test]
fn add_waits_for_inbox_lock() {
    let e = Env::new();
    let inbox = e.path().join("l.jsonl");
    let lock = std::fs::File::options()
        .create(true)
        .append(true)
        .open(e.path().join("l.jsonl.lock"))
        .unwrap();
    lock.lock().unwrap();
    let mut c = e.gate();
    c.args(["--add", inbox.to_str().unwrap(), LINE1])
        .stdin(std::process::Stdio::null());
    let mut child = c.spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        child.try_wait().unwrap().is_none(),
        "--add must block on the lock"
    );
    assert!(!inbox.exists());
    lock.unlock().unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(read(&inbox), format!("{LINE1}\n"));
}
