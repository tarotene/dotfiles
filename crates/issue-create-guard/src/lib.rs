//! issue-create-guard — `gh issue create` を作成時点で検査する PreToolUse(Bash) の判定
//! (設計と根拠は docs/agents/issue-create-guard.md、ADR-1379)。
//!
//! 本人の Issue 台帳の不変条件は、未完了の Issue が (1) ボール保持者を排他ラベル
//! `ball:me` / `ball:them` / `ball:agent` のちょうど 1 つで持ち、(2) 次に動く契機
//! `next: YYYY-MM-DD` を持つこと。これまでは事後の検査だけで、ボールを持たない Issue を
//! 作れてしまった。ここでは作成の呼び出し自体を止める。
//!
//! 対象は `gh issue create` で、対象リポジトリの owner が `tarotene` のときだけ
//! (`-R/--repo`、無ければ作業ディレクトリの remote から読む。他の org・第三者のリポジトリは
//! 対象外)。検査は 2 つ。
//!
//! - ラベル: `--label`(`-l`、複数指定・カンマ区切りの両方)に、ボールのラベルがちょうど 1 つ。
//! - 本文の先頭行(`--body` / `--body-file`): 非公開リポジトリなら、先頭の非空行が
//!   `next: YYYY-MM-DD`(実在の日付)。公開リポジトリなら、`next:` `due:` `postponed:` で
//!   始まる行を本文に書かない(日付を公開に載せないため。日付は別の非公開の台帳が持つ)。
//!
//! 免除: ラベルに `ledger-status`(機械が書く常設の状態 Issue)があるとき、両方とも免除する。
//!
//! 縮退(fail-open): owner を決められない、ラベルや本文がシェルの展開(`$(…)`・変数)で中身が
//! 分からない、本文ファイルが読めない、リポジトリの公開/非公開を引けないときは、決められない
//! 検査を飛ばす(判定不能を deny に変えない)。引けないときはラベルだけを検査する。
//!
//! 軸: 検出のみ — `gh api repos/…/issues` や Web UI からの作成、MCP 経由の作成はこの hook の
//! 対象外(事後の検査が拾う)。`gh issue create` の経路を実行前に弾いて、書き直しの往復を 1 回に抑える
//! ところまでが責務。

use agent_session::hook::{jq_r_path, ToolCall};
use agent_session::{Agent, Outcome};
use command_inspection::command::{first_deny, gh_command_at};
use command_inspection::shell::is_sep;
use git_repository::repo::owner_repo;
use github_command::gh::{assemble_body_text, has_heredoc, BodyFlags};
use std::collections::BTreeSet;
use std::path::Path;

/// 検査の対象にする owner。
const OWNER: &str = "tarotene";

/// ボール保持者の排他ラベル(閉語彙)。
pub const BALL_LABELS: [&str; 3] = ["ball:me", "ball:them", "ball:agent"];

/// 機械が書く常設の状態 Issue のラベル。両方の検査を免除する。
pub const EXEMPT_LABEL: &str = "ledger-status";

/// 公開リポジトリの本文に書いてはならない行の接頭辞(日付を公開に載せない)。
const PUBLIC_FORBIDDEN_PREFIXES: [&str; 3] = ["next:", "due:", "postponed:"];

/// リポジトリの公開/非公開。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    /// 非公開(`PRIVATE`)と組織内(`INTERNAL`)。本文に日付を置いてよい側。
    NonPublic,
}

/// `gh repo view --json visibility --jq .visibility` の出力を読む。読めなければ `None`。
pub fn parse_visibility(s: &str) -> Option<Visibility> {
    match s.trim() {
        "PUBLIC" => Some(Visibility::Public),
        "PRIVATE" | "INTERNAL" => Some(Visibility::NonPublic),
        _ => None,
    }
}

/// `gh repo view <owner/repo>` で公開/非公開を引く。引けなければ `None`(呼び出し側は日付の検査を飛ばす)。
pub fn gh_visibility(nwo: &str) -> Option<Visibility> {
    let out = github_command::exec::gh_ok(&[
        "repo",
        "view",
        nwo,
        "--json",
        "visibility",
        "--jq",
        ".visibility",
    ])?;
    parse_visibility(&out)
}

