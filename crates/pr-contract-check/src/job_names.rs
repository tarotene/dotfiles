//! workflow の YAML から、job が check として報告する名前を静的に読む(ADR-503 Amendment
//! 2026-10-27、#1014・#1065)。
//!
//! `rulesets-context-check` は、実行中の job 名が Actions API に現れるまで待つ代わりに、
//! この head の `.github/workflows/` 配下の**全 workflow**から読んだ名前と、宣言の required
//! context を突き合わせる。required context は 1 つの workflow に収まらない(`CI passed` は
//! `ci.yml`、`PR title` は別 workflow の `pr-title.yml`)ので、`ci.yml` 専用の
//! [`crate::workflows::parse_ci_yaml`] ではなく、任意の workflow を読めるこの走査を使う。
//!
//! 汎用の YAML パーサは使わない(この crate は依存ゼロ — 他リポジトリの CI が sparse-checkout
//! して `cargo build` する)。行ベースで、`jobs:` の直下の job と、その直下の `name:` /
//! `strategy:` / `uses:` だけを読む。インデント幅は固定せず、最初に現れた幅を使う。
//!
//! 読み切れないものは「不明(unresolved)」として持つ。名前が `${{ … }}` を含むものはワイルド
//! カード、matrix の job は `<名前> (…)`、reusable workflow を呼ぶ job は `<名前> / …` を
//! 取りうる名前として足す。**突き合わせが外れたときに、不明な workflow があれば fail でなく
//! warning に倒す**(判定できないことを、他リポジトリの CI の fail にしない)。

use std::collections::BTreeSet;

/// 1 つ以上の workflow から読んだ、job が報告しうる名前。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobNames {
    /// 固定の名前(`name:` が文字列、無ければ job id)。
    exact: BTreeSet<String>,
    /// `*` を任意の文字列とみなす名前(`name:` に式がある、matrix、reusable の呼び出し)。
    patterns: BTreeSet<String>,
    /// 読み切れなかった workflow のファイル名。
    unresolved: BTreeSet<String>,
}

impl JobNames {
    pub fn new() -> JobNames {
        JobNames::default()
    }

    /// 固定の名前だけを持つ集合(テスト用の簡便な入口)。
    pub fn from_exact<I, S>(names: I) -> JobNames
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        JobNames {
            exact: names.into_iter().map(Into::into).collect(),
            ..JobNames::default()
        }
    }

    pub fn extend(&mut self, other: JobNames) {
        self.exact.extend(other.exact);
        self.patterns.extend(other.patterns);
        self.unresolved.extend(other.unresolved);
    }

    /// `context` が、読んだ名前のどれかと一致するか。
    pub fn matches(&self, context: &str) -> bool {
        self.exact.contains(context) || self.patterns.iter().any(|p| glob_match(p, context))
    }

    /// 読み切れなかった workflow があるか(あれば、不一致を fail にしない)。
    pub fn has_unresolved(&self) -> bool {
        !self.unresolved.is_empty()
    }

    pub fn unresolved_files(&self) -> impl Iterator<Item = &String> {
        self.unresolved.iter()
    }

    pub fn exact(&self) -> impl Iterator<Item = &String> {
        self.exact.iter()
    }
}

/// `*` だけを特殊文字とする glob(`?` や `[` は特殊でない)。
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn skippable(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// 値の後ろの ` #` コメントを落とす(引用符の中の `#` は残す)。
fn strip_comment(v: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_space = true;
    for (i, c) in v.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && prev_space => return &v[..i],
            None => {}
        }
        prev_space = c == ' ' || c == '\t';
    }
    v
}

/// YAML のスカラー値。名前として読めない(ブロックスカラー・アンカー・フロー形式・空)ときは
/// `None`。
fn scalar(v: &str) -> Option<String> {
    let v = strip_comment(v).trim();
    let first = v.chars().next()?;
    match first {
        '"' => {
            let mut out = String::new();
            let mut chars = v[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => out.push(chars.next()?),
                    '"' => return Some(out),
                    c => out.push(c),
                }
            }
            None
        }
        '\'' => {
            let mut out = String::new();
            let mut chars = v[1..].chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        out.push('\'');
                    } else {
                        return Some(out);
                    }
                } else {
                    out.push(c);
                }
            }
            None
        }
        '|' | '>' | '&' | '*' | '!' | '{' | '[' | '%' | '@' | '`' => None,
        _ => Some(v.to_string()),
    }
}

