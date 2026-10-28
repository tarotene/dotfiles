//! GitHub の PR・Issue の URL の認識(単一正本)。
//!
//! `gh-edit-allow`(自分が作った PR・Issue だけ編集を通す)と `codex-plan`(PR の作成を
//! tool の出力から読む)が同じ規則を使う。規則は `https://github.com/<o>/<r>/(pull|issues)/<N>`
//! だけで、`#`・`?` 以降と末尾の `/` は無視する。それ以外(`/files`・`/commit/…`・他のホスト・
//! 番号が 0 や符号つき)は認識しない。

/// `gh <kind> <verb>` の kind、および URL の `pull` / `issues`。
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

/// `https://github.com/<o>/<r>/(pull|issues)/<N>` を (kind, 小文字の owner/repo, N) に。
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

/// 正の整数(符号・空白・先頭の `+` を許さない)。
pub fn parse_number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| *n > 0)
}

/// `u` が PR の URL か。
pub fn is_pr_url(u: &str) -> bool {
    matches!(parse_url(u), Some((Kind::Pr, _, _)))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn pr_url_rule() {
        // 移す前に codex-plan が持っていた複写のテストの入力（Issue は PR ではない）。
        for (url, pr) in [
            ("https://github.com/Octo/Hello/pull/7", true),
            ("https://github.com/o/r/pull/7#issuecomment-1", true),
            ("https://github.com/o/r/pull/7/", true),
            ("https://github.com/o/r/issues/3#issuecomment-1", false),
            ("https://github.com/o/r/pull/7/files", false),
            ("https://github.com/o/r/commit/abc", false),
            ("https://gitlab.com/o/r/pull/7", false),
            ("https://github.com/o/r/pull/0", false),
            ("https://github.com//r/pull/7", false),
            ("https://github.com/o/r/pull/+7", false),
            ("", false),
        ] {
            assert_eq!(is_pr_url(url), pr, "{url}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number("12"), Some(12));
        for s in ["", "0", "+7", "-1", "1 ", "x"] {
            assert_eq!(parse_number(s), None, "{s:?}");
        }
    }
}
