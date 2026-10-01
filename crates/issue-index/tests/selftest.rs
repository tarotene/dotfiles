//! `config/claude/hooks/issue-index.sh --selftest` の全ケースを、実際の一時 git
//! repo と gh スタブで再現する統合テスト(#413、docs/rust-migration.md の段 1-2)。
//!
//! テスト対象は既定で cargo bin(Rust 版)。`ISSUE_INDEX_ORACLE` に bash 版の
//! パスを入れると、同じケースを bash 版に向けて走らせる(移植前の緑確認用)。
//! 各ケースの stdout / stderr は `tests/expected/` のバイト列と完全一致させる
//! (bash 版から `ISSUE_INDEX_BLESS=1` で生成した)。
//!
//! gh スタブは bash selftest のものをそのまま使う(`ISSUE_INDEX_STUB_*` で制御)。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GH_STUB: &str = r#"#!/usr/bin/env bash
emit() { # $1=file変数名 $2=rc変数名 $3=err変数名 $4=既定JSON
  local f="${!1:-}" rc="${!2:-0}" e="${!3:-}"
  [[ -n "$e" ]] && printf '%s\n' "$e" >&2
  if [[ -n "$f" && -f "$f" ]]; then
    cat "$f"
  else
    printf '%s' "$4"
  fi
  exit "$rc"
}
case "$1" in
  pr)
    emit ISSUE_INDEX_STUB_PR_FILE ISSUE_INDEX_STUB_PR_RC ISSUE_INDEX_STUB_PR_ERR '[]'
    ;;
  api)
    case "$*" in
      *graphql*)
        emit ISSUE_INDEX_STUB_WHO_FILE ISSUE_INDEX_STUB_WHO_RC ISSUE_INDEX_STUB_WHO_ERR 'tester'
        ;;
      *"assignee:@me"*)
        emit ISSUE_INDEX_STUB_MINE_FILE ISSUE_INDEX_STUB_MINE_RC ISSUE_INDEX_STUB_MINE_ERR \
          '{"total_count":0,"incomplete_results":false,"items":[]}'
        ;;
      *"handoff:ai"*)
        emit ISSUE_INDEX_STUB_HANDOFF_FILE ISSUE_INDEX_STUB_HANDOFF_RC ISSUE_INDEX_STUB_HANDOFF_ERR \
          '{"total_count":0,"incomplete_results":false,"items":[]}'
        ;;
      *)
        emit ISSUE_INDEX_STUB_ALL_FILE ISSUE_INDEX_STUB_ALL_RC ISSUE_INDEX_STUB_ALL_ERR \
          '{"total_count":0,"incomplete_results":false,"items":[]}'
        ;;
    esac
    ;;
  *)
    exit 1
    ;;
esac
"#;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn ctx(&self) -> String {
        let v: Value = serde_json::from_str(&self.stdout).expect("stdout is JSON");
        v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("additionalContext")
            .to_string()
    }
}

fn which(cmd: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("{cmd} not on PATH"))
}

struct Fx {
    dir: tempfile::TempDir,
    stub_path: String,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, GH_STUB).unwrap();
        chmod_x(&gh);
        let stub_path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Fx { dir, stub_path }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// github remote 付き(または任意の remote 付き)の git repo を作る。
    fn repo(&self, rel: &str, remote: Option<&str>) -> PathBuf {
        let r = self.p(rel);
        std::fs::create_dir_all(&r).unwrap();
        git(&r, &["init", "-q"]);
        if let Some(url) = remote {
            git(&r, &["remote", "add", "origin", url]);
        }
        r
    }

    fn json(&self, name: &str, v: &Value) -> PathBuf {
        let f = self.p(name);
        std::fs::write(&f, format!("{v}\n")).unwrap();
        f
    }

    /// PATH を `path` に固定し、`env` を足して hook を 1 回実行する。
    fn run(&self, path: &str, project: &Path, env: &[(&str, &Path)], stdin: &str) -> Out {
        let mut cmd = target();
        for (k, _) in std::env::vars() {
            if k.starts_with("ISSUE_INDEX_STUB_") || k == "CLAUDE_PROJECT_DIR" {
                cmd.env_remove(k);
            }
        }
        cmd.env("PATH", path).env("CLAUDE_PROJECT_DIR", project);
        cmd.env("GIT_CONFIG_GLOBAL", gitconfig())
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, v) in env {
            cmd.env(k, v);
        }
        run_with_stdin(cmd, stdin)
    }

    fn run_stub(&self, project: &Path, env: &[(&str, &Path)]) -> Out {
        self.run(&self.stub_path, project, env, "{}")
    }
}

