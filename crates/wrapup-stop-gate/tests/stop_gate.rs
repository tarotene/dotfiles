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

/// 期待される Stop 出力の inbox 節(`printf %q` の結果は、テストの一時パスが
/// クオート不要の文字だけでできている前提で素のまま埋める)。
fn stop_inbox_block(count: usize, inbox: &str, name_q: &str, url_q: &str) -> String {
    let gate = gate_path();
    format!(
        "<hook-directive source=\"wrapup-stop-gate\" event=\"Stop\">\n\
         {count} unfiled item(s) in the wrap-up inbox: {inbox}\n\
         Run `ATTRIBUTION_AGENT_NAME={name_q} ATTRIBUTION_AGENT_URL={url_q} bash {gate} --procedure {inbox}` and follow its output.\n\
         </hook-directive>",
        gate = gate.display()
    )
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
         that does not generalize, or sensitive content (security, personal data), is\n\
         exempt.\n\
         </hook-directive>",
        n = paths.len()
    )
}

fn procedure_text(inbox: &str, name: &str, url: &str) -> String {
    let gate = gate_path();
    let gate = gate.display();
    format!(
        r#"<hook-directive source="wrapup-stop-gate" kind="procedure">
Each line of {inbox} is one JSONL item (ts/title/detail, optionally repo/go).
Process the lines one by one:
  1. Run bash '{gate}' --check-dup "<title>" [repo] (pass repo if the line
     has one; otherwise the target is this project's repository).
     exit 1 means an open Issue with the same title already exists (duplicate).
     exit 3 means the check could not be made — skip that line this time and
     leave it in the inbox.
  2. If the line has no "go":"ask" and is not a duplicate, file it with
     gh issue create [-R <repo>] --title "<title>" --body "<body>".
  3. If the line has "go":"ask" (auto-aggregated from the verdict ledger,
     ADR-478), do not file it right away even when it is not a duplicate.
     Show title, detail, and repo (the default target) via AskUserQuestion with
     the choices 「このまま <repo> に起票する」「別のリポジトリに振り直す」
     「今回は起票しない」, then act on the answer (re-routing only changes the
     -R target). gh-edit-allow may auto-allow gh issue create based on earlier
     creations in the same session, so the absence of a permission prompt is
     not a GO — always confirm with AskUserQuestion.
  4. Write the body from detail plus the conversation context, and end it with
     this line (the provenance footer, grep-able for inbox-origin Issues, also
     serves as the attribution that attribution-guard.sh requires):
       「🤖 Filed from [{name}]({url}) wrap-up inbox」
  5. Remove only the lines that were filed, skipped as duplicates, or declined
     with 「今回は起票しない」 in step 3, using
     bash '{gate}' --mark-filed '{inbox}' '<the line verbatim>'.
     If gh issue create fails, do not call --mark-filed; the line stays in the
     inbox for a retry on the next turn.
Do not edit the inbox directly (always go through --add / --mark-filed).
</hook-directive>
"#
    )
}

/// 「Stop 出力から --procedure コマンドを抽出できる」の sed と同じ抽出。
fn extract_procedure_cmd(stderr: &str) -> Option<String> {
    stderr.lines().find_map(|l| {
        l.strip_prefix("Run `")
            .and_then(|r| r.strip_suffix("` and follow its output."))
            .map(str::to_string)
    })
}

fn sh(e: &Env, cmd: &str) -> Run {
    let mut c = std::process::Command::new("bash");
    c.arg("-c").arg(cmd);
    // base 環境(HOME 等)を揃えるため gate() の env を借りず、最低限だけ渡す。
    c.env("HOME", e.home())
        .env_remove("ATTRIBUTION_AGENT_NAME")
        .env_remove("ATTRIBUTION_AGENT_URL");
    run(c, "")
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

/// selftest: 「非空 inbox でゲート発動」「ゲートは stderr に指示を出す」
/// 「Stop 出力は hook-directive 外枠 4 行に収まる」「Stop 出力から --procedure
/// コマンドを抽出できる」「--procedure は既定フッターを含む」
/// 「--procedure は差し替えフッターを引き継ぐ」
#[test]
fn gate_fires_and_points_to_procedure() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let inbox = inbox_path(&e, &repo);
    add(&e, Path::new(&inbox), LINE1);
    add(&e, Path::new(&inbox), LINE2);

    let r = stop(&e, &repo);
    assert_eq!(r.code, 2);
    assert_eq!(r.stdout, "");
    let want = stop_inbox_block(2, &inbox, r"Claude\ Code", "https://claude.com/claude-code");
    assert_eq!(r.stderr, format!("{want}\n"));
    assert_eq!(r.stderr.lines().count(), 4);

    let cmd = extract_procedure_cmd(&r.stderr).expect("procedure cmd");
    let p = sh(&e, &cmd);
    assert_eq!(p.code, 0);
    assert_eq!(
        p.stdout,
        procedure_text(&inbox, "Claude Code", "https://claude.com/claude-code")
    );

    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &repo)
        .env("ATTRIBUTION_AGENT_NAME", "Codex CLI")
        .env("ATTRIBUTION_AGENT_URL", "https://example.com/codex");
    let r = run(c, &hook_input(&repo));
    assert_eq!(r.code, 2);
    assert_eq!(
        r.stderr,
        format!(
            "{}\n",
            stop_inbox_block(2, &inbox, r"Codex\ CLI", "https://example.com/codex")
        )
    );
    let cmd = extract_procedure_cmd(&r.stderr).unwrap();
    let p = sh(&e, &cmd);
    assert!(p
        .stdout
        .contains("Filed from [Codex CLI](https://example.com/codex) wrap-up inbox"));
}