/// `${{ … }}` を `*` に置き換える。式があったかも返す。
fn expressions_to_wildcards(s: &str) -> (String, bool) {
    let mut out = String::new();
    let mut rest = s;
    let mut found = false;
    while let Some(i) = rest.find("${{") {
        out.push_str(&rest[..i]);
        match rest[i..].find("}}") {
            Some(j) => {
                out.push('*');
                found = true;
                rest = &rest[i + j + 2..];
            }
            None => {
                // 閉じていない式: 残りを全部ワイルドカードにする。
                out.push('*');
                return (out, true);
            }
        }
    }
    out.push_str(rest);
    (out, found)
}

#[derive(Default)]
struct Job {
    id: String,
    /// `None` = `name:` 無し、`Some(None)` = あるが読めない。
    name: Option<Option<String>>,
    has_strategy: bool,
    uses: bool,
}

fn finish(job: Job, names: &mut JobNames, file: &str) {
    let base = match job.name {
        None => job.id.clone(),
        Some(Some(n)) => n,
        Some(None) => {
            names.unresolved.insert(file.to_string());
            return;
        }
    };
    let (pattern, has_expr) = expressions_to_wildcards(&base);
    if job.uses {
        // reusable workflow の呼び出し: 報告名は `<呼び出し側の job 名> / <呼ばれる job 名>`。
        names.patterns.insert(format!("{pattern} / *"));
    } else if has_expr {
        names.patterns.insert(pattern);
    } else if job.has_strategy {
        // matrix の job は、名前に式が無ければ `<名前> (<値>, …)` と報告される。
        names.exact.insert(base);
        names.patterns.insert(format!("{pattern} (*)"));
    } else {
        names.exact.insert(base);
    }
}

