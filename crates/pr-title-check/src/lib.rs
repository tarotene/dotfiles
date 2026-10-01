//! PR タイトル(= squash merge 後の main commit subject)の文法検査。
//! 判定エンジンの単一正本(ADR-0031、docs/claude/pr-title-contract.md)。
//!
//! bash 版 `scripts/pr-title-check` の `check_title()` の移植(ADR-0024 Stage
//! 4a、#415)。client-side guard(`crates/pr-title-guard`)はこのライブラリを
//! 直接呼び、サーバ側 required check(`.github/actions/pr-title`)は同じ
//! クレートの `pr-title-check` バイナリを呼ぶ — 文法は 1 箇所にしか存在しない。
//!
//! 文法: `type(scope)?!?: subject`
//! - type    = feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert
//! - scope   = `[a-z0-9._/-]+` のカンマ区切り、省略可
//! - subject = 非空、言語自由、長さ無制限、末尾の `.` / `。` は禁止
//!
//! bash 版は `LC_ALL=C` の `grep -qE`(行単位)と `case` のリテラル一致で
//! 判定していた。バイト単位・行単位の挙動(複数行タイトルは「どれか 1 行が
//! ヘッダーに合えばよい」、subject は全体の最初の `": "` 以降)をそのまま
//! 移すため、入力は `&[u8]` で受ける。

/// 判定結果。bash 版の終了コード(0 / 1 / 2)に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 適合(exit 0)。
    Conforming,
    /// 非適合(exit 1)。
    NonConforming,
    /// 判定不能 — タイトルが空(exit 2)。
    Indeterminate,
}

impl Verdict {
    pub fn exit_code(self) -> u8 {
        match self {
            Verdict::Conforming => 0,
            Verdict::NonConforming => 1,
            Verdict::Indeterminate => 2,
        }
    }
}

const TYPES: &[&str] = &[
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// `[[:space:]]`(`LC_ALL=C`)。
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn is_scope_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'/' | b'-')
}

/// 1 行が `^(TYPE)(\(SCOPE\))?!?: [^[:space:]]` に合うか。
///
/// type 同士に接頭辞関係が無く、scope の文字クラスが `,` `)` を含まないので、
/// 正規表現のバックトラックは起きず先頭から決定的に読める。
fn line_has_header(line: &[u8]) -> bool {
    let Some(ty) = TYPES.iter().find(|t| line.starts_with(t.as_bytes())) else {
        return false;
    };
    let mut rest = &line[ty.len()..];
    if rest.first() == Some(&b'(') {
        // SCOPE = [a-z0-9._/-]+(,[a-z0-9._/-]+)*
        let mut i = 1;
        loop {
            let start = i;
            while i < rest.len() && is_scope_byte(rest[i]) {
                i += 1;
            }
            if i == start {
                return false;
            }
            if rest.get(i) == Some(&b',') {
                i += 1;
                continue;
            }
            break;
        }
        if rest.get(i) != Some(&b')') {
            return false;
        }
        rest = &rest[i + 1..];
    }
    if rest.first() == Some(&b'!') {
        rest = &rest[1..];
    }
    match rest {
        [b':', b' ', c, ..] => !is_space(*c),
        _ => false,
    }
}

/// bash の `${title#*: }`(最短一致の接頭辞除去)。`": "` が無ければ全体。
fn subject_of(title: &[u8]) -> &[u8] {
    title
        .windows(2)
        .position(|w| w == b": ")
        .map_or(title, |p| &title[p + 2..])
}

/// タイトルを判定する(`check_title`)。
pub fn check_title(title: &[u8]) -> Verdict {
    if title.is_empty() {
        return Verdict::Indeterminate;
    }
    // `grep -qE … <<< "$title"` は行単位で、どれか 1 行が合えば真。
    if !title.split(|b| *b == b'\n').any(line_has_header) {
        return Verdict::NonConforming;
    }
    let subject = subject_of(title);
    if subject.is_empty() {
        return Verdict::NonConforming;
    }
    // 末尾の "." / "。"(E3 80 82)。全角句点は正規表現の文字クラスに混ぜると
    // バイト単位で衝突しうるため、bash 版もリテラル一致で別に見ていた。
    if subject.ends_with(b".") || subject.ends_with("。".as_bytes()) {
        return Verdict::NonConforming;
    }
    Verdict::Conforming
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(t: &str) -> Verdict {
        check_title(t.as_bytes())
    }

    #[test]
    fn selftest_conforming() {
        for t in [
            "feat: 追加する",
            "fix(claude): 直す",
            "feat(alacritty,herdr): 両方直す",
            "feat!: 破壊的変更",
            "feat(api)!: 破壊的変更",
            "docs: update README",
            "revert: 署名検証の早期 return を戻す",
            "chore: x",
            "fix(v2): 直す",
        ] {
            assert_eq!(c(t), Verdict::Conforming, "{t}");
        }
    }

    #[test]
    fn selftest_non_conforming() {
        for t in [
            "PR タイトルを直す",
            "misc: 何か",
            "feat 追加する",
            "feat:",
            "feat: ",
            "feat: 追加する。",
            "feat: add feature.",
            "Feat: 追加する",
            "Revert \"feat: 追加する\"",
            "feat(Claude): 追加する",
        ] {
            assert_eq!(c(t), Verdict::NonConforming, "{t}");
        }
    }

    #[test]
    fn empty_is_indeterminate() {
        assert_eq!(c(""), Verdict::Indeterminate);
    }

    #[test]
    fn multiline_quirks_follow_bash() {
        // どれか 1 行がヘッダーに合えばよい。
        assert_eq!(c("junk\nfeat: x"), Verdict::Conforming);
        // subject は全体の最初の ": " 以降。末尾 "." は全体の末尾を見る。
        assert_eq!(c("a: b\nfeat: x."), Verdict::NonConforming);
        // 末尾改行は subject の一部(末尾が "." でなければ適合)。
        assert_eq!(c("feat: x\n"), Verdict::Conforming);
        assert_eq!(c("feat: x.\n"), Verdict::Conforming);
    }

    #[test]
    fn malformed_scope() {
        for t in [
            "feat(): x",
            "feat(a,): x",
            "feat(a b): x",
            "feat(a: x",
            "feat(a)(b): x",
            "feat!(a): x",
            "feat:  x",
            "feat:\tx",
        ] {
            assert_eq!(c(t), Verdict::NonConforming, "{t}");
        }
    }
}