/// `gh issue create` だけを対象にする。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<()> {
    gh_command_at(tokens, i, &["issue", "create"]).then_some(())
}

/// `gh issue create` が値を取るフラグ(読み飛ばすときに、値を別のフラグと取り違えないため)。
const VALUE_FLAGS: [&str; 15] = [
    "--assignee",
    "-a",
    "--body",
    "-b",
    "--body-file",
    "-F",
    "--milestone",
    "-m",
    "--project",
    "-p",
    "--recover",
    "--template",
    "-T",
    "--title",
    "-t",
];

#[derive(Debug, Default, PartialEq, Eq)]
struct Flags {
    /// カンマで分けたラベルの全部(出現順)。
    labels: Vec<String>,
    /// `-R/--repo` の値(最後の指定)。
    repo: Option<String>,
}

/// `-l` / `--label` と `-R` / `--repo` を 1 パスで読む(`--title -R` のように値が別フラグの綴りを持つ
/// 場合に取り違えないため、値を取るフラグは値ごと読み飛ばす)。`tokens` は `gh issue create` の
/// 次から。
fn scan_flags(tokens: &[String]) -> Flags {
    let mut f = Flags::default();
    let n = tokens.len();
    let mut i = 0;
    while i < n {
        let t = tokens[i].as_str();
        let take = |f: &mut Flags, flag: &str, v: &str| {
            if flag == "label" {
                f.labels.extend(
                    v.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                );
            } else {
                f.repo = Some(v.to_string());
            }
        };
        match t {
            "--label" | "-l" | "--repo" | "-R" => {
                let flag = if matches!(t, "--label" | "-l") {
                    "label"
                } else {
                    "repo"
                };
                if let Some(v) = tokens.get(i + 1) {
                    take(&mut f, flag, v);
                }
                i += 2;
                continue;
            }
            _ if VALUE_FLAGS.contains(&t) => {
                i += 2;
                continue;
            }
            _ => {}
        }
        if let Some(v) = t.strip_prefix("--label=") {
            take(&mut f, "label", v);
        } else if let Some(v) = t.strip_prefix("--repo=") {
            take(&mut f, "repo", v);
        } else if let Some(v) = short_attached(t, 'l') {
            take(&mut f, "label", v);
        } else if let Some(v) = short_attached(t, 'R') {
            take(&mut f, "repo", v);
        }
        i += 1;
    }
    f
}

/// `-lfoo` / `-l=foo` のように、短いフラグに値が続けて付いた形の値。
fn short_attached(t: &str, flag: char) -> Option<&str> {
    let rest = t.strip_prefix('-')?.strip_prefix(flag)?;
    if t.starts_with("--") || rest.is_empty() {
        return None;
    }
    Some(rest.strip_prefix('=').unwrap_or(rest))
}

/// `[HOST/]OWNER/REPO` を `OWNER/REPO` にする。`OWNER/REPO` の形でなければ `None`。
fn normalize_nwo(s: &str) -> Option<String> {
    if s.contains(['$', '`']) {
        return None;
    }
    let parts: Vec<&str> = s.split('/').collect();
    let [.., owner, repo] = parts.as_slice() else {
        return None;
    };
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

/// 本文の先頭の非空行が `next: YYYY-MM-DD`(実在の日付。後ろに空白以外が続かない)か。
pub fn first_line_is_next(body: &str) -> bool {
    body.lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.trim_end().strip_prefix("next: "))
        .is_some_and(is_real_date)
}

/// 本文に `next:` `due:` `postponed:` で始まる行があるか(公開リポジトリで日付を載せない検査)。
pub fn has_date_line(body: &str) -> bool {
    body.lines().any(|l| {
        let l = l.trim_start().to_ascii_lowercase();
        PUBLIC_FORBIDDEN_PREFIXES.iter().any(|p| l.starts_with(p))
    })
}

/// `YYYY-MM-DD` で、暦に実在する日付か。
fn is_real_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        b[r].iter().try_fold(0u32, |acc, c| {
            c.is_ascii_digit().then(|| acc * 10 + u32::from(c - b'0'))
        })
    };
    let (Some(y), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=dim).contains(&d)
}

