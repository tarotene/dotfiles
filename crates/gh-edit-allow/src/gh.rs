//! `gh pr|issue edit|create` の引数解析。
//!
//! 未知のフラグを 1 つでも含むコマンドは `None` にする。値を取るフラグか
//! どうかが分からないと位置引数(対象番号)を取り違えるためで、その場合は
//! 判定しない(通常の確認フローに落ちる)。

/// `gh <kind> <verb>` の kind。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pr,
    Issue,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Pr => "pr",
            Kind::Issue => "issue",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Edit,
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhCommand {
    pub kind: Kind,
    pub verb: Verb,
    /// `-R/--repo` の値(`OWNER/REPO` または `HOST/OWNER/REPO`)。
    pub repo: Option<String>,
    pub positionals: Vec<String>,
}

// gh 2.x の `pr edit` / `issue edit` / `issue create` のフラグ(gh <cmd> --help)。
const VALUE_FLAGS: &[&str] = &[
    "-R",
    "--repo",
    "-t",
    "--title",
    "-b",
    "--body",
    "-F",
    "--body-file",
    "-B",
    "--base",
    "-m",
    "--milestone",
    "-a",
    "--assignee",
    "-l",
    "--label",
    "-p",
    "--project",
    "-T",
    "--template",
    "--recover",
    "--add-assignee",
    "--remove-assignee",
    "--add-label",
    "--remove-label",
    "--add-project",
    "--remove-project",
    "--add-reviewer",
    "--remove-reviewer",
];
const BOOL_FLAGS: &[&str] = &["--remove-milestone", "-w", "--web", "-e", "--editor"];

pub fn parse(words: &[String]) -> Option<GhCommand> {
    let (kind, verb, rest) = match words {
        [gh, k, v, rest @ ..] if gh == "gh" => {
            let kind = match k.as_str() {
                "pr" => Kind::Pr,
                "issue" => Kind::Issue,
                _ => return None,
            };
            let verb = match v.as_str() {
                "edit" => Verb::Edit,
                "create" => Verb::Create,
                _ => return None,
            };
            (kind, verb, rest)
        }
        _ => return None,
    };
    let mut repo = None;
    let mut positionals = Vec::new();
    let mut it = rest.iter();
    while let Some(w) = it.next() {
        if w == "--" {
            positionals.extend(it.by_ref().cloned());
            break;
        }
        if !w.starts_with('-') || w == "-" {
            positionals.push(w.clone());
            continue;
        }
        let (name, inline) = match w.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (w.as_str(), None),
        };
        if VALUE_FLAGS.contains(&name) {
            let value = match inline {
                Some(v) => v,
                None => it.next()?.clone(),
            };
            if name == "-R" || name == "--repo" {
                repo = Some(value);
            }
        } else if BOOL_FLAGS.contains(&name) && inline.is_none() {
        } else {
            return None;
        }
    }
    Some(GhCommand {
        kind,
        verb,
        repo,
        positionals,
    })
}

/// `-R` の値を小文字の `owner/repo` にする。github.com 以外のホストは `None`。
pub fn normalize_repo(r: &str) -> Option<String> {
    let parts: Vec<&str> = r.split('/').collect();
    let (owner, name) = match parts.as_slice() {
        [o, n] => (*o, *n),
        [host, o, n] if host.eq_ignore_ascii_case("github.com") => (*o, *n),
        _ => return None,
    };
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(format!("{owner}/{name}").to_ascii_lowercase())
}

/// `https://github.com/<o>/<r>/(pull|issues)/<N>` を (kind, owner/repo, N) に。
pub fn parse_url(u: &str) -> Option<(Kind, String, u64)> {
    let rest = u.strip_prefix("https://github.com/")?;
    let rest = rest.split(['#', '?']).next()?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let [o, r, k, n] = parts.as_slice() else {
        return None;
    };
    let kind = match *k {
        "pull" => Kind::Pr,
        "issues" => Kind::Issue,
        _ => return None,
    };
    let num = parse_number(n)?;
    if o.is_empty() || r.is_empty() {
        return None;
    }
    Some((kind, format!("{o}/{r}").to_ascii_lowercase(), num))
}

pub fn parse_number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        crate::shell::split(s).unwrap()
    }

    #[test]
    fn parses_edit() {
        let c = parse(&w("gh pr edit 12 --title x -R Octo/Hello --add-label=bug")).unwrap();
        assert_eq!(c.kind, Kind::Pr);
        assert_eq!(c.verb, Verb::Edit);
        assert_eq!(c.repo.as_deref(), Some("Octo/Hello"));
        assert_eq!(c.positionals, vec!["12"]);
    }

    #[test]
    fn flag_values_are_not_positionals() {
        let c = parse(&w("gh issue edit --title 99 5 6 --body-file b.md")).unwrap();
        assert_eq!(c.positionals, vec!["5", "6"]);
    }

    #[test]
    fn unknown_or_other_commands() {
        assert_eq!(parse(&w("gh pr edit 1 --frobnicate")), None);
        assert_eq!(parse(&w("gh pr merge 1")), None);
        assert_eq!(parse(&w("gh repo edit")), None);
        assert_eq!(parse(&w("hub pr edit 1")), None);
        assert_eq!(parse(&w("gh pr edit 1 --title")), None);
        assert_eq!(parse(&w("gh pr edit 1 --web=true")), None);
    }

    #[test]
    fn repo_normalization() {
        assert_eq!(normalize_repo("Octo/Hello").as_deref(), Some("octo/hello"));
        assert_eq!(normalize_repo("github.com/o/r").as_deref(), Some("o/r"));
        assert_eq!(normalize_repo("ghe.example.com/o/r"), None);
        assert_eq!(normalize_repo("o"), None);
    }

    #[test]
    fn urls() {
        assert_eq!(
            parse_url("https://github.com/Octo/Hello/pull/7"),
            Some((Kind::Pr, "octo/hello".into(), 7))
        );
        assert_eq!(
            parse_url("https://github.com/o/r/issues/3#issuecomment-1"),
            Some((Kind::Issue, "o/r".into(), 3))
        );
        assert_eq!(parse_url("https://github.com/o/r/pull/7/files"), None);
        assert_eq!(parse_url("https://github.com/o/r/commit/abc"), None);
        assert_eq!(parse_url("https://gitlab.com/o/r/pull/7"), None);
        assert_eq!(parse_url("https://github.com/o/r/pull/0"), None);
    }
}
