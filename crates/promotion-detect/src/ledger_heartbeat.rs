//! 本人の Issue 台帳の、端末側の生存の記録と、クラウドが止まったことを知る逆向きの確認(ADR-1382)。
//!
//! 台帳の日次の通知はクラウドが届け、クラウドは vault(本人の端末にあるノート)を読めない。
//! そこで日次 timer(`promotion-detect`)が、次の 2 つを台帳のリポジトリの常設 Issue に対して行う。
//!
//! - **生存の記録**: タイトル `ledger-heartbeat`・ラベル `ledger-status` の open な Issue の本文を、
//!   [`render_body`] の書式で全面的に書き換える(`asof` と、wrap-up inbox の open 件数のプロジェクト別)。
//! - **逆向きの確認**: タイトル `ledger-digest`・ラベル `ledger-status` の Issue の最新のコメントが
//!   [`DIGEST_STALE_SECS`] より古い(Issue が無い・コメントが無いを含む)なら、Herdr で本人に知らせる。
//!   通知は 1 日 1 回([`Notified::AlreadyToday`])。
//!
//! 台帳のリポジトリは非公開で、名前はこのリポジトリに書かない。端末ローカルの設定
//! (`$XDG_CONFIG_HOME/ledger-heartbeat/target`、無ければ `~/.config/ledger-heartbeat/target`、
//! 1 行 `OWNER/REPO`)から読む。ファイルが無ければ機能は無効で、何もしない。
//!
//! 時刻と外部コマンド(`gh`・`herdr`)は引数で渡す。ここは判定と組み立てだけを持ち、プロセス起動は
//! [`CommandGh`]・[`CommandHerdr`] に閉じる(試験は偽の実装を渡す)。失敗は標準エラーに 1 行出して続け、
//! 日次の他の処理を止めない。

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// 設定ファイルの、設定ディレクトリからの相対パス。
pub const CONFIG_REL: &str = "ledger-heartbeat/target";
/// 生存の記録を書く Issue のタイトル。
pub const HEARTBEAT_TITLE: &str = "ledger-heartbeat";
/// クラウドの日次の通知が届く Issue のタイトル。
pub const DIGEST_TITLE: &str = "ledger-digest";
/// 両方の Issue が持つラベル。
pub const STATUS_LABEL: &str = "ledger-status";
/// 最新のコメントがこれより古いと、日次の通知が届いていないとみなす(36 時間)。
pub const DIGEST_STALE_SECS: i64 = 36 * 3600;

// ---------------------------------------------------------------------------
// 純粋な部分
// ---------------------------------------------------------------------------

/// 設定ファイルの中身から `OWNER/REPO` を取り出す(最初の空でない行。前後の空白は無視)。
/// 英数字・`_`・`.`・`-` だけの `OWNER/REPO` の形でなければ `None`。
pub fn parse_target(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let word_ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    let (owner, repo) = line.split_once('/')?;
    (word_ok(owner) && word_ok(repo)).then(|| line.to_string())
}

/// unix 秒 → UTC の ISO8601 `YYYY-MM-DDTHH:MM:SSZ`。
pub fn format_asof(unix: i64) -> String {
    let (y, m, d) = issue_inbox::vault::civil_from_days(unix.div_euclid(86_400));
    let s = unix.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        s % 3600 / 60,
        s % 60
    )
}

/// UTC の ISO8601 `YYYY-MM-DDTHH:MM:SSZ`(GitHub の `createdAt` の形)→ unix 秒。読めなければ `None`。
pub fn parse_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let shaped = b.len() == 20
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && [11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&i| b[i].is_ascii_digit());
    if !shaped {
        return None;
    }
    let day = issue_inbox::vault::parse_day(&s[..10])?;
    let (h, mi, sec) = (
        s[11..13].parse::<i64>().ok()?,
        s[14..16].parse::<i64>().ok()?,
        s[17..19].parse::<i64>().ok()?,
    );
    (h < 24 && mi < 60 && sec < 60).then_some(day * 86_400 + h * 3600 + mi * 60 + sec)
}

