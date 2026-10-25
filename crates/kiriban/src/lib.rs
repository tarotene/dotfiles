//! kiriban — Issue / PR 番号のキリ番の判定と、祝福の文面(docs/kiriban.md)。
//!
//! 規則と文面の単一正本はこのファイル。`.github/actions/kiriban` がこの crate を
//! build して呼び、各リポジトリの workflow は規則も文面も持たない。依存ゼロ。
//! 時刻には依存しない(期限の日付は呼び出し側が計算して渡す)。
//!
//! どの文面にも `@` を書かない。通知は取り消せないので、踏んだ人はメンションでなく
//! ログイン名の文字列で書く。

use std::fmt::Write as _;

/// キリ番の種類。`classify` はこの宣言順(= 見出しの優先順)で返す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// 1000 の倍数。
    Grand,
    /// 111 以上で、全桁が同じ数字。
    Repdigit,
    /// 100 の倍数(Grand を除く)。
    Hundred,
    /// 1000 の倍数の 1 つ前(999, 1999, …)。
    Before,
    /// 1000 の倍数の 1 つ後(1001, 2001, …)。
    After,
}

impl Class {
    /// CLI の出力に使う安定した名前。
    pub fn name(self) -> &'static str {
        match self {
            Class::Grand => "grand",
            Class::Repdigit => "repdigit",
            Class::Hundred => "hundred",
            Class::Before => "before",
            Class::After => "after",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Class::Grand => "大キリ番",
            Class::Repdigit => "ゾロ目キリ番",
            Class::Hundred => "キリ番",
            Class::Before | Class::After => "前後賞",
        }
    }
}

fn is_repdigit(n: u64) -> bool {
    if n < 111 {
        return false;
    }
    let s = n.to_string();
    let first = s.as_bytes()[0];
    s.bytes().all(|b| b == first)
}

/// `n` が当たるキリ番の種類(優先順)。空ならキリ番ではない。
pub fn classify(n: u64) -> Vec<Class> {
    let mut v = Vec::new();
    if n > 0 && n.is_multiple_of(1000) {
        v.push(Class::Grand);
    }
    if is_repdigit(n) {
        v.push(Class::Repdigit);
    }
    if n > 0 && n.is_multiple_of(100) && !n.is_multiple_of(1000) {
        v.push(Class::Hundred);
    }
    if n >= 999 && (n + 1).is_multiple_of(1000) {
        v.push(Class::Before);
    }
    if n >= 1001 && (n - 1).is_multiple_of(1000) {
        v.push(Class::After);
    }
    v
}

/// キリリク受付 Issue を開くか(大キリ番とゾロ目だけ)。
pub fn opens_request(classes: &[Class]) -> bool {
    classes
        .iter()
        .any(|c| matches!(c, Class::Grand | Class::Repdigit))
}

/// 踏んだのが bot か(`user.type == "Bot"` か、ログイン名の末尾が `[bot]`)。
pub fn is_bot(login: &str, user_type: &str) -> bool {
    user_type == "Bot" || login.ends_with("[bot]")
}

/// ログイン名・owner・repo に使える文字か(`@` や空白を文面に入れない)。
pub fn is_safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'[' | b']' | b'/')
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Issue,
    Pr,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "issue" => Some(Kind::Issue),
            "pr" => Some(Kind::Pr),
            _ => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Kind::Issue => "Issue",
            Kind::Pr => "PR",
        }
    }
}

/// 文面に必要な、踏まれた側の事実。
pub struct Ctx<'a> {
    pub number: u64,
    pub kind: Kind,
    pub getter: &'a str,
    pub bot: bool,
    /// リポジトリの持ち主(管理人)。
    pub owner: &'a str,
    /// `owner/repo`。
    pub nwo: &'a str,
}

/// 同じ run で開いたキリリク受付 Issue。
pub struct Request<'a> {
    pub number: u64,
    /// `YYYY-MM-DD`。
    pub expires: &'a str,
}

/// 冪等のため、祝福コメントの先頭に埋める印。
pub fn comment_marker(n: u64) -> String {
    format!("<!-- kiriban:{n} -->")
}

/// 冪等のため、キリリク受付 Issue の本文の先頭に埋める印。
pub fn request_marker(n: u64) -> String {
    format!("<!-- kiriban-request:{n} -->")
}

/// 作るラベル: (名前, 色, 説明)。`kiriban` が殿堂、`kiririku` が受付 Issue の印。
pub const LABELS: [(&str, &str, &str); 2] = [
    (
        "kiriban",
        "fbca04",
        "キリ番を踏んだ Issue / PR(殿堂は label:kiriban の検索)",
    ),
    (
        "kiririku",
        "d4c5f9",
        "キリリク受付 Issue(kiriban workflow が自動で開く)",
    ),
];