/// 範囲の本文。分からないとき(本文フラグが無い・中身がシェルの展開・ファイルが読めない)は `None`。
fn body_of(tokens: &[String], heredoc_bodies: &str) -> Option<String> {
    let flags = BodyFlags::scan(tokens);
    if !flags.have_flag || !flags.unreadable_files.is_empty() {
        return None;
    }
    if has_heredoc(tokens) && !heredoc_bodies.is_empty() {
        return Some(heredoc_bodies.to_string());
    }
    // `--body-file -`(標準入力)や空の指定で、値が 1 つも取れなかったときは中身が不明。
    // 空の本文(`--body ""`)は texts が `[""]` になり、空の本文として扱う。
    if flags.texts.is_empty() {
        return None;
    }
    // コマンド置換の判定は、インラインの `--body` の値だけに掛ける。`--body-file` の中身は読んだ後の
    // 確定した文字列で、Markdown のインラインコード(バッククォート)を含んでよい
    // (`github_command::gh::assemble_body_text` の説明)。
    let inline = flags.inline_texts.join("\n");
    if inline.contains("$(") || inline.contains('`') {
        return None;
    }
    // 空白だけでも本文として返す(非公開側では「先頭行が無い」違反になる)。
    Some(assemble_body_text(&flags.inline_texts, &flags.file_texts, false, "").unwrap_or_default())
}

fn rewrite_hint() -> &'static str {
    "例: gh issue create -R tarotene/<repo> --label ball:me --body-file <絶対パス> …(複数ラベルは --label ball:me,<他のラベル>)。\
機械が書く常設の状態 Issue だけは --label ledger-status で免除されます。"
}

/// ラベルの違反。ボールのラベルがちょうど 1 つでなければ理由を返す。
fn judge_labels(labels: &[String]) -> Option<String> {
    let balls: BTreeSet<&str> = labels
        .iter()
        .map(String::as_str)
        .filter(|l| BALL_LABELS.contains(l))
        .collect();
    let unknown: Vec<&str> = labels
        .iter()
        .map(String::as_str)
        .filter(|l| l.starts_with("ball:") && !BALL_LABELS.contains(l))
        .collect();
    match balls.len() {
        1 if unknown.is_empty() => None,
        1 => Some(format!(
            "ボールのラベルの語彙の外です: {}(使えるのは {})。",
            unknown.join(", "),
            BALL_LABELS.join(" / ")
        )),
        0 => Some(format!(
            "ボールのラベルがありません。ball:me(本人)/ ball:them(相手待ち)/ ball:agent(エージェント)のちょうど 1 つを --label に付けてください{}。",
            if unknown.is_empty() {
                String::new()
            } else {
                format!("(語彙の外: {})", unknown.join(", "))
            }
        )),
        _ => Some(format!(
            "ボールのラベルが複数あります: {}。排他なので 1 つにしてください。",
            balls.into_iter().collect::<Vec<_>>().join(", ")
        )),
    }
}

/// 本文の違反。公開/非公開で規則が逆になる。
fn judge_body(body: &str, visibility: Visibility) -> Option<String> {
    match visibility {
        Visibility::NonPublic if !first_line_is_next(body) => Some(
            "非公開リポジトリの Issue は、本文の先頭の非空行を `next: YYYY-MM-DD`(実在の日付。次に動く契機)にしてください。"
                .to_string(),
        ),
        Visibility::Public if has_date_line(body) => Some(
            "公開リポジトリの Issue 本文には、`next:` `due:` `postponed:` で始まる行を書けません(日付を公開に載せません。日付は非公開の台帳に持ちます)。"
                .to_string(),
        ),
        _ => None,
    }
}