/// プロジェクト名を本文の 1 行に載せられる形にする。空白・制御文字・`:` は `_` にし、空なら `unknown`。
fn project_label(repo: &str) -> String {
    let t: String = repo
        .trim()
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() || c == ':' {
                '_'
            } else {
                c
            }
        })
        .collect();
    if t.is_empty() {
        "unknown".to_string()
    } else {
        t
    }
}

/// wrap-up inbox の open 件数(`repo` ごと)を、本文の書式に直す。同じ名前になる `repo` は合算し、
/// 件数 0 のプロジェクトは書かない。名前の昇順。
pub fn project_counts(open_repos: impl IntoIterator<Item = String>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for repo in open_repos {
        *m.entry(project_label(&repo)).or_insert(0) += 1;
    }
    m
}

/// 生存の記録の本文。先頭行が `asof: <UTC の ISO8601>`、続けて `inbox <プロジェクト名>: <件数>` を 1 プロジェクト 1 行。
/// 本文はこの行だけで、末尾の改行は付けない。
pub fn render_body(now_unix: i64, counts: &BTreeMap<String, usize>) -> String {
    let mut lines = vec![format!("asof: {}", format_asof(now_unix))];
    for (name, n) in counts {
        if *n > 0 {
            lines.push(format!("inbox {name}: {n}"));
        }
    }
    lines.join("\n")
}

/// 最新のコメントが 36 時間より古い(コメントが無い=`None` を含む)なら `true`。ちょうど 36 時間は古くない。
/// 未来の時刻(時計のずれ)は古くない。
pub fn is_digest_stale(latest_comment_unix: Option<i64>, now_unix: i64) -> bool {
    latest_comment_unix.is_none_or(|t| now_unix - t > DIGEST_STALE_SECS)
}

/// 本文が同じか(改行コードと末尾の空白は無視)。同じなら更新しない。
fn same_body(existing: &str, new: &str) -> bool {
    existing.replace("\r\n", "\n").trim_end() == new.trim_end()
}

/// 通知の題。
pub const NOTICE_TITLE: &str = "台帳の日次通知が届いていない";

/// 通知の本文。短く、何が起きたか(日次の通知が 36 時間届いていない)と、確かめる場所(台帳の常設 Issue)を書く。
/// 台帳のリポジトリ名は実行時に設定から入る(このリポジトリには書かない)。
pub fn notice_body(repo: &str, digest_issue: Option<u64>) -> String {
    match digest_issue {
        Some(n) => format!("日次の通知が 36 時間届いていない。台帳の常設 Issue({repo}#{n})を確かめる"),
        None => format!(
            "日次の通知が 36 時間届いていない。台帳の常設 Issue({DIGEST_TITLE})が見つからない。{repo} を確かめる"
        ),
    }
}

// ---------------------------------------------------------------------------
// 外部コマンド(試験は偽の実装を渡す)
// ---------------------------------------------------------------------------

/// `gh` の呼び出し。成功なら標準出力、失敗なら理由の 1 行。
pub trait Gh {
    fn run(&self, args: &[String]) -> Result<String, String>;
}

/// `herdr notification show` の呼び出し。起動できたら標準出力(JSON)、起動できなければ理由。
pub trait Herdr {
    fn show(&self, title: &str, body: &str) -> Result<String, String>;
}

pub struct CommandGh {
    pub bin: String,
}

impl Gh for CommandGh {
    fn run(&self, args: &[String]) -> Result<String, String> {
        let out = Command::new(&self.bin)
            .args(args)
            .output()
            .map_err(|e| format!("{} を起動できない: {e}", self.bin))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().next().unwrap_or("").trim();
            return Err(format!("gh が失敗した({}): {first}", out.status));
        }
        String::from_utf8(out.stdout).map_err(|e| format!("gh の出力が UTF-8 でない: {e}"))
    }
}

pub struct CommandHerdr {
    pub bin: String,
}

