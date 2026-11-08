//! 本人の Issue 台帳の生存の記録と逆向きの確認(ADR-0000)の、main.rs のワイヤリングの結合テスト。
//! 偽の `gh`・`herdr`(固定応答を返し、呼ばれた引数を残すスクリプト)と、一時ディレクトリの設定・vault で動かす。
//! 判定そのもの(35/37 時間の境、1 日 1 回、書式)は `ledger_heartbeat.rs` の単体テストが検査する。
//! 台帳のリポジトリは非公開で、ここには書かない。架空の `example/ledger` を使う。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// 2026-11-08T06:00:00Z。
const NOW: i64 = 1_794_117_600;
const H: i64 = 3600;

fn write_exec(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm).unwrap();
}

fn asof(unix: i64) -> String {
    promotion_detect::ledger_heartbeat::format_asof(unix)
}

fn days_ago(n: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    issue_inbox::vault::jst_date(now - n * 86_400)
}

struct World {
    tmp: tempfile::TempDir,
}

impl World {
    /// 偽の `gh`・`herdr`・`wrapup-stop-gate`・`worklog`、vault、設定(`example/ledger`)を用意する。
    /// `age_hours` は digest の最新のコメントの古さ(`None` ならコメントが無い)。
    fn new(age_hours: Option<i64>) -> World {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        for dir in [
            "claude",
            "state",
            "config/ledger-heartbeat",
            "vault/.obsidian",
        ] {
            fs::create_dir_all(d.join(dir)).unwrap();
        }
        fs::write(d.join("vault/.obsidian/app.json"), "{}").unwrap();
        fs::write(d.join("config/ledger-heartbeat/target"), "example/ledger\n").unwrap();
        fs::write(
            d.join("status-issues.json"),
            serde_json::json!([
                {"number": 11, "title": "ledger-heartbeat", "body": "asof: old"},
                {"number": 12, "title": "ledger-digest", "body": ""},
            ])
            .to_string(),
        )
        .unwrap();
        let nodes: Vec<_> = age_hours
            .map(|h| serde_json::json!({"createdAt": asof(NOW - h * H)}))
            .into_iter()
            .collect();
        fs::write(
            d.join("comments.json"),
            serde_json::json!({"data":{"repository":{"issue":{"comments":{"nodes":nodes}}}}})
                .to_string(),
        )
        .unwrap();
        fs::write(d.join("feedback.json"), "[]").unwrap();
        // 引数の先頭 2 語を gh.log に 1 行ずつ残し、`issue edit` の `--body`(7 番目)を edit-body に残す。
        // d/fail があれば、台帳に対する呼び出し(ラベルでの一覧・graphql・edit)だけが失敗する。
        write_exec(
            &d.join("gh"),
            &format!(
                "#!/bin/sh\necho \"$1 $2\" >> '{d}/gh.log'\n\
                 if [ -f '{d}/fail' ]; then case \"$*\" in *ledger-status*|*graphql*|*'issue edit'*) exit 1;; esac; fi\n\
                 case \"$1 $2\" in\n\
                 'issue list') case \"$*\" in *ledger-status*) cat '{d}/status-issues.json';; *) cat '{d}/feedback.json';; esac;;\n\
                 'api graphql') cat '{d}/comments.json';;\n\
                 'issue edit') printf '%s' \"$7\" > '{d}/edit-body'; echo \"$*\" > '{d}/edit-args';;\n\
                 *) exit 1;;\n\
                 esac\n",
                d = d.display()
            ),
        );
        write_exec(
            &d.join("herdr"),
            &format!(
                "#!/bin/sh\necho \"$*\" >> '{d}/herdr.log'\necho '{{\"result\":{{\"shown\":true,\"reason\":null}}}}'\n",
                d = d.display()
            ),
        );
        write_exec(
            &d.join("wrapup-stop-gate"),
            "#!/bin/sh\ncase \"$1\" in --check-dup) exit 0;; esac\n",
        );
        write_exec(&d.join("worklog"), "#!/bin/sh\nexit 0\n");
        World { tmp }
    }

    fn p(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    fn run(&self, dry_run: bool) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_promotion-detect"));
        cmd.env("HOME", "/nonexistent-home")
            .env_remove("XDG_STATE_HOME")
            .env("XDG_CONFIG_HOME", self.p("config"))
            .env("OBSIDIAN_VAULT", self.p("vault"))
            .env("PROMOTION_DETECT_GH_BIN", self.p("gh"))
            .env("PROMOTION_DETECT_HERDR_BIN", self.p("herdr"))
            .env("PROMOTION_DETECT_CLAUDE_DIR", self.p("claude"))
            .env("PROMOTION_DETECT_STATE_DIR", self.p("state"))
            .env("PROMOTION_DETECT_WRAPUP_BIN", self.p("wrapup-stop-gate"))
            .env("PROMOTION_DETECT_INBOX_PATH", self.p("inbox.jsonl"))
            .env("PROMOTION_DETECT_WORKLOG_BIN", self.p("worklog"))
            .env("PROMOTION_DETECT_NOW", NOW.to_string());
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.output().unwrap()
    }