/// git の既定ブランチ名などユーザー設定に左右されないよう、固定の global config を使う
/// (unborn branch の名前が PR 行に出るため)。
fn gitconfig() -> PathBuf {
    static ONCE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let p =
            std::env::temp_dir().join(format!("issue-index-test-{}.gitconfig", std::process::id()));
        std::fs::write(&p, "[init]\n\tdefaultBranch = main\n").unwrap();
        p
    })
    .clone()
}

fn chmod_x(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", gitconfig())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// テスト対象。`ISSUE_INDEX_ORACLE` があれば bash 版を絶対パスの bash で起動する
/// (PATH を絞るケースでも `#!/usr/bin/env bash` の解決に失敗しないように —
/// bash selftest の `"$BASH" "$self"` と同じ)。
fn target() -> Command {
    match std::env::var_os("ISSUE_INDEX_ORACLE") {
        Some(script) => {
            let mut c = Command::new(which("bash"));
            c.arg(script);
            c
        }
        // 段 1-2: Rust 版はまだ無いので、既定も bash 版に向ける。
        None => {
            let mut c = Command::new(which("bash"));
            c.arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../config/claude/hooks/issue-index.sh"
            ));
            c
        }
    }
}

fn run_with_stdin(mut cmd: Command, stdin: &str) -> Out {
    use std::io::Write;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8(o.stdout).unwrap(),
        stderr: String::from_utf8(o.stderr).unwrap(),
    }
}

/// stdout / stderr を `tests/expected/<name>.{stdout,stderr}` と完全一致させる。
fn snap(name: &str, out: &Out) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected");
    for (ext, got) in [("stdout", &out.stdout), ("stderr", &out.stderr)] {
        let f = dir.join(format!("{name}.{ext}"));
        if std::env::var_os("ISSUE_INDEX_BLESS").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&f, got).unwrap();
        } else {
            let want = std::fs::read_to_string(&f)
                .unwrap_or_else(|_| panic!("snapshot {} が無い", f.display()));
            assert_eq!(got, &want, "{name}.{ext}");
        }
    }
}

fn count(hay: &str, needle: &str) -> usize {
    hay.lines().filter(|l| l.contains(needle)).count()
}

fn zero() -> Value {
    json!({"total_count":0,"incomplete_results":false,"items":[]})
}

/// mine 側で使う「更新の新しい順」の固定サンプル(bash の mkfifteen)。
fn fifteen(total: u64, incomplete: bool) -> Value {
    let items: Vec<Value> = (0..15)
        .map(|i| json!({"number": 100 + i, "title": format!("Issue {i}"), "labels": [], "user": {"login": "tester"}}))
        .collect();
    json!({"total_count": total, "incomplete_results": incomplete, "items": items})
}

fn all12() -> Value {
    let items: Vec<Value> = (0..12)
        .map(|i| json!({"number": 1 + i, "title": format!("issue {i}"), "labels": [], "user": {"login": "tester"}}))
        .collect();
    json!({"total_count": 12, "incomplete_results": false, "items": items})
}

const MINE: &str = "ISSUE_INDEX_STUB_MINE_FILE";
const ALL: &str = "ISSUE_INDEX_STUB_ALL_FILE";
const HANDOFF: &str = "ISSUE_INDEX_STUB_HANDOFF_FILE";