impl Herdr for CommandHerdr {
    /// `due-remind` と同じ呼び出し(`herdr notification show <題> --body <本文> --sound request`)。
    fn show(&self, title: &str, body: &str) -> Result<String, String> {
        let out = Command::new(&self.bin)
            .args([
                "notification",
                "show",
                title,
                "--body",
                body,
                "--sound",
                "request",
            ])
            .output()
            .map_err(|e| format!("{} を起動できない: {e}", self.bin))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

// ---------------------------------------------------------------------------
// 1 回の実行
// ---------------------------------------------------------------------------

/// 生存の記録の結末。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    /// 本文を書き換えた。
    Written,
    /// 本文が同じなので書かなかった。
    Unchanged,
    /// `ledger-heartbeat` の Issue が無い(端末側からは作らない)。
    NoIssue,
    /// open 件数を数えられない(vault を宣言していない・読めない機体)ので、書かなかった。
    NoCounts,
    /// `gh` が失敗した。
    Failed,
}

/// 逆向きの確認の結末。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notified {
    /// 最新のコメントが 36 時間以内で、通知は要らない。
    Fresh,
    /// 古いが、今日はもう通知した。
    AlreadyToday,
    /// 古く、通知した(Herdr が `shown: true` を返した)。
    Shown,
    /// 古いが、Herdr が出せなかった(前面に無い・無効・起動できない)。次の実行で再試行する。
    NotShown,
    /// 状態を読めなかった(`gh` の失敗など)ので、判断しなかった。
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub heartbeat: Wrote,
    pub digest: Notified,
}

/// 1 回の実行の入力。
pub struct Run<'a> {
    pub gh: &'a dyn Gh,
    pub herdr: &'a dyn Herdr,
    /// 台帳のリポジトリ `OWNER/REPO`(設定から)。
    pub repo: &'a str,
    pub now_unix: i64,
    /// wrap-up inbox の open 件数(プロジェクトごと)。数えられなければ `None`(生存の記録を書かない)。
    pub counts: Option<&'a BTreeMap<String, usize>>,
    /// 通知した日(JST の `YYYY-MM-DD`)を残す状態ファイル。
    pub notified_path: &'a Path,
}

#[derive(Debug, Deserialize)]
struct RawIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
}

fn s(x: &str) -> String {
    x.to_string()
}

fn list_status_issues(gh: &dyn Gh, repo: &str) -> Result<Vec<RawIssue>, String> {
    let out = gh.run(&[
        s("issue"),
        s("list"),
        s("-R"),
        s(repo),
        s("--label"),
        s(STATUS_LABEL),
        s("--state"),
        s("open"),
        s("--json"),
        s("number,title,body"),
        s("--limit"),
        s("100"),
    ])?;
    serde_json::from_str(&out).map_err(|e| format!("Issue の一覧を読めない: {e}"))
}

/// Issue の最新のコメントの作成時刻(unix 秒)。コメントが無ければ `Ok(None)`。
fn latest_comment(gh: &dyn Gh, repo: &str, number: u64) -> Result<Option<i64>, String> {
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| format!("OWNER/REPO の形でない: {repo}"))?;
    let query = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){issue(number:$number){comments(last:1){nodes{createdAt}}}}}";
    let out = gh.run(&[
        s("api"),
        s("graphql"),
        s("-f"),
        format!("query={query}"),
        s("-f"),
        format!("owner={owner}"),
        s("-f"),
        format!("name={name}"),
        s("-F"),
        format!("number={number}"),
    ])?;
    let v: serde_json::Value =
        serde_json::from_str(&out).map_err(|e| format!("コメントの応答を読めない: {e}"))?;
    let nodes = v
        .pointer("/data/repository/issue/comments/nodes")
        .and_then(|n| n.as_array())
        .ok_or_else(|| "コメントの応答に nodes が無い".to_string())?;
    let Some(last) = nodes.last() else {
        return Ok(None);
    };
    let created = last
        .get("createdAt")
        .and_then(|c| c.as_str())
        .ok_or_else(|| "コメントに createdAt が無い".to_string())?;
    parse_utc(created)
        .map(Some)
        .ok_or_else(|| format!("createdAt を読めない: {created}"))
}