const DOCS_URL: &str = "https://github.com/tarotene/dotfiles/blob/main/docs/kiriban.md";

/// 検査: `YYYY-MM-DD` の形か(日付として正しいかは見ない)。
pub fn is_date_shape(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

fn decoration(head: Class) -> (&'static str, &'static str) {
    if head == Class::Grand {
        ("☆彡☆彡", "☆彡☆彡")
    } else {
        ("☆彡", "☆彡")
    }
}

/// 前後賞の対象になる大キリ番。
fn zenkosho_target(n: u64, c: Class) -> u64 {
    if c == Class::Before {
        n + 1
    } else {
        n - 1
    }
}

/// 祝福コメント。キリ番でない(`classes` が空)なら `None`。
/// `request` が `Some` のときだけキリリクの案内を書く。
pub fn render_comment(ctx: &Ctx, classes: &[Class], request: Option<&Request>) -> Option<String> {
    let head = *classes.first()?;
    let n = ctx.number;
    let (l, r) = decoration(head);
    let mut s = String::new();
    let _ = writeln!(s, "{}", comment_marker(n));
    let _ = writeln!(s, "{l} {} #{n} {r}", head.label());
    s.push('\n');

    if ctx.bot {
        let _ = writeln!(
            s,
            "{} さんが {} #{n} を踏みました……が、どうやら踏み逃げのようです。",
            ctx.getter,
            ctx.kind.word()
        );
    } else {
        let _ = writeln!(s, "おめでとうございます!!");
        let _ = writeln!(s, "このリポジトリの #{n} を踏んだのは");
        let _ = writeln!(s, " {} さん です!!", ctx.getter);
    }

    for c in classes {
        if !matches!(c, Class::Before | Class::After) {
            continue;
        }
        let t = zenkosho_target(n, *c);
        let side = if *c == Class::Before {
            "前賞"
        } else {
            "後賞"
        };
        s.push('\n');
        if head == *c {
            let _ = writeln!(
                s,
                "大キリ番 #{t} の{side}です。惜しくも大キリ番は逃しましたが、これも何かのご縁!"
            );
        } else {
            let _ = writeln!(s, "さらに大キリ番 #{t} の{side}でもあります。惜しい!");
        }
    }

    s.push('\n');
    match (ctx.bot, request) {
        (false, Some(q)) => {
            let _ = writeln!(s, "踏み逃げは禁止です(笑)");
            let _ = writeln!(s, "キリリクを受け付けています → #{}", q.number);
            let _ = writeln!(s, "(有効期限: {})", q.expires);
        }
        (true, Some(q)) => {
            let _ = writeln!(
                s,
                "キリリク権は管理人({})預かりとなります → #{}",
                ctx.owner, q.number
            );
            let _ = writeln!(s, "(有効期限: {})", q.expires);
        }
        (false, None) => {
            let _ = writeln!(
                s,
                "記念に一言カキコしていってください。踏み逃げは禁止です(笑)"
            );
        }
        (true, None) => {}
    }

    s.push('\n');
    let _ = writeln!(s, "―― 管理人より");
    s.push('\n');
    let _ = writeln!(
        s,
        "<sub>kiriban workflow による自動カキコです。キリ番の殿堂 → \
         https://github.com/{}/issues?q=label%3Akiriban</sub>",
        ctx.nwo
    );
    Some(s)
}

/// キリリク受付 Issue の題名。キリリクを開かない番号なら `None`。
pub fn request_title(n: u64, classes: &[Class]) -> Option<String> {
    if !opens_request(classes) {
        return None;
    }
    Some(format!("キリリク受付: #{n}({})", classes[0].label()))
}

/// キリリク受付 Issue の本文。キリリクを開かない番号なら `None`。
pub fn render_request_body(ctx: &Ctx, classes: &[Class], expires: &str) -> Option<String> {
    if !opens_request(classes) {
        return None;
    }
    let n = ctx.number;
    let label = classes[0].label();
    let getter = if ctx.bot {
        format!(
            "{}(踏み逃げ)— 権利は管理人 {} 預かり",
            ctx.getter, ctx.owner
        )
    } else {
        ctx.getter.to_string()
    };
    let mut s = String::new();
    let _ = writeln!(s, "{}", request_marker(n));
    let _ = writeln!(s, "## キリリク受付");
    s.push('\n');
    let _ = writeln!(
        s,
        "{label} #{n} のキリリク(キリ番リクエスト)を受け付けます。"
    );
    if ctx.bot {
        let _ = writeln!(
            s,
            "踏んだ {} さんは踏み逃げでしたので、権利は管理人 {} が預かります。",
            ctx.getter, ctx.owner
        );
    }
    s.push('\n');
    let _ = writeln!(s, "| 項目 | 内容 |");
    let _ = writeln!(s, "|---|---|");
    let _ = writeln!(s, "| キリ番 | #{n}({label}、{}) |", ctx.kind.word());
    let _ = writeln!(s, "| ゲッター | {getter} |");
    let _ = writeln!(s, "| 受付期限 | {expires} |");
    s.push('\n');
    let _ = writeln!(s, "### リクエスト内容");
    s.push('\n');
    let _ = writeln!(s, "(この Issue へのコメントで、リクエストをお寄せください)");
    s.push('\n');
    let _ = writeln!(s, "---");
    let _ = writeln!(
        s,
        "- 期限を過ぎたら管理人が手で閉じます(自動では閉じません)。"
    );
    let _ = writeln!(
        s,
        "- この Issue は kiriban workflow が自動で開きました([仕組み]({DOCS_URL}))。"
    );
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: u64) -> Vec<&'static str> {
        classify(n).into_iter().map(Class::name).collect()
    }

    #[test]
    fn classify_boundaries() {
        let none: Vec<&str> = vec![];
        for n in [
            0, 1, 11, 99, 101, 110, 112, 998, 1002, 1099, 1101, 1234, 9998,
        ] {
            assert_eq!(names(n), none, "{n}");
        }
        assert_eq!(names(100), ["hundred"]);
        assert_eq!(names(111), ["repdigit"]);
        assert_eq!(names(222), ["repdigit"]);
        assert_eq!(names(999), ["repdigit", "before"]);
        assert_eq!(names(1000), ["grand"]);
        assert_eq!(names(1001), ["after"]);
        assert_eq!(names(1100), ["hundred"]);
        assert_eq!(names(1111), ["repdigit"]);
        assert_eq!(names(1999), ["before"]);
        assert_eq!(names(2000), ["grand"]);
        assert_eq!(names(2001), ["after"]);
        assert_eq!(names(9999), ["repdigit", "before"]);
        assert_eq!(names(10000), ["grand"]);
        assert_eq!(names(10001), ["after"]);
        assert_eq!(names(11111), ["repdigit"]);
    }

    #[test]
    fn two_digit_repdigits_are_not_kiriban() {
        for n in [11, 22, 33, 44, 55, 66, 77, 88, 99] {
            assert!(classify(n).is_empty(), "{n}");
        }
    }

    #[test]
    fn request_opens_only_for_grand_and_repdigit() {
        assert!(opens_request(&classify(1000)));
        assert!(opens_request(&classify(999)));
        assert!(opens_request(&classify(222)));
        assert!(!opens_request(&classify(100)));
        assert!(!opens_request(&classify(1100)));
        assert!(!opens_request(&classify(1001)));
        assert!(!opens_request(&classify(1999)));
        assert!(!opens_request(&classify(1234)));
    }

    #[test]
    fn bot_detection() {
        assert!(is_bot("renovate[bot]", "Bot"));
        assert!(is_bot("renovate[bot]", "User"));
        assert!(is_bot("someone", "Bot"));
        assert!(!is_bot("tarotene", "User"));
    }

    fn ctx<'a>(n: u64, getter: &'a str, bot: bool) -> Ctx<'a> {
        Ctx {
            number: n,
            kind: Kind::Pr,
            getter,
            bot,
            owner: "tarotene",
            nwo: "tarotene/dotfiles",
        }
    }

    #[test]
    fn grand_comment_for_a_human_matches_the_sample() {
        let c = classify(1000);
        let q = Request {
            number: 1005,
            expires: "2026-11-24",
        };
        let s = render_comment(&ctx(1000, "tarotene", false), &c, Some(&q)).unwrap();
        assert!(s.starts_with("<!-- kiriban:1000 -->\n☆彡☆彡 大キリ番 #1000 ☆彡☆彡\n\n"));
        assert!(s.contains("このリポジトリの #1000 を踏んだのは\n tarotene さん です!!\n"));
        assert!(s.contains(
            "踏み逃げは禁止です(笑)\nキリリクを受け付けています → #1005\n(有効期限: 2026-11-24)\n"
        ));
        assert!(s.contains("―― 管理人より"));
        assert!(s.contains("issues?q=label%3Akiriban"));
    }

    #[test]
    fn bot_comment_holds_the_request_for_the_owner() {
        let c = classify(1000);
        let q = Request {
            number: 1005,
            expires: "2026-11-24",
        };
        let s = render_comment(&ctx(1000, "renovate[bot]", true), &c, Some(&q)).unwrap();
        assert!(s.contains(
            "renovate[bot] さんが PR #1000 を踏みました……が、どうやら踏み逃げのようです。"
        ));
        assert!(s.contains("キリリク権は管理人(tarotene)預かりとなります → #1005"));
        assert!(!s.contains("おめでとうございます"));
    }

    #[test]
    fn nested_comment_has_no_request_lines() {
        let c = classify(1111);
        let s = render_comment(&ctx(1111, "github-actions[bot]", true), &c, None).unwrap();
        assert!(!s.contains("キリリク"));
        assert!(s.contains("踏み逃げ"));
    }

    #[test]
    fn repdigit_that_is_also_zenkosho_mentions_both() {
        let c = classify(999);
        let q = Request {
            number: 1005,
            expires: "2026-11-24",
        };
        let s = render_comment(&ctx(999, "tarotene", false), &c, Some(&q)).unwrap();
        assert!(s.contains("ゾロ目キリ番 #999"));
        assert!(s.contains("さらに大キリ番 #1000 の前賞でもあります"));
    }

    #[test]
    fn zenkosho_only_comment() {
        let c = classify(1001);
        let s = render_comment(&ctx(1001, "tarotene", false), &c, None).unwrap();
        assert!(s.contains("前後賞 #1001"));
        assert!(s.contains("大キリ番 #1000 の後賞です"));
        assert!(!s.contains("キリリクを受け付けています"));
    }

    #[test]
    fn not_kiriban_renders_nothing() {
        let c = classify(1234);
        assert!(render_comment(&ctx(1234, "tarotene", false), &c, None).is_none());
        assert!(request_title(1234, &c).is_none());
        assert!(render_request_body(&ctx(1234, "tarotene", false), &c, "2026-11-24").is_none());
    }

    #[test]
    fn request_title_and_body() {
        let c = classify(1000);
        assert_eq!(
            request_title(1000, &c).unwrap(),
            "キリリク受付: #1000(大キリ番)"
        );
        let b = render_request_body(&ctx(1000, "tarotene", false), &c, "2026-11-24").unwrap();
        assert!(b.starts_with("<!-- kiriban-request:1000 -->\n## キリリク受付\n"));
        assert!(b.contains("| ゲッター | tarotene |"));
        assert!(b.contains("| 受付期限 | 2026-11-24 |"));
        let b = render_request_body(&ctx(1000, "renovate[bot]", true), &c, "2026-11-24").unwrap();
        assert!(b.contains("renovate[bot](踏み逃げ)— 権利は管理人 tarotene 預かり"));
    }

    #[test]
    fn no_at_sign_and_marker_everywhere() {
        let q = Request {
            number: 99999,
            expires: "2099-12-31",
        };
        for n in 1..=20000u64 {
            let c = classify(n);
            if c.is_empty() {
                continue;
            }
            for (getter, bot) in [("tarotene", false), ("renovate[bot]", true)] {
                let cx = ctx(n, getter, bot);
                for req in [None, Some(&q)] {
                    let s = render_comment(&cx, &c, req).unwrap();
                    assert!(!s.contains('@'), "{n}");
                    assert!(s.starts_with(&comment_marker(n)), "{n}");
                }
                if let Some(b) = render_request_body(&cx, &c, "2099-12-31") {
                    assert!(!b.contains('@'), "{n}");
                    assert!(b.starts_with(&request_marker(n)), "{n}");
                }
            }
        }
    }

    #[test]
    fn output_is_deterministic() {
        let c = classify(1000);
        let cx = ctx(1000, "tarotene", false);
        assert_eq!(render_comment(&cx, &c, None), render_comment(&cx, &c, None));
    }

    #[test]
    fn safe_names_and_dates() {
        assert!(is_safe_name("tarotene"));
        assert!(is_safe_name("renovate[bot]"));
        assert!(is_safe_name("tarotene/dotfiles"));
        assert!(!is_safe_name(""));
        assert!(!is_safe_name("@tarotene"));
        assert!(!is_safe_name("a b"));
        assert!(is_date_shape("2026-11-24"));
        assert!(!is_date_shape("2026-1-24"));
        assert!(!is_date_shape("20261124"));
        assert!(!is_date_shape("2026-11-2x"));
    }
}