#[test]
fn out_of_scope_is_silent() {
    let fx = Fx::new();

    let norepo = fx.p("norepo");
    std::fs::create_dir_all(&norepo).unwrap();
    let o = fx.run_stub(&norepo, &[]);
    assert_eq!(o.code, 0, "git repo でない: exit 0");
    assert_eq!(o.stdout, "", "git repo でない: stdout 空");
    assert_eq!(o.stderr, "", "git repo でない: stderr 空");

    let nogh_remote = fx.repo(
        "nogh-remote",
        Some("https://gitlab.com/example/example.git"),
    );
    let o = fx.run_stub(&nogh_remote, &[]);
    assert_eq!(o.code, 0, "GitHub remote が無い: exit 0");
    assert_eq!(o.stdout, "", "GitHub remote が無い: stdout 空");
    assert_eq!(o.stderr, "", "GitHub remote が無い: stderr 空");

    let zero = fx.json("zero.json", &zero());
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let o = fx.run_stub(&repo, &[(MINE, &zero), (ALL, &zero)]);
    assert_eq!(o.code, 0, "両方 total_count=0(Issues 無効の実挙動): exit 0");
    assert_eq!(o.stdout, "", "両方 total_count=0: stdout 空");
    assert_eq!(o.stderr, "", "両方 total_count=0: stderr 空");
}

/// #458: リポジトリ名に "." を含む GitHub リポジトリが対象外と誤判定されない。
#[test]
fn dotted_repo_name() {
    let fx = Fx::new();
    let dot = fx.json(
        "dotrepo.json",
        &json!({"total_count":1,"incomplete_results":false,
            "items":[{"number":1,"title":"dotted repo test","labels":[],"user":{"login":"tester"}}]}),
    );
    for (rel, url) in [
        ("dotrepo", "https://github.com/o/o.github.io.git"),
        ("dotrepo-noext", "https://github.com/o/o.github.io"),
    ] {
        let r = fx.repo(rel, Some(url));
        let o = fx.run_stub(&r, &[(ALL, &dot)]);
        assert_eq!(o.code, 0, "{url}: exit 0");
        assert_eq!(
            count(&o.ctx(), "#1 dotted repo test"),
            1,
            "{url}: 索引が注入される"
        );
        snap(&format!("dotrepo-{rel}"), &o);
    }
}

/// jq / gh が PATH に無い。Rust 版は jq を使わないので jq 不在は前提ではなくなるが、
/// 同じ PATH で stdout が空のままであることは保つ(既定のスタブ応答は 0 件)。
#[test]
fn missing_jq_or_gh() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let stub_gh = fx.p("bin/gh");
    for (name, tools) in [
        (
            "nojq",
            &[
                "gh", "git", "awk", "sed", "grep", "basename", "dirname", "cat",
            ][..],
        ),
        (
            "nogh",
            &[
                "jq", "git", "awk", "sed", "grep", "basename", "dirname", "cat",
            ][..],
        ),
    ] {
        let d = fx.p(name);
        std::fs::create_dir_all(&d).unwrap();
        for t in tools {
            let src = if *t == "gh" {
                stub_gh.clone()
            } else {
                which(t)
            };
            std::os::unix::fs::symlink(src, d.join(t)).unwrap();
        }
        let o = fx.run(d.to_str().unwrap(), &repo, &[], "{}");
        assert_eq!(o.code, 0, "{name}: exit 0");
        assert_eq!(o.stdout, "", "{name}: stdout 空");
    }
}