/// 生存の記録と逆向きの確認を、1 回ずつ行う。どちらかが失敗しても、もう一方は行う。
pub fn run(r: &Run) -> Report {
    let issues = match list_status_issues(r.gh, r.repo) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("promotion-detect: 台帳の常設 Issue を読めない: {e}");
            return Report {
                heartbeat: Wrote::Failed,
                digest: Notified::Failed,
            };
        }
    };
    let find = |title: &str| issues.iter().find(|i| i.title == title);
    Report {
        heartbeat: write_heartbeat(r, find(HEARTBEAT_TITLE)),
        digest: check_digest(r, find(DIGEST_TITLE)),
    }
}

fn write_heartbeat(r: &Run, issue: Option<&RawIssue>) -> Wrote {
    let Some(issue) = issue else {
        eprintln!("promotion-detect: {HEARTBEAT_TITLE} の Issue が無い(端末側からは作らない)。生存の記録を書かない");
        return Wrote::NoIssue;
    };
    let Some(counts) = r.counts else {
        eprintln!("promotion-detect: wrap-up inbox の open 件数を数えられない(vault を使えない機体)。生存の記録を書かない");
        return Wrote::NoCounts;
    };
    let body = render_body(r.now_unix, counts);
    if same_body(issue.body.as_deref().unwrap_or(""), &body) {
        return Wrote::Unchanged;
    }
    let edited = r.gh.run(&[
        s("issue"),
        s("edit"),
        issue.number.to_string(),
        s("-R"),
        s(r.repo),
        s("--body"),
        body,
    ]);
    match edited {
        Ok(_) => Wrote::Written,
        Err(e) => {
            eprintln!("promotion-detect: 生存の記録を書けない: {e}");
            Wrote::Failed
        }
    }
}

/// JST の `YYYY-MM-DD`(通知の 1 日 1 回の単位)。
fn jst_day(now_unix: i64) -> String {
    issue_inbox::vault::jst_date(now_unix.max(0) as u64)
}

fn check_digest(r: &Run, issue: Option<&RawIssue>) -> Notified {
    let latest = match issue {
        None => None,
        Some(i) => match latest_comment(r.gh, r.repo, i.number) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("promotion-detect: {DIGEST_TITLE} の最新のコメントを読めない: {e}");
                return Notified::Failed;
            }
        },
    };
    if !is_digest_stale(latest, r.now_unix) {
        return Notified::Fresh;
    }
    let today = jst_day(r.now_unix);
    if std::fs::read_to_string(r.notified_path).is_ok_and(|prev| prev.trim() == today) {
        return Notified::AlreadyToday;
    }
    let body = notice_body(r.repo, issue.map(|i| i.number));
    let response = match r.herdr.show(NOTICE_TITLE, &body) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("promotion-detect: 台帳の通知を出せない(次の実行で再試行): {e}");
            return Notified::NotShown;
        }
    };
    let shown = serde_json::from_str::<serde_json::Value>(&response)
        .map(|v| due_remind::find_shown_reason(&v))
        .unwrap_or((None, None));
    match shown {
        (Some(true), _) => {
            if let Some(parent) = r.notified_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(r.notified_path, &today).is_err() {
                eprintln!("promotion-detect: 台帳の通知の日付を書けない({today} は通知済み)");
            }
            Notified::Shown
        }
        (_, Some(reason)) if reason == "disabled" => {
            eprintln!("promotion-detect: herdr の通知配信が無効化されている");
            Notified::NotShown
        }
        _ => Notified::NotShown,
    }
}

