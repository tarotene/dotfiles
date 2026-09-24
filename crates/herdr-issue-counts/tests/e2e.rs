//! 実バイナリを end-to-end で走らせる。`herdr` と `gh` は PATH 先頭の stub に
//! 差し替え(本物の herdr サーバや GitHub には触れない)、git は一時ディレクトリの
//! 本物のリポジトリを使う。stub の herdr は受け取った report-metadata の引数を
//! ログファイルに 1 行ずつ残し、テストはそれを検証する。
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    tmp: tempfile::TempDir,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        Fixture { tmp, bin }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    /// `remote` を 1 つ持つ git リポジトリを作り、そのパスを返す。
    fn repo(&self, name: &str, remote_url: Option<&str>) -> PathBuf {
        let dir = self.path(name);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        if let Some(url) = remote_url {
            git(&dir, &["remote", "add", "origin", url]);
        }
        dir
    }

    fn stub(&self, name: &str, script: &str) {
        let p = self.bin.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{script}")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// herdr stub: `workspace list` は `list.json` を出し、`report-metadata` は
    /// 引数を `reports.log` に追記する。`list.json` が無ければサーバ未起動を装う。
    fn stub_herdr(&self, workspaces_json: Option<&str>, fail_report: bool) {
        if let Some(json) = workspaces_json {
            std::fs::write(self.path("list.json"), json).unwrap();
        }
        let log = self.path("reports.log");
        let list = self.path("list.json");
        let report_exit = if fail_report { 1 } else { 0 };
        self.stub(
            "herdr",
            &format!(
                r#"case "$1 $2" in
  "workspace list")
    if [ -f '{list}' ]; then cat '{list}'; exit 0; fi
    echo '{{"error":{{"code":"server_not_running"}}}}'; exit 1 ;;
  "workspace report-metadata")
    shift 2; echo "$*" >>'{log}'; exit {report_exit} ;;
esac
exit 99
"#,
                list = list.display(),
                log = log.display(),
            ),
        );
    }

    /// gh stub: `auth status` は `authed` に従い、`api graphql` は `graphql.json`
    /// を出して `graphql_exit` で終わる(部分エラー時の gh と同じく非 0 + data)。
    /// 受け取ったクエリは `query.txt` に残す。
    fn stub_gh(&self, authed: bool, graphql_json: &str, graphql_exit: i32) {
        std::fs::write(self.path("graphql.json"), graphql_json).unwrap();
        let auth_exit = if authed { 0 } else { 1 };
        self.stub(
            "gh",
            &format!(
                r#"case "$1 $2" in
  "auth status") echo 'not logged in' >&2; exit {auth_exit} ;;
  "api graphql") printf '%s' "$4" >'{query}'; cat '{json}'; exit {graphql_exit} ;;
esac
exit 99
"#,
                query = self.path("query.txt").display(),
                json = self.path("graphql.json").display(),
            ),
        );
    }

    fn run(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_herdr-issue-counts"))
            .args(args)
            .env("PATH", path)
            .output()
            .unwrap()
    }

    fn reports(&self) -> Vec<String> {
        std::fs::read_to_string(self.path("reports.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn workspaces(entries: &[(&str, Option<&Path>)]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|(id, root)| match root {
            Some(r) => format!(
                r#"{{"workspace_id":"{id}","worktree":{{"repo_root":"{}"}}}}"#,
                r.display()
            ),
            None => format!(r#"{{"workspace_id":"{id}"}}"#),
        })
        .collect();
    format!(
        r#"{{"id":"cli:workspace:list","result":{{"workspaces":[{}]}}}}"#,
        items.join(",")
    )
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn reports_each_workspace_with_one_query_per_repo() {
    let f = Fixture::new();
    let a = f.repo("a", Some("https://github.com/o/a.git"));
    let b = f.repo("b", Some("git@github.com:o/b.git"));
    let local = f.repo("local", None);
    // w1/w2 は同じリポジトリ(worktree を想定)、w3 は GitHub remote 無し、
    // w4 は worktree 情報無し。
    f.stub_herdr(
        Some(&workspaces(&[
            ("w1", Some(&a)),
            ("w2", Some(&a)),
            ("w3", Some(&local)),
            ("w4", None),
            ("w5", Some(&b)),
        ])),
        false,
    );
    f.stub_gh(
        true,
        r#"{"data":{"r0":{"issues":{"totalCount":27}},"r1":{"issues":{"totalCount":0}}}}"#,
        0,
    );

    let out = f.run(&[]);
    assert!(out.status.success(), "{}", stderr(&out));

    let query = std::fs::read_to_string(f.path("query.txt")).unwrap();
    assert_eq!(query.matches("repository(").count(), 2, "{query}");
    assert!(
        query.contains(r#"r0: repository(owner: "o", name: "a")"#),
        "{query}"
    );
    assert!(
        query.contains(r#"r1: repository(owner: "o", name: "b")"#),
        "{query}"
    );

    let reports = f.reports();
    assert_eq!(reports.len(), 3, "{reports:?}");
    for (line, (ws, value)) in reports
        .iter()
        .zip([("w1", "27"), ("w2", "27"), ("w5", "0")])
    {
        assert!(
            line.starts_with(&format!(
                "{ws} --source issue-counts --token issues=\u{f41b} {value} --seq "
            )),
            "{line}"
        );
        assert!(line.ends_with(" --ttl-ms 900000"), "{line}");
    }
}

#[test]
fn dry_run_prints_table_without_reporting() {
    let f = Fixture::new();
    let a = f.repo("a", Some("https://github.com/o/a.git"));
    f.stub_herdr(Some(&workspaces(&[("w1", Some(&a))])), false);
    f.stub_gh(true, r#"{"data":{"r0":{"issues":{"totalCount":3}}}}"#, 0);

    let out = f.run(&["--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "w1\to/a\t3\n");
    assert!(f.reports().is_empty());
}

#[test]
fn partial_graphql_error_skips_only_that_repo() {
    let f = Fixture::new();
    let a = f.repo("a", Some("https://github.com/o/a.git"));
    let gone = f.repo("gone", Some("https://github.com/o/gone.git"));
    f.stub_herdr(
        Some(&workspaces(&[("w1", Some(&a)), ("w2", Some(&gone))])),
        false,
    );
    // gh は部分エラーで exit 1 だが stdout に data が入る(2026-09-24 実測)。
    f.stub_gh(
        true,
        r#"{"data":{"r0":{"issues":{"totalCount":5}},"r1":null},"errors":[{"path":["r1"],"message":"Could not resolve"}]}"#,
        1,
    );

    let out = f.run(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("o/gone を取得できない"),
        "{}",
        stderr(&out)
    );
    let reports = f.reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].starts_with("w1 "), "{reports:?}");
}

#[test]
fn herdr_not_running_is_skipped_with_exit_zero() {
    let f = Fixture::new();
    f.stub_herdr(None, false);
    f.stub_gh(true, "{}", 0);

    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).contains("skip"), "{}", stderr(&out));
}

#[test]
fn gh_unauthenticated_is_skipped_with_exit_zero() {
    let f = Fixture::new();
    let a = f.repo("a", Some("https://github.com/o/a.git"));
    f.stub_herdr(Some(&workspaces(&[("w1", Some(&a))])), false);
    f.stub_gh(false, "{}", 0);

    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).contains("未認証"), "{}", stderr(&out));
    assert!(f.reports().is_empty());
}

#[test]
fn no_github_workspace_is_skipped_with_exit_zero() {
    let f = Fixture::new();
    let local = f.repo("local", Some("https://gitlab.com/o/a.git"));
    f.stub_herdr(Some(&workspaces(&[("w1", Some(&local))])), false);
    f.stub_gh(true, "{}", 0);

    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!f.path("query.txt").exists(), "GraphQL を呼ばないこと");
}

#[test]
fn graphql_failure_without_data_exits_nonzero() {
    let f = Fixture::new();
    let a = f.repo("a", Some("https://github.com/o/a.git"));
    f.stub_herdr(Some(&workspaces(&[("w1", Some(&a))])), false);
    f.stub_gh(true, r#"{"errors":[{"message":"Bad credentials"}]}"#, 1);

    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(f.reports().is_empty());
}

#[test]
fn report_failure_exits_nonzero_but_tries_every_workspace() {
    let f = Fixture::new();
    let a = f.repo("a", Some("https://github.com/o/a.git"));
    f.stub_herdr(
        Some(&workspaces(&[("w1", Some(&a)), ("w2", Some(&a))])),
        true,
    );
    f.stub_gh(true, r#"{"data":{"r0":{"issues":{"totalCount":1}}}}"#, 0);

    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(f.reports().len(), 2);
}