/// `--procedure` の全文(env で差し替えたフッター込み)。
#[test]
fn procedure_full_text() {
    let e = Env::new();
    let mut c = e.gate();
    c.env("ATTRIBUTION_AGENT_NAME", "N")
        .env("ATTRIBUTION_AGENT_URL", "U");
    let r = run_args(c, &["--procedure", "/x/y.jsonl"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, procedure_text("/x/y.jsonl", "N", "U"));
    // 空文字の env は未設定扱い(bash の `:-`)
    let mut c = e.gate();
    c.env("ATTRIBUTION_AGENT_NAME", "");
    let r = run_args(c, &["--procedure", "/x/y.jsonl"]);
    assert!(r
        .stdout
        .contains("[Claude Code](https://claude.com/claude-code)"));
}

/// selftest: 「gh 不在で素通り」
#[test]
fn no_gh_passes() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let inbox = inbox_path(&e, &repo);
    add(&e, Path::new(&inbox), LINE1);
    let nogh = e.path().join("nogh");
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
    let r = run(c, &hook_input(&repo));
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

/// selftest: 「GitHub remote なしで素通り」「git repo 外で素通り」
#[test]
fn no_github_remote_or_no_repo_passes() {
    let e = Env::new();
    let norepo = repo(&e.path().join("norepo"), None);
    let plain = e.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let gitlab = repo(
        &e.path().join("gitlab"),
        Some("https://gitlab.com/example/example.git"),
    );
    for p in [&norepo, &plain, &gitlab] {
        let inbox = inbox_path(&e, p);
        add(&e, Path::new(&inbox), LINE1);
        let r = stop(&e, p);
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
    assert_eq!(run_args(c, &["--check-dup", "dup title"]).code, 1);
    let mut c = e.gate();
    c.env("WRAPUP_STUB_DUP", "1");
    assert_eq!(run_args(c, &["--check-dup", "dup"]).code, 0);
    let r = run_args(e.gate(), &["--check-dup", "dup title"]);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "", ""));
    assert_eq!(
        read(&e.gh_log()),
        "issue list --state open --search in:title dup title --json title\n\
         issue list --state open --search in:title dup --json title\n\
         issue list --state open --search in:title dup title --json title\n"
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
        "issue list -R acme/bleep --state open --search in:title dup title --json title\n"
    );
    // repo が空文字なら -R を付けない
    std::fs::write(e.gh_log(), "").unwrap();
    run_args(e.gate(), &["--check-dup", "t", ""]);
    assert_eq!(
        read(&e.gh_log()),
        "issue list --state open --search in:title t --json title\n"
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

/// selftest: 「verdict-escalate が追記した行だけでもゲート発動」「Stop 出力は
/// --procedure へのポインタを含む」「手順書に go:ask の扱いが含まれる」
/// 「verdict-escalate 未配備でも素通り」
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
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("--procedure"));
    assert!(!r.stderr.contains("ve-stderr"));
    // verdict-escalate の stdout は hook の stdout にそのまま流れる
    assert_eq!(r.stdout, "ve-stdout\n");
    // stdin は `$(cat)` 由来で末尾改行が落ちたものが渡る
    assert_eq!(read(&e.path().join("ve-bin/stdin.txt")), hook_input(&repo1));
    let inbox = inbox_path(&e, &repo1);
    assert!(read(Path::new(&inbox)).contains(r#""go":"ask""#));
    let r = run_args(e.gate(), &["--procedure", "/x.jsonl"]);
    assert!(r.stdout.contains(r#""go":"ask""#));

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

/// stamp を 1 時間前に戻す(selftest の `touch -d '1 hour ago'`)。
fn backdate(p: &Path) {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

/// inbox と feedback の両方があるときは 2 節を空行で区切って出す。memory の
/// 探索は `-path '*/memory/*.md'`(下位ディレクトリも含む)・`-type f`・
/// `-newer stamp` に従う。session_id はサニタイズされる。
#[test]
fn feedback_and_inbox_combined() {
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
    assert_eq!(
        r.stderr,
        format!(
            "{}\n\n{}\n",
            stop_inbox_block(1, &inbox, r"Claude\ Code", "https://claude.com/claude-code"),
            feedback_block(&[&f1])
        )
    );
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
    let inbox = inbox_path(&e, &repo);
    add(&e, Path::new(&inbox), LINE1);
    let r = run(e.gate(), &hook_input(&repo));
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains(&inbox));
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
            &["--procedure"][..],
            "usage: wrapup-stop-gate.sh --procedure <inbox>",
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