    fn finding(&self, name: &str, repo: &str, status: &str, last: &str) {
        let inbox = self.p("vault/inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(
            inbox.join(format!("{name}.md")),
            format!(
                "---\ntitle: \"{name}\"\nrepo: \"{repo}\"\nstatus: \"{status}\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"{last}\"\n---\n- {last} d\n"
            ),
        )
        .unwrap();
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.p(name)).unwrap_or_default()
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn without_the_config_file_nothing_about_the_ledger_is_called() {
    let w = World::new(Some(48));
    fs::remove_file(w.p("config/ledger-heartbeat/target")).unwrap();
    let out = w.run(false);
    assert!(out.status.success(), "{}", stderr(&out));
    // 一覧は feedback の 1 回だけ。ラベル `ledger-status` での一覧も graphql も edit も呼ばない。
    assert!(
        !w.read("gh.log").contains("graphql"),
        "{}",
        w.read("gh.log")
    );
    assert!(!w.p("edit-body").exists());
    assert!(!w.p("herdr.log").exists());
    assert!(!stderr(&out).contains("台帳"), "{}", stderr(&out));
}

#[test]
fn the_heartbeat_body_counts_open_findings_per_project_after_the_sweep() {
    let w = World::new(Some(1));
    w.finding("a", "example/a", "open", &days_ago(1));
    w.finding("b", "example/a", "open", &days_ago(2));
    w.finding("c", "example/b", "open", &days_ago(3));
    w.finding("d", "example/b", "fixed", &days_ago(3));
    // 14 日放置された open は、掃除で lapsed になってから数えるので、件数に入らない。
    w.finding("e", "example/c", "open", &days_ago(14));
    let out = w.run(false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        w.read("edit-body"),
        format!(
            "asof: {}\ninbox example/a: 2\ninbox example/b: 1",
            asof(NOW)
        )
    );
    let args = w.read("edit-args");
    assert!(
        args.starts_with("issue edit 11 -R example/ledger --body"),
        "{args}"
    );
    // digest の最新のコメントは 1 時間前。通知は出さない。
    assert!(!w.p("herdr.log").exists());
}

#[test]
fn a_dry_run_prints_the_body_and_calls_nothing_about_the_ledger() {
    let w = World::new(Some(48));
    w.finding("a", "example/a", "open", &days_ago(1));
    let out = w.run(true);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("inbox example/a: 1"),
        "{}",
        stderr(&out)
    );
    assert!(!w.read("gh.log").contains("graphql"));
    assert!(!w.p("edit-body").exists());
    assert!(!w.p("herdr.log").exists());
}

#[test]
fn a_digest_older_than_36_hours_notifies_once_a_day() {
    let w = World::new(Some(40));
    let first = w.run(false);
    assert!(first.status.success(), "{}", stderr(&first));
    let log = w.read("herdr.log");
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("notification show"), "{log}");
    assert!(log.contains("36 時間届いていない"), "{log}");
    assert!(log.contains("example/ledger#12"), "{log}");
    // 同じ日の次の run は、通知を重ねない。
    w.run(false);
    assert_eq!(w.read("herdr.log").lines().count(), 1);
}

#[test]
fn a_digest_with_no_comment_notifies() {
    let w = World::new(None);
    w.run(false);
    assert_eq!(w.read("herdr.log").lines().count(), 1);
}

#[test]
fn a_failing_gh_stops_nothing_else_in_the_daily_run() {
    let w = World::new(Some(48));
    fs::write(w.p("fail"), "").unwrap();
    // 台帳が失敗しても、別の検出(feedback Issue の再発)は inbox に流れる。
    fs::write(
        w.p("feedback.json"),
        serde_json::json!([
            {"number": 1, "body": "Target: hook/foo-gate\n本文"},
            {"number": 2, "body": "Target: hook/foo-gate\n本文2"},
        ])
        .to_string(),
    )
    .unwrap();
    write_exec(
        &w.p("wrapup-stop-gate"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in --check-dup) exit 0;; --add) echo added >> '{}';; esac\n",
            w.p("wrapup.log").display()
        ),
    );
    let out = w.run(false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("台帳の常設 Issue を読めない"),
        "{}",
        stderr(&out)
    );
    assert_eq!(w.read("wrapup.log").lines().count(), 1);
    assert!(
        !w.p("herdr.log").exists(),
        "読めなかったことを根拠に通知しない"
    );
}

#[test]
fn a_malformed_config_is_reported_and_skipped() {
    let w = World::new(Some(48));
    fs::write(w.p("config/ledger-heartbeat/target"), "not a repo\n").unwrap();
    let out = w.run(false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("OWNER/REPO の形でない"),
        "{}",
        stderr(&out)
    );
    assert!(!w.read("gh.log").contains("graphql"));
}

#[test]
fn a_machine_without_a_vault_does_not_claim_an_empty_inbox() {
    let w = World::new(Some(1));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_promotion-detect"));
    cmd.env("HOME", "/nonexistent-home")
        .env_remove("XDG_STATE_HOME")
        .env_remove("OBSIDIAN_VAULT")
        .env("XDG_CONFIG_HOME", w.p("config"))
        .env("PROMOTION_DETECT_GH_BIN", w.p("gh"))
        .env("PROMOTION_DETECT_HERDR_BIN", w.p("herdr"))
        .env("PROMOTION_DETECT_CLAUDE_DIR", w.p("claude"))
        .env("PROMOTION_DETECT_STATE_DIR", w.p("state"))
        .env("PROMOTION_DETECT_WRAPUP_BIN", w.p("wrapup-stop-gate"))
        .env("PROMOTION_DETECT_INBOX_PATH", w.p("inbox.jsonl"))
        .env("PROMOTION_DETECT_NOW", NOW.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!w.p("edit-body").exists());
}