/// 1 つの workflow(`file` はメッセージ用のファイル名)から、job が報告しうる名前を読む。
pub fn parse(file: &str, text: &str) -> JobNames {
    let mut names = JobNames::new();
    let lines: Vec<&str> = text.lines().collect();
    let jobs_at = lines.iter().position(|l| {
        l.strip_prefix("jobs:")
            .is_some_and(|rest| strip_comment(rest).trim().is_empty())
    });
    let Some(jobs_at) = jobs_at else {
        // `jobs:` が行頭に無い。フロー形式(`jobs: {…}`)や、読めない形かもしれない。
        if lines.iter().any(|l| l.contains("jobs:")) {
            names.unresolved.insert(file.to_string());
        }
        return names;
    };

    let mut job_indent: Option<usize> = None;
    let mut prop_indent: Option<usize> = None;
    let mut cur: Option<Job> = None;
    for line in &lines[jobs_at + 1..] {
        if skippable(line) {
            continue;
        }
        let ind = indent_of(line);
        if ind == 0 {
            break; // `jobs:` の次のトップレベルのキー
        }
        let ji = *job_indent.get_or_insert(ind);
        let body = line.trim_start_matches(' ');
        if ind == ji {
            if let Some(j) = cur.take() {
                finish(j, &mut names, file);
            }
            prop_indent = None;
            match body.split_once(':') {
                Some((id, rest))
                    if !id.is_empty()
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                        && strip_comment(rest).trim().is_empty() =>
                {
                    cur = Some(Job {
                        id: id.to_string(),
                        ..Job::default()
                    });
                }
                // アンカー・マージキー・引用符つきの id・1 行に収めた job: 読めない。
                _ => {
                    names.unresolved.insert(file.to_string());
                }
            }
        } else if ind > ji {
            let pi = *prop_indent.get_or_insert(ind);
            if ind == pi {
                if let (Some(job), Some((key, rest))) = (cur.as_mut(), body.split_once(':')) {
                    match key {
                        "name" => job.name = Some(scalar(rest)),
                        "strategy" => job.has_strategy = true,
                        "uses" => job.uses = true,
                        _ => {}
                    }
                }
            }
        }
    }
    if let Some(j) = cur.take() {
        finish(j, &mut names, file);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_name_or_job_id() {
        let n = parse(
            "ci.yml",
            "name: CI\non: push\njobs:\n  a:\n    name: Alpha\n    runs-on: x\n  b-two:\n    runs-on: x\n",
        );
        assert!(n.matches("Alpha"));
        assert!(n.matches("b-two"));
        assert!(!n.matches("a"), "name: があれば job id は報告されない");
        assert!(!n.matches("alpha"), "大文字小文字は区別する");
        assert!(!n.has_unresolved());
    }

    #[test]
    fn step_names_and_workflow_name_are_not_job_names() {
        let n = parse(
            "ci.yml",
            "name: Workflow\njobs:\n  j:\n    name: Job\n    steps:\n      - name: Step\n        run: x\n",
        );
        assert!(n.matches("Job"));
        assert!(!n.matches("Step"));
        assert!(!n.matches("Workflow"));
    }

    #[test]
    fn four_space_indent_and_comments_and_quotes() {
        let n = parse(
            "x.yml",
            "jobs: # comment\n    one:   # c\n        name: \"Quoted # not a comment\" # real comment\n    two:\n        name: 'It''s'\n",
        );
        assert!(n.matches("Quoted # not a comment"));
        assert!(n.matches("It's"));
    }

    #[test]
    fn expressions_become_wildcards() {
        let n = parse(
            "ci.yml",
            "jobs:\n  build-host:\n    name: Build ${{ matrix.host }}\n    strategy:\n      matrix:\n        host: [a]\n",
        );
        assert!(n.matches("Build vega"));
        assert!(n.matches("Build arcturus"));
        assert!(!n.matches("CI passed"));
    }

    #[test]
    fn matrix_job_without_expression_gets_value_suffix() {
        let n = parse(
            "ci.yml",
            "jobs:\n  t:\n    name: Test\n    strategy:\n      matrix:\n        os: [a, b]\n",
        );
        assert!(n.matches("Test"));
        assert!(n.matches("Test (a)"));
        assert!(n.matches("Test (a, b)"));
        assert!(!n.matches("Tests"));
    }

    #[test]
    fn reusable_workflow_caller_reports_a_joined_name() {
        let n = parse(
            "x.yml",
            "jobs:\n  check:\n    name: PR Title\n    uses: o/r/.github/workflows/p.yml@main\n",
        );
        assert!(n.matches("PR Title / PR title"));
        assert!(!n.matches("PR title"));
    }

    #[test]
    fn unreadable_shapes_are_unresolved_not_silent() {
        for src in [
            "jobs: { a: { runs-on: x } }\n",
            "jobs:\n  a:\n    name: >-\n      folded\n",
            "jobs:\n  <<: *base\n",
            "jobs:\n  a: { runs-on: x }\n",
        ] {
            let n = parse("odd.yml", src);
            assert!(n.has_unresolved(), "{src:?}");
            assert_eq!(n.unresolved_files().next().unwrap(), "odd.yml");
        }
        // jobs を持たない YAML(workflow でない)は何も報告せず、不明でもない。
        let n = parse("x.yml", "name: not a workflow\non: push\n");
        assert!(!n.has_unresolved());
        assert!(!n.matches("anything"));
    }

    #[test]
    fn top_level_keys_after_jobs_end_the_block() {
        let n = parse(
            "x.yml",
            "jobs:\n  a:\n    name: A\nenv:\n  b:\n    name: NotAJob\n",
        );
        assert!(n.matches("A"));
        assert!(!n.matches("NotAJob"));
    }

    #[test]
    fn glob_matching() {
        assert!(glob_match("Build *", "Build vega"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b", "ac"));
        assert!(glob_match("a (*)", "a (x, y)"));
        assert!(!glob_match("Build *", "Build"));
    }

    #[test]
    fn extend_merges_all_three_sets() {
        let mut a = parse("a.yml", "jobs:\n  x:\n    name: X\n");
        a.extend(parse("b.yml", "jobs:\n  y:\n    name: Y ${{ z }}\n"));
        a.extend(parse("c.yml", "jobs: {}\n"));
        assert!(a.matches("X") && a.matches("Y 1") && a.has_unresolved());
    }
}