#[test]
fn search_failures_report_one_line() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let zero = fx.json("zero.json", &zero());
    let mine15 = fx.json("mine15of68.json", &fifteen(68, false));
    let one = Path::new("1");

    let o = fx.run_stub(
        &repo,
        &[
            ("ISSUE_INDEX_STUB_MINE_RC", one),
            ("ISSUE_INDEX_STUB_MINE_ERR", Path::new("rate limit")),
        ],
    );
    assert_eq!(o.code, 0, "@me 側 Search 失敗: exit 0");
    assert_eq!(o.stdout, "", "@me 側 Search 失敗: stdout 空");
    assert!(!o.stderr.is_empty(), "@me 側 Search 失敗: stderr 非空");
    snap("fail-mine", &o);

    let o = fx.run_stub(
        &repo,
        &[
            (MINE, &zero),
            ("ISSUE_INDEX_STUB_ALL_RC", one),
            ("ISSUE_INDEX_STUB_ALL_ERR", Path::new("network error")),
        ],
    );
    assert_eq!(o.code, 0, "@me 0件 + 全体側だけ失敗: exit 0");
    assert_eq!(o.stdout, "", "@me 0件 + 全体側だけ失敗: stdout 空");
    assert!(
        !o.stderr.is_empty(),
        "@me 0件 + 全体側だけ失敗: stderr 非空"
    );
    snap("fail-all", &o);

    // 失敗時に stderr が空なら「不明なエラー」に倒れる(selftest 外の経路)。
    let o = fx.run_stub(&repo, &[("ISSUE_INDEX_STUB_MINE_RC", one)]);
    assert_eq!(o.stdout, "");
    snap("fail-mine-noerr", &o);

    let o = fx.run_stub(&repo, &[(MINE, &mine15), ("ISSUE_INDEX_STUB_ALL_RC", one)]);
    assert_eq!(o.code, 0, "@me が非0件なら全体側の失敗は無視: exit 0");
    assert!(
        !o.ctx().is_empty(),
        "@me が非0件なら全体側の失敗は無視: 注入は成功する"
    );
}

#[test]
fn injection_body() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let zero = fx.json("zero.json", &zero());
    let mine15 = fx.json("mine15of68.json", &fifteen(68, false));
    let all12 = fx.json("all12of12.json", &all12());

    let o = fx.run_stub(&repo, &[(MINE, &mine15)]);
    let ctx = o.ctx();
    assert_eq!(o.code, 0, "68件のうち15件: exit 0");
    let v: Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    assert_eq!(
        count(&ctx, "68 件のうち、更新の新しい 15 件を示す(53 件を省略)"),
        1
    );
    let issue_lines = ctx
        .lines()
        .filter(|l| {
            l.strip_prefix('#')
                .and_then(|r| r.split_once(" Issue "))
                .is_some_and(|(a, b)| {
                    !a.is_empty()
                        && a.chars().all(|c| c.is_ascii_digit())
                        && !b.is_empty()
                        && b.chars().all(|c| c.is_ascii_digit())
                })
        })
        .count();
    assert_eq!(issue_lines, 15, "68件のうち15件: 15行の Issue が出る");
    snap("mine-15-of-68", &o);

    let o = fx.run_stub(&repo, &[(MINE, &zero), (ALL, &all12)]);
    let ctx = o.ctx();
    assert_eq!(
        count(
            &ctx,
            "repo 全体の open Issue 12 件を更新の新しい順に全件示す"
        ),
        1,
        "@me 0件フォールバック: 全体件数の文が出る"
    );
    assert_eq!(
        ctx.lines()
            .filter(|l| l.starts_with('#') && l.contains(" issue "))
            .count(),
        12
    );
    snap("fallback-all-12", &o);
}

#[test]
fn title_sanitize_and_truncate() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let long: String = "あ".repeat(200);
    let longf = fx.json(
        "longtitle.json",
        &json!({"total_count":1,"incomplete_results":false,
            "items":[{"number":1,"title":long,"labels":[],"user":{"login":"tester"}}]}),
    );
    let ctlf = fx.json(
        "ctltitle.json",
        &json!({"total_count":1,"incomplete_results":false,
            "items":[{"number":2,"title":"legit\u{7}\ntitle","labels":[],"user":{"login":"tester"}}]}),
    );

    let o = fx.run_stub(&repo, &[(MINE, &longf)]);
    let ctx = o.ctx();
    let line = ctx
        .lines()
        .find_map(|l| l.strip_prefix("#1 "))
        .expect("#1 line");
    assert_eq!(line.chars().count(), 120, "200字タイトルは120字で切れる");
    snap("long-title", &o);

    let o = fx.run_stub(&repo, &[(MINE, &ctlf)]);
    assert_eq!(o.code, 0, "制御文字入りタイトルは exit 0 のまま注入される");
    assert_eq!(count(&o.ctx(), "#2 legittitle"), 1, "制御文字が除去される");
    snap("control-title", &o);
}