/// コマンド文字列全体を判定する。deny なら理由。
///
/// - `default_nwo`: `-R` が無いときの対象(作業ディレクトリの remote)。
/// - `visibility`: `owner/repo` の公開/非公開。引けなければ `None`。
pub fn decide(
    cmd: &str,
    default_nwo: impl Fn() -> Option<String>,
    visibility: impl Fn(&str) -> Option<Visibility>,
) -> Option<String> {
    first_deny(cmd, is_target_at, |range| {
        // 範囲は次の対象コマンドまで続くので、最初の区切りで切る(後ろの別コマンドの
        // フラグを読まない)。先頭の `gh issue create` の 3 語は読み飛ばす。
        let end = range
            .tokens
            .iter()
            .position(|t| is_sep(t))
            .unwrap_or(range.tokens.len());
        let tokens = &range.tokens[3.min(end)..end];
        let flags = scan_flags(tokens);

        let nwo = match &flags.repo {
            Some(r) => normalize_nwo(r)?,
            None => default_nwo()?,
        };
        if nwo
            .split('/')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some(OWNER)
        {
            return None;
        }
        // ラベルがシェルの展開で中身が分からないときは、免除かどうかも言えないので判定しない。
        if flags.labels.iter().any(|l| l.contains(['$', '`'])) {
            return None;
        }
        if flags.labels.iter().any(|l| l == EXEMPT_LABEL) {
            return None;
        }

        let mut problems: Vec<String> = Vec::new();
        problems.extend(judge_labels(&flags.labels));

        if let Some(body) = body_of(tokens, range.heredoc_bodies) {
            // 公開/非公開を引けないとき(gh が使えない・認証が無い・遅い)は、日付の検査を飛ばして
            // ラベルだけを検査する。引けないことを理由に deny へ倒すと、判定できない呼び出しを
            // 止めてしまうため(fail-open)。
            if let Some(v) = visibility(&nwo) {
                problems.extend(judge_body(&body, v));
            }
        }

        if problems.is_empty() {
            return None;
        }
        Some(format!(
            "tarotene 配下のリポジトリの `gh issue create` は、本人の Issue 台帳の不変条件(ボールを 1 つ持ち、次の契機を持つ)を作成時に満たす必要があります(対象: {nwo}):\n\n- {}\n\n{}",
            problems.join("\n- "),
            rewrite_hint()
        ))
    })
}