// ---------------------------------------------------------------------------
// 試験
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // 2026-11-08T06:00:00Z
    const NOW: i64 = 1_794_117_600;
    const H: i64 = 3600;

    #[test]
    fn the_fixed_instant_is_what_the_tests_say() {
        assert_eq!(format_asof(NOW), "2026-11-08T06:00:00Z");
        assert_eq!(parse_utc("2026-11-08T06:00:00Z"), Some(NOW));
    }

    #[test]
    fn target_is_the_first_non_empty_line_in_owner_repo_form() {
        assert_eq!(
            parse_target("example/ledger\n"),
            Some("example/ledger".into())
        );
        assert_eq!(
            parse_target("\n  example/ledger.v2  \nother/x\n"),
            Some("example/ledger.v2".into())
        );
        for bad in [
            "",
            "\n\n",
            "ledger",
            "a/b/c",
            "a/ b",
            "a b/c",
            "/c",
            "a/",
            "a/b;rm -rf",
        ] {
            assert_eq!(parse_target(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn utc_round_trips_and_refuses_what_is_not_the_github_shape() {
        for t in [0, 86_399, 86_400, NOW, 4_102_444_799] {
            assert_eq!(parse_utc(&format_asof(t)), Some(t), "{t}");
        }
        for bad in [
            "",
            "2026-11-08",
            "2026-11-08T06:00:00",
            "2026-11-08T06:00:00+09:00",
            "2026-11-08 06:00:00Z",
            "2026-11-08T24:00:00Z",
            "2026-11-08T06:60:00Z",
            "2026-02-30T06:00:00Z",
            "2026-11-08T06:00:00.5Z",
        ] {
            assert_eq!(parse_utc(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_body_starts_with_asof_then_one_line_per_project_and_omits_zero() {
        let counts = project_counts(["o/b", "o/a", "o/b", "o/b"].map(String::from));
        assert_eq!(
            render_body(NOW, &counts),
            "asof: 2026-11-08T06:00:00Z\ninbox o/a: 1\ninbox o/b: 3"
        );
        // 件数が 0 の項目は書かない。何も無ければ asof の 1 行だけ(末尾の改行なし)。
        let mut zero = BTreeMap::new();
        zero.insert("o/z".to_string(), 0);
        assert_eq!(render_body(NOW, &zero), "asof: 2026-11-08T06:00:00Z");
        assert_eq!(
            render_body(NOW, &BTreeMap::new()),
            "asof: 2026-11-08T06:00:00Z"
        );
    }

    #[test]
    fn a_project_name_cannot_break_the_line_format() {
        let counts = project_counts(["o/r\ninbox x: 9", "", "a:b"].map(String::from));
        let body = render_body(NOW, &counts);
        assert_eq!(body.lines().count(), 4, "{body}");
        assert!(body.contains("inbox unknown: 1"), "{body}");
        assert!(body.contains("inbox a_b: 1"), "{body}");
        for line in body.lines().skip(1) {
            assert!(line.starts_with("inbox "), "{line}");
            assert_eq!(line.matches(": ").count(), 1, "{line}");
        }
    }

    #[test]
    fn stale_means_older_than_36_hours_and_a_missing_comment_is_stale() {
        assert!(!is_digest_stale(Some(NOW - 35 * H), NOW));
        assert!(!is_digest_stale(Some(NOW - 36 * H), NOW));
        assert!(is_digest_stale(Some(NOW - 36 * H - 1), NOW));
        assert!(is_digest_stale(Some(NOW - 37 * H), NOW));
        assert!(is_digest_stale(None, NOW));
        // 時計のずれで未来の時刻になっても、古くはない。
        assert!(!is_digest_stale(Some(NOW + H), NOW));
    }

    #[test]
    fn the_notice_names_the_symptom_and_where_to_look() {
        let with_issue = notice_body("example/ledger", Some(12));
        assert!(with_issue.contains("36 時間届いていない"), "{with_issue}");
        assert!(with_issue.contains("example/ledger#12"), "{with_issue}");
        let without = notice_body("example/ledger", None);
        assert!(without.contains("36 時間届いていない"), "{without}");
        assert!(without.contains("example/ledger"), "{without}");
        assert!(with_issue.chars().count() < 80 && without.chars().count() < 100);
    }

    // ---- 偽の gh・herdr ----

    /// 呼ばれた引数を記録し、`list`・`comments`・`edit` の応答を決めた値で返す。
    struct FakeGh {
        list: Result<String, String>,
        comments: Result<String, String>,
        edit: Result<String, String>,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FakeGh {
        fn new(list: &str, comments: &str) -> FakeGh {
            FakeGh {
                list: Ok(list.to_string()),
                comments: Ok(comments.to_string()),
                edit: Ok(String::new()),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn edits(&self) -> Vec<Vec<String>> {
            self.calls
                .borrow()
                .iter()
                .filter(|c| c.get(1).map(String::as_str) == Some("edit"))
                .cloned()
                .collect()
        }
    }

    impl Gh for FakeGh {
        fn run(&self, args: &[String]) -> Result<String, String> {
            self.calls.borrow_mut().push(args.to_vec());
            match (args[0].as_str(), args[1].as_str()) {
                ("issue", "list") => self.list.clone(),
                ("api", "graphql") => self.comments.clone(),
                ("issue", "edit") => self.edit.clone(),
                other => panic!("想定外の gh: {other:?}"),
            }
        }
    }

    struct FakeHerdr {
        response: Result<String, String>,
        shown: RefCell<Vec<(String, String)>>,
    }

    impl FakeHerdr {
        fn new(response: &str) -> FakeHerdr {
            FakeHerdr {
                response: Ok(response.to_string()),
                shown: RefCell::new(Vec::new()),
            }
        }
    }

    impl Herdr for FakeHerdr {
        fn show(&self, title: &str, body: &str) -> Result<String, String> {
            self.shown
                .borrow_mut()
                .push((title.to_string(), body.to_string()));
            self.response.clone()
        }
    }

    const SHOWN: &str = r#"{"result":{"shown":true,"reason":null}}"#;

    fn list_json(heartbeat_body: &str) -> String {
        serde_json::json!([
            {"number": 11, "title": "ledger-heartbeat", "body": heartbeat_body},
            {"number": 12, "title": "ledger-digest", "body": ""},
            {"number": 13, "title": "something else", "body": "x"},
        ])
        .to_string()
    }

    fn comment_json(created: Option<i64>) -> String {
        let nodes: Vec<_> = created
            .map(|t| serde_json::json!({"createdAt": format_asof(t)}))
            .into_iter()
            .collect();
        serde_json::json!({"data":{"repository":{"issue":{"comments":{"nodes":nodes}}}}})
            .to_string()
    }

    fn counts() -> BTreeMap<String, usize> {
        project_counts(["example/a", "example/a", "example/b"].map(String::from))
    }

    fn run_with(
        gh: &FakeGh,
        herdr: &FakeHerdr,
        counts: Option<&BTreeMap<String, usize>>,
        notified: &Path,
    ) -> Report {
        run(&Run {
            gh,
            herdr,
            repo: "example/ledger",
            now_unix: NOW,
            counts,
            notified_path: notified,
        })
    }

    #[test]
    fn the_heartbeat_body_is_written_to_the_heartbeat_issue_only() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = FakeGh::new(&list_json("asof: old"), &comment_json(Some(NOW - H)));
        let herdr = FakeHerdr::new(SHOWN);
        let report = run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n"));
        assert_eq!(report.heartbeat, Wrote::Written);
        let edits = gh.edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0],
            [
                "issue",
                "edit",
                "11",
                "-R",
                "example/ledger",
                "--body",
                "asof: 2026-11-08T06:00:00Z\ninbox example/a: 2\ninbox example/b: 1"
            ]
        );
        // 一覧は、ラベルと open で絞る。
        let list = gh.calls.borrow()[0].join(" ");
        assert!(list.contains("--label ledger-status"), "{list}");
        assert!(list.contains("--state open"), "{list}");
    }

    #[test]
    fn the_same_body_is_not_written_again() {
        let tmp = tempfile::tempdir().unwrap();
        let body = render_body(NOW, &counts());
        // 取得した本文の改行コードが CRLF でも、末尾の空白があっても、同じとみなす。
        let existing = format!("{}\r\n", body.replace('\n', "\r\n"));
        let gh = FakeGh::new(&list_json(&existing), &comment_json(Some(NOW - H)));
        let report = run_with(
            &gh,
            &FakeHerdr::new(SHOWN),
            Some(&counts()),
            &tmp.path().join("n"),
        );
        assert_eq!(report.heartbeat, Wrote::Unchanged);
        assert!(gh.edits().is_empty());
    }

    #[test]
    fn a_new_asof_alone_is_a_change_so_liveness_is_always_refreshed() {
        let tmp = tempfile::tempdir().unwrap();
        let yesterday = render_body(NOW - 24 * H, &counts());
        let gh = FakeGh::new(&list_json(&yesterday), &comment_json(Some(NOW - H)));
        let report = run_with(
            &gh,
            &FakeHerdr::new(SHOWN),
            Some(&counts()),
            &tmp.path().join("n"),
        );
        assert_eq!(report.heartbeat, Wrote::Written);
    }

    #[test]
    fn without_a_heartbeat_issue_nothing_is_written_and_none_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let only_digest = r#"[{"number":12,"title":"ledger-digest","body":""}]"#;
        let gh = FakeGh::new(only_digest, &comment_json(Some(NOW - H)));
        let report = run_with(
            &gh,
            &FakeHerdr::new(SHOWN),
            Some(&counts()),
            &tmp.path().join("n"),
        );
        assert_eq!(report.heartbeat, Wrote::NoIssue);
        assert!(gh
            .calls
            .borrow()
            .iter()
            .all(|c| c[1] != "edit" && c[1] != "create"));
        // 逆向きの確認は、生存の記録の Issue の有無に依らず続く。
        assert_eq!(report.digest, Notified::Fresh);
    }

    #[test]
    fn without_counts_the_heartbeat_is_not_written_rather_than_claiming_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = FakeGh::new(&list_json("asof: old"), &comment_json(Some(NOW - H)));
        let report = run_with(&gh, &FakeHerdr::new(SHOWN), None, &tmp.path().join("n"));
        assert_eq!(report.heartbeat, Wrote::NoCounts);
        assert!(gh.edits().is_empty());
    }

    #[test]
    fn a_comment_35_hours_old_is_fresh_and_37_hours_old_notifies() {
        for (age, want_shown) in [(35, false), (37, true)] {
            let tmp = tempfile::tempdir().unwrap();
            let gh = FakeGh::new(&list_json("x"), &comment_json(Some(NOW - age * H)));
            let herdr = FakeHerdr::new(SHOWN);
            let report = run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n"));
            assert_eq!(
                report.digest,
                if want_shown {
                    Notified::Shown
                } else {
                    Notified::Fresh
                },
                "{age}h"
            );
            assert_eq!(
                herdr.shown.borrow().len(),
                usize::from(want_shown),
                "{age}h"
            );
        }
    }

    #[test]
    fn a_digest_issue_without_comments_notifies() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = FakeGh::new(&list_json("x"), &comment_json(None));
        let herdr = FakeHerdr::new(SHOWN);
        let report = run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n"));
        assert_eq!(report.digest, Notified::Shown);
        let shown = herdr.shown.borrow();
        assert_eq!(shown[0].0, NOTICE_TITLE);
        assert!(shown[0].1.contains("example/ledger#12"), "{}", shown[0].1);
    }

    #[test]
    fn a_missing_digest_issue_notifies() {
        let tmp = tempfile::tempdir().unwrap();
        let only_heartbeat = r#"[{"number":11,"title":"ledger-heartbeat","body":""}]"#;
        let gh = FakeGh::new(only_heartbeat, "unused");
        let herdr = FakeHerdr::new(SHOWN);
        let report = run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n"));
        assert_eq!(report.digest, Notified::Shown);
        assert!(herdr.shown.borrow()[0].1.contains("見つからない"));
    }

    #[test]
    fn the_notice_is_shown_at_most_once_a_day_and_again_the_next_day() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state/notified");
        let gh = FakeGh::new(&list_json("x"), &comment_json(Some(NOW - 40 * H)));
        let herdr = FakeHerdr::new(SHOWN);
        assert_eq!(
            run_with(&gh, &herdr, Some(&counts()), &state).digest,
            Notified::Shown
        );
        assert_eq!(std::fs::read_to_string(&state).unwrap(), "2026-11-08");
        // 同じ日の 2 回目は、Herdr を呼ばない。
        assert_eq!(
            run_with(&gh, &herdr, Some(&counts()), &state).digest,
            Notified::AlreadyToday
        );
        assert_eq!(herdr.shown.borrow().len(), 1);
        // 翌日は、また出す。
        let next_day = run(&Run {
            gh: &gh,
            herdr: &herdr,
            repo: "example/ledger",
            now_unix: NOW + 24 * H,
            counts: Some(&counts()),
            notified_path: &state,
        });
        assert_eq!(next_day.digest, Notified::Shown);
        assert_eq!(herdr.shown.borrow().len(), 2);
    }

    #[test]
    fn a_notice_herdr_did_not_show_is_not_recorded_so_it_is_tried_again() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("notified");
        let gh = FakeGh::new(&list_json("x"), &comment_json(Some(NOW - 40 * H)));
        for response in [
            r#"{"result":{"shown":false,"reason":"not_focused"}}"#,
            r#"{"result":{"shown":false,"reason":"disabled"}}"#,
            "not json",
        ] {
            let herdr = FakeHerdr::new(response);
            assert_eq!(
                run_with(&gh, &herdr, Some(&counts()), &state).digest,
                Notified::NotShown,
                "{response}"
            );
            assert!(!state.exists(), "{response}");
        }
        let broken = FakeHerdr {
            response: Err("herdr を起動できない".into()),
            shown: RefCell::new(Vec::new()),
        };
        assert_eq!(
            run_with(&gh, &broken, Some(&counts()), &state).digest,
            Notified::NotShown
        );
    }

    #[test]
    fn a_failing_gh_stops_nothing_else_and_never_notifies_on_a_guess() {
        let tmp = tempfile::tempdir().unwrap();
        let herdr = FakeHerdr::new(SHOWN);

        // 一覧が失敗: どちらも判断しない。
        let mut gh = FakeGh::new("[]", "");
        gh.list = Err("gh が失敗した".into());
        let report = run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n"));
        assert_eq!(
            report,
            Report {
                heartbeat: Wrote::Failed,
                digest: Notified::Failed
            }
        );

        // 一覧が JSON でない。
        let gh = FakeGh::new("not json", "");
        assert_eq!(
            run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n")).heartbeat,
            Wrote::Failed
        );

        // 書き込みが失敗しても、逆向きの確認は行われる。
        let mut gh = FakeGh::new(&list_json("old"), &comment_json(Some(NOW - 40 * H)));
        gh.edit = Err("gh が失敗した".into());
        let report = run_with(&gh, &herdr, Some(&counts()), &tmp.path().join("n"));
        assert_eq!(
            report,
            Report {
                heartbeat: Wrote::Failed,
                digest: Notified::Shown
            }
        );

        // コメントの取得が失敗しても、生存の記録は書かれる。通知はしない(古いかどうか分からない)。
        let herdr2 = FakeHerdr::new(SHOWN);
        let mut gh = FakeGh::new(&list_json("old"), "");
        gh.comments = Err("gh が失敗した".into());
        let report = run_with(&gh, &herdr2, Some(&counts()), &tmp.path().join("m"));
        assert_eq!(
            report,
            Report {
                heartbeat: Wrote::Written,
                digest: Notified::Failed
            }
        );
        assert!(herdr2.shown.borrow().is_empty());

        // 形の違う応答も、通知の根拠にしない。
        for bad in [
            r#"{"data":null}"#,
            r#"{"data":{"repository":{"issue":{"comments":{"nodes":[{"createdAt":"yesterday"}]}}}}}"#,
        ] {
            let gh = FakeGh::new(&list_json("x"), bad);
            let herdr3 = FakeHerdr::new(SHOWN);
            assert_eq!(
                run_with(&gh, &herdr3, Some(&counts()), &tmp.path().join("k")).digest,
                Notified::Failed,
                "{bad}"
            );
            assert!(herdr3.shown.borrow().is_empty());
        }
    }

    #[test]
    fn the_comment_query_asks_for_the_last_comment_of_the_digest_issue() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = FakeGh::new(&list_json("x"), &comment_json(Some(NOW - H)));
        run_with(
            &gh,
            &FakeHerdr::new(SHOWN),
            Some(&counts()),
            &tmp.path().join("n"),
        );
        let calls = gh.calls.borrow();
        let graphql = calls.iter().find(|c| c[0] == "api").unwrap().join(" ");
        assert!(graphql.contains("comments(last:1)"), "{graphql}");
        assert!(graphql.contains("owner=example"), "{graphql}");
        assert!(graphql.contains("name=ledger"), "{graphql}");
        assert!(graphql.contains("number=12"), "{graphql}");
    }
}