#[test]
fn incomplete_results() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let f = fx.json(
        "incomplete.json",
        &json!({"total_count":1,"incomplete_results":true,
            "items":[{"number":1,"title":"部分結果","labels":[],"user":{"login":"tester"}}]}),
    );
    let o = fx.run_stub(&repo, &[(MINE, &f)]);
    let ctx = o.ctx();
    assert_eq!(o.code, 0);
    assert_eq!(count(&ctx, "#1 部分結果"), 1);
    assert_eq!(count(&ctx, "総数・省略件数は不正確です"), 1);
    assert!(!o.stderr.is_empty(), "incomplete_results=true: stderr 非空");
    snap("incomplete", &o);
}

#[test]
fn author_annotation() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let f = fx.json(
        "mixed.json",
        &json!({"total_count":2,"incomplete_results":false,"items":[
            {"number":1,"title":"自分の Issue","labels":[],"user":{"login":"tester"}},
            {"number":2,"title":"他人の Issue","labels":[{"name":"bug"}],"user":{"login":"other-user"}}
        ]}),
    );
    let o = fx.run_stub(&repo, &[(MINE, &f)]);
    let ctx = o.ctx();
    assert_eq!(count(&ctx, "#1 自分の Issue"), 1);
    assert_eq!(count(&ctx, "#1 自分の Issue (起票"), 0);
    assert_eq!(
        count(&ctx, "#2 他人の Issue [bug] (起票: other-user)"),
        1,
        "他人起票の行にだけ (起票: login) を付ける"
    );
    snap("author-mixed", &o);

    let o = fx.run_stub(
        &repo,
        &[(MINE, &f), ("ISSUE_INDEX_STUB_WHO_RC", Path::new("1"))],
    );
    assert_eq!(o.code, 0, "viewer login 取得失敗でも注入は成功する");
    assert_eq!(count(&o.ctx(), "(起票:"), 0, "起票者注記が一切出ない");
    snap("author-who-failed", &o);
}

#[test]
fn handoff_ai_section() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    let zero = fx.json("zero.json", &zero());
    let all12 = fx.json("all12of12.json", &all12());
    let mine15 = fx.json("mine15of68.json", &fifteen(68, false));
    let mixed = fx.json(
        "handoff-mixed.json",
        &json!({"total_count":2,"incomplete_results":false,"items":[
            {"number":910,"title":"着手可能なタスク","issue_dependencies_summary":{"blocked_by":0}},
            {"number":911,"title":"blocker 未 close のタスク","issue_dependencies_summary":{"blocked_by":1}}
        ]}),
    );
    let nosummary = fx.json(
        "handoff-nosummary.json",
        &json!({"total_count":1,"incomplete_results":false,
            "items":[{"number":912,"title":"summary フィールド無し"}]}),
    );

    let o = fx.run_stub(&repo, &[(MINE, &zero), (ALL, &all12), (HANDOFF, &mixed)]);
    let ctx = o.ctx();
    assert_eq!(count(&ctx, "#910 着手可能なタスク"), 1);
    assert_eq!(count(&ctx, "#911"), 0);
    assert_eq!(count(&ctx, "着手可能な handoff:ai"), 1);
    snap("handoff-mixed", &o);

    let o = fx.run_stub(&repo, &[(MINE, &zero), (ALL, &all12)]);
    assert_eq!(
        count(&o.ctx(), "着手可能な handoff:ai"),
        0,
        "0件: 節ごと沈黙"
    );

    let o = fx.run_stub(
        &repo,
        &[(MINE, &zero), (ALL, &all12), (HANDOFF, &nosummary)],
    );
    assert_eq!(count(&o.ctx(), "#912"), 0, "summary 欠落: fail-closed");

    let o = fx.run_stub(
        &repo,
        &[
            (MINE, &zero),
            (ALL, &all12),
            ("ISSUE_INDEX_STUB_HANDOFF_RC", Path::new("1")),
        ],
    );
    assert_eq!(o.code, 0, "handoff:ai 検索失敗でも注入は成功する");
    assert_eq!(count(&o.ctx(), "着手可能な handoff:ai"), 0);

    let o = fx.run_stub(&repo, &[(MINE, &mine15), (HANDOFF, &mixed)]);
    let ctx = o.ctx();
    assert_eq!(count(&ctx, "#910 着手可能なタスク"), 1, "@me 枠と併存する");
    assert_eq!(
        count(&ctx, "68 件のうち、更新の新しい 15 件を示す(53 件を省略)"),
        1,
        "@me 枠自体も出続ける"
    );
    snap("handoff-with-mine", &o);
}