/// hook 本体(`main` が標準入力を読んだあとの処理)。ルータ(`crates/hook-router`)が呼ぶ。
/// 入力が読めない・不正 JSON・Bash 以外は `None`(判定しない)。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    let call = ToolCall::parse(agent, raw)?;
    let cmd = call.bash_command()?;
    let project = match agent {
        Agent::Claude => std::env::var("CLAUDE_PROJECT_DIR")
            .ok()
            .filter(|p| !p.is_empty()),
        _ => None,
    }
    .or_else(|| jq_r_path(&call.raw, &["cwd"]).ok().flatten())
    .unwrap_or_default();
    decide(
        &cmd,
        || {
            (!project.is_empty())
                .then(|| owner_repo(Path::new(&project)))
                .flatten()
        },
        gh_visibility,
    )
    .map(Outcome::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIV: fn(&str) -> Option<Visibility> = |_| Some(Visibility::NonPublic);
    const PUB: fn(&str) -> Option<Visibility> = |_| Some(Visibility::Public);
    const UNKNOWN: fn(&str) -> Option<Visibility> = |_| None;

    fn run(cmd: &str, v: fn(&str) -> Option<Visibility>) -> Option<String> {
        decide(cmd, || Some("tarotene/here".to_string()), v)
    }

    fn write(dir: &tempfile::TempDir, name: &str, body: &str) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn passes_a_private_issue_with_one_ball_and_a_next_line() {
        let d = tempfile::tempdir().unwrap();
        let f = write(&d, "b.md", "next: 2027-02-28\n\n本文\n");
        let cmd =
            format!("gh issue create -R tarotene/x --title t --label ball:me --body-file {f}");
        assert_eq!(run(&cmd, PRIV), None);
    }

    #[test]
    fn viewing_the_help_is_not_creating_an_issue() {
        for cmd in [
            "gh issue create --help",
            "gh issue create -h",
            "gh issue create -R tarotene/x --help",
        ] {
            assert_eq!(run(cmd, PRIV), None, "{cmd}");
        }
    }

    #[test]
    fn denies_a_missing_ball_label_and_shows_the_rewrite() {
        let r = run("gh issue create -R tarotene/x --title t --label bug", PRIV).unwrap();
        assert!(r.contains("ボールのラベルがありません"), "{r}");
        assert!(r.contains("--label ball:me"), "{r}");
        assert!(run("gh issue create -R tarotene/x --title t", PRIV).is_some());
    }

    #[test]
    fn reads_every_label_spelling() {
        for cmd in [
            "gh issue create -R tarotene/x --label ball:me,bug",
            "gh issue create -R tarotene/x --label bug --label ball:them",
            "gh issue create -R tarotene/x -l ball:agent",
            "gh issue create -R tarotene/x --label=ball:agent",
            "gh issue create -R tarotene/x -lball:me",
            "gh issue create -R tarotene/x -l=ball:me",
            "gh issue create -R tarotene/x --label 'bug, ball:me'",
            "gh issue create --repo tarotene/x --label ball:me",
            "gh issue create --repo=tarotene/x --label ball:me",
            "gh issue create -Rtarotene/x --label ball:me",
        ] {
            assert_eq!(run(cmd, UNKNOWN), None, "{cmd}");
        }
    }

    #[test]
    fn denies_two_distinct_balls_but_not_the_same_one_twice() {
        let r = run(
            "gh issue create -R tarotene/x --label ball:me --label ball:them",
            UNKNOWN,
        )
        .unwrap();
        assert!(r.contains("複数"), "{r}");
        assert_eq!(
            run(
                "gh issue create -R tarotene/x --label ball:me --label ball:me",
                UNKNOWN
            ),
            None
        );
    }

    #[test]
    fn denies_a_ball_label_outside_the_vocabulary() {
        let r = run("gh issue create -R tarotene/x --label ball:you", UNKNOWN).unwrap();
        assert!(r.contains("ball:you"), "{r}");
        let r = run(
            "gh issue create -R tarotene/x --label ball:me,ball:you",
            UNKNOWN,
        )
        .unwrap();
        assert!(r.contains("語彙の外"), "{r}");
    }

    #[test]
    fn a_title_that_looks_like_a_flag_does_not_hide_the_label() {
        // `--title -l` の `-l` は題名で、ラベルの指定ではない。
        let r = run("gh issue create -R tarotene/x --title -l --body b", UNKNOWN);
        assert!(r.is_some());
        assert_eq!(
            run(
                "gh issue create -R tarotene/x --title -l --label ball:me",
                UNKNOWN
            ),
            None
        );
    }

    #[test]
    fn only_tarotene_owned_repositories_are_checked() {
        assert_eq!(run("gh issue create -R acme/x --title t", PRIV), None);
        assert_eq!(
            run("gh issue create -R someone-else/tarotene --title t", PRIV),
            None
        );
        assert!(run("gh issue create -R TAROTENE/x --title t", PRIV).is_some());
        // ホスト付きの指定も owner を読める。
        assert!(run("gh issue create -R github.com/tarotene/x --title t", PRIV).is_some());
        assert_eq!(
            run("gh issue create -R github.com/acme/x --title t", PRIV),
            None
        );
    }

    #[test]
    fn without_dash_r_the_working_directory_repository_decides() {
        let theirs = decide("gh issue create --title t", || Some("acme/y".into()), PRIV);
        assert_eq!(theirs, None);
        let mine = decide(
            "gh issue create --title t",
            || Some("tarotene/y".into()),
            PRIV,
        );
        assert!(mine.is_some());
        // 作業ディレクトリのリポジトリも決められなければ、判定しない。
        assert_eq!(decide("gh issue create --title t", || None, PRIV), None);
    }

    #[test]
    fn the_ledger_status_label_exempts_both_checks() {
        let d = tempfile::tempdir().unwrap();
        let f = write(&d, "b.md", "状態の本文\n");
        let cmd = format!("gh issue create -R tarotene/x --label ledger-status --body-file {f}");
        assert_eq!(run(&cmd, PRIV), None);
        assert_eq!(run(&cmd, PUB), None);
    }

    #[test]
    fn a_private_body_needs_a_real_next_date_on_the_first_non_empty_line() {
        let d = tempfile::tempdir().unwrap();
        let base = "gh issue create -R tarotene/x --label ball:me --body-file";
        let ok = write(&d, "ok.md", "\n\nnext: 2028-02-29\nrest\n");
        assert_eq!(run(&format!("{base} {ok}"), PRIV), None);
        for (name, body) in [
            ("none.md", "ただの本文\n"),
            ("late.md", "目的\nnext: 2027-01-01\n"),
            ("bad-day.md", "next: 2027-02-29\n"),
            ("bad-month.md", "next: 2027-13-01\n"),
            ("bad-form.md", "next: 2027-1-1\n"),
            ("trailing.md", "next: 2027-01-01 までに\n"),
            ("no-space.md", "next:2027-01-01\n"),
            ("empty.md", ""),
        ] {
            let f = write(&d, name, body);
            let r = run(&format!("{base} {f}"), PRIV).unwrap_or_else(|| panic!("{name}"));
            assert!(r.contains("next: YYYY-MM-DD"), "{name}: {r}");
        }
    }

    /// `--body-file` の中身にバッククォート(Markdown のインラインコード)があっても、本文の検査を
    /// 掛ける。以前は「中身が不明」として本文の検査を飛ばしていた(段 4 の穴と同じ形)。
    #[test]
    fn a_body_file_with_backticks_is_still_checked() {
        let d = tempfile::tempdir().unwrap();
        let base = "gh issue create -R tarotene/x --label ball:me --body-file";
        let bad = write(&d, "bad.md", "`cargo test` を実行する\n");
        let r =
            run(&format!("{base} {bad}"), PRIV).expect("backtick 入りでも先頭行が無ければ deny");
        assert!(r.contains("next: YYYY-MM-DD"), "{r}");
        let ok = write(&d, "ok.md", "next: 2028-02-29\n`cargo test` を実行する\n");
        assert_eq!(run(&format!("{base} {ok}"), PRIV), None);
    }

    #[test]
    fn a_public_body_must_not_carry_a_date_line() {
        let d = tempfile::tempdir().unwrap();
        let base = "gh issue create -R tarotene/x --label ball:me --body-file";
        let clean = write(&d, "clean.md", "目的\n日付は書かない\n");
        assert_eq!(run(&format!("{base} {clean}"), PUB), None);
        for (name, body) in [
            ("next.md", "next: 2027-01-01\n本文"),
            ("due.md", "本文\ndue: 2027-01-01\n"),
            ("postponed.md", "本文\n  Postponed: 2027-01-01\n"),
        ] {
            let f = write(&d, name, body);
            let r = run(&format!("{base} {f}"), PUB).unwrap_or_else(|| panic!("{name}"));
            assert!(r.contains("公開リポジトリ"), "{name}: {r}");
        }
        // 文中の語や、接頭辞に似た別の語は数えない。
        let f = write(&d, "prose.md", "次の due: は書かない\nnextstep: x\n");
        assert_eq!(run(&format!("{base} {f}"), PUB), None);
    }

    #[test]
    fn when_visibility_is_unknown_only_the_label_is_checked() {
        let d = tempfile::tempdir().unwrap();
        let f = write(&d, "b.md", "日付の無い本文\n");
        let base = "gh issue create -R tarotene/x --body-file";
        assert_eq!(run(&format!("{base} {f} --label ball:me"), UNKNOWN), None);
        let r = run(&format!("{base} {f}"), UNKNOWN).unwrap();
        assert!(r.contains("ボールのラベル") && !r.contains("next:"), "{r}");
    }

    #[test]
    fn an_unknown_body_skips_the_body_check() {
        // 本文フラグが無い・標準入力・読めないファイル・シェルの展開は、中身が分からない。
        for cmd in [
            "gh issue create -R tarotene/x --label ball:me --title t",
            "gh issue create -R tarotene/x --label ball:me --body-file -",
            "gh issue create -R tarotene/x --label ball:me --body-file /nonexistent/zzz.md",
            "gh issue create -R tarotene/x --label ball:me --body \"$(cat body.md)\"",
        ] {
            assert_eq!(run(cmd, PRIV), None, "{cmd}");
        }
        // 明示した空の本文は、空の本文として扱う。
        assert!(run(
            "gh issue create -R tarotene/x --label ball:me --body ''",
            PRIV
        )
        .is_some());
    }

    #[test]
    fn a_heredoc_body_is_read() {
        let ok = "gh issue create -R tarotene/x --label ball:me --body \"$(cat <<'EOF'\nnext: 2027-03-01\n本文\nEOF\n)\"";
        assert_eq!(run(ok, PRIV), None);
        let bad = "gh issue create -R tarotene/x --label ball:me --body \"$(cat <<'EOF'\n本文だけ\nEOF\n)\"";
        assert!(run(bad, PRIV).is_some());
        let public = "gh issue create -R tarotene/x --label ball:me --body \"$(cat <<'EOF'\ndue: 2027-03-01\nEOF\n)\"";
        assert!(run(public, PUB).is_some());
    }

    #[test]
    fn label_and_body_problems_are_reported_together() {
        let d = tempfile::tempdir().unwrap();
        let f = write(&d, "b.md", "本文\n");
        let r = run(
            &format!("gh issue create -R tarotene/x --body-file {f}"),
            PRIV,
        )
        .unwrap();
        assert!(
            r.contains("ボールのラベル") && r.contains("next: YYYY-MM-DD"),
            "{r}"
        );
    }

    #[test]
    fn shell_expanded_labels_are_not_judged() {
        assert_eq!(
            run("gh issue create -R tarotene/x --label \"$BALL\"", PRIV),
            None
        );
    }

    #[test]
    fn other_commands_and_other_ranges_are_left_alone() {
        for cmd in [
            "gh issue comment 1 -R tarotene/x --body x",
            "gh issue edit 1 -R tarotene/x --add-label bug",
            "gh pr create -R tarotene/x --title t",
            "echo 'gh issue create -R tarotene/x'",
            "ls",
        ] {
            assert_eq!(run(cmd, PRIV), None, "{cmd}");
        }
        // 後ろの別コマンドのフラグを、この Issue のものとして読まない。
        assert_eq!(
            run(
                "gh issue create -R tarotene/x --label ball:me && gh pr create --label bug -R acme/y",
                UNKNOWN
            ),
            None
        );
        // 1 つのコマンドに複数の作成があれば、違反のある方で止める。
        let two = "gh issue create -R tarotene/x --label ball:me && gh issue create -R tarotene/y --title t";
        assert!(run(two, UNKNOWN).unwrap().contains("tarotene/y"));
    }

    #[test]
    fn visibility_output_is_parsed_closed() {
        assert_eq!(parse_visibility("PUBLIC\n"), Some(Visibility::Public));
        assert_eq!(parse_visibility("PRIVATE"), Some(Visibility::NonPublic));
        assert_eq!(parse_visibility("INTERNAL"), Some(Visibility::NonPublic));
        assert_eq!(parse_visibility(""), None);
        assert_eq!(parse_visibility("public"), None);
    }

    #[test]
    fn real_dates_only() {
        for ok in ["2026-11-08", "2028-02-29", "2000-02-29", "2026-12-31"] {
            assert!(is_real_date(ok), "{ok}");
        }
        for bad in [
            "2027-02-29",
            "1900-02-29",
            "2026-00-10",
            "2026-04-31",
            "2026-4-01",
            "26-04-01",
            "2026/04/01",
            "2026-04-0a",
            "",
        ] {
            assert!(!is_real_date(bad), "{bad}");
        }
    }

    #[test]
    fn the_hook_reads_the_command_from_the_tool_call() {
        let raw = r#"{"tool_name":"Bash","cwd":"","tool_input":{"command":"gh issue create -R tarotene/x --title t"}}"#;
        // 公開/非公開は gh で引くので、ここではラベルの違反だけで deny になることを見る
        // (本文フラグが無いので、公開/非公開は引かない)。
        match hook(Agent::Claude, raw) {
            Some(Outcome::Deny(r)) => assert!(r.contains("ボールのラベル"), "{r}"),
            other => panic!("deny のはず: {other:?}"),
        }
        let other = r#"{"tool_name":"Write","tool_input":{"command":"gh issue create"}}"#;
        assert!(hook(Agent::Claude, other).is_none());
        assert!(hook(Agent::Claude, "not json").is_none());
    }
}