#[test]
fn pr_line() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("https://github.com/example/example.git"));
    git(&repo, &["checkout", "-qb", "feature/x"]);
    let zero = fx.json("zero.json", &zero());
    let all12 = fx.json("all12of12.json", &all12());
    let pr = fx.json(
        "pr.json",
        &json!([{"number":3858,"title":"PR タイトル","isDraft":false,
            "closingIssuesReferences":[{"number":30},{"number":29}]}]),
    );
    let draft = fx.json(
        "pr-draft.json",
        &json!([{"number":7,"title":"下書き","isDraft":true,"closingIssuesReferences":[]}]),
    );

    let o = fx.run_stub(
        &repo,
        &[
            (MINE, &zero),
            (ALL, &all12),
            ("ISSUE_INDEX_STUB_PR_FILE", &pr),
        ],
    );
    assert_eq!(
        count(
            &o.ctx(),
            "現ブランチ feature/x に対応する open PR: #3858 PR タイトル (closes: #30,#29)"
        ),
        1
    );
    snap("pr-present", &o);

    // selftest 外: draft かつ closes 無し、PR 0 件(「なし」)。
    let o = fx.run_stub(
        &repo,
        &[
            (MINE, &zero),
            (ALL, &all12),
            ("ISSUE_INDEX_STUB_PR_FILE", &draft),
        ],
    );
    snap("pr-draft", &o);
    let o = fx.run_stub(&repo, &[(MINE, &zero), (ALL, &all12)]);
    assert_eq!(
        count(&o.ctx(), "現ブランチ feature/x に対応する open PR: なし"),
        1
    );
    snap("pr-none", &o);

    let o = fx.run_stub(
        &repo,
        &[
            (MINE, &zero),
            (ALL, &all12),
            ("ISSUE_INDEX_STUB_PR_RC", Path::new("1")),
        ],
    );
    assert_eq!(o.code, 0, "PR 取得失敗でも注入は成功する");
    assert_eq!(
        count(&o.ctx(), "対応する open PR"),
        0,
        "PR 行そのものが出ない"
    );
}

/// `.cwd` からの project 解決(CLAUDE_PROJECT_DIR が空のとき)。Codex も同じ形。
#[test]
fn project_from_cwd() {
    let fx = Fx::new();
    let repo = fx.repo("repo", Some("git@github.com:example/example.git"));
    let all12 = fx.json("all12of12.json", &all12());
    let mut cmd = target();
    cmd.env("PATH", &fx.stub_path)
        .env("GIT_CONFIG_GLOBAL", gitconfig())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CLAUDE_PROJECT_DIR", "")
        .env(ALL, &all12);
    let stdin =
        json!({"session_id":"s","cwd": repo, "hook_event_name":"SessionStart","source":"startup"})
            .to_string();
    let o = run_with_stdin(cmd, &stdin);
    assert_eq!(o.code, 0);
    snap("cwd-scp-remote", &o);
}
