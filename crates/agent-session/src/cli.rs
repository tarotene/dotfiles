//! 手動モード(`--check`)の形。guard・gate を、hook でなく手で叩くときの、出力と終了コードを 1 つにそろえる。
//!
//! | 結果 | 標準出力 | 標準エラー | 終了コード |
//! |---|---|---|---|
//! | 通す | `pass` | | 0 |
//! | 拒否 | `deny: <理由>`(理由が複数行ならそのまま続ける) | | 1 |
//! | 使い方の誤り(引数の過不足、読めない入力、判定できない) | | `<名前>: <メッセージ>` と `usage: <使い方>` | 2 |
//!
//! hook として呼ばれたときの判定は JSON で、終了コードは 0 のままである(1 と 2 は手動モードだけの意味)。
//! hook として登録された bin の `--agent` の誤りは、`entry::enter` が終了コード 1 にする。hook で終了コード 2 は
//! 「止める」の意味なので、使わない。

use std::io::Write;
use std::process::ExitCode;

/// 通す。
pub const PASS: u8 = 0;
/// 拒否する。
pub const DENY: u8 = 1;
/// 使い方の誤り。
pub const USAGE: u8 = 2;

/// 手動モードの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Deny(String),
    Usage(String),
}

impl From<Option<String>> for Verdict {
    /// 検査の理由があれば拒否、なければ通す。
    fn from(reason: Option<String>) -> Verdict {
        match reason {
            Some(r) => Verdict::Deny(r),
            None => Verdict::Pass,
        }
    }
}

/// `--check` の後ろの引数。`--check` が無ければ `None`(hook モード)。位置は問わない。
pub fn manual(rest: &[String]) -> Option<&[String]> {
    let i = rest.iter().position(|a| a == "--check")?;
    Some(&rest[i + 1..])
}

/// 手動モードの引数を、必須 `required` 個と任意 `optional` 個に割る。過不足は使い方の誤り。
/// 戻りは、与えられた分(任意が省かれたら短い)。
pub fn args(a: &[String], required: usize, optional: usize) -> Result<Vec<&str>, Verdict> {
    if a.len() < required || a.len() > required + optional {
        return Err(Verdict::Usage(format!(
            "引数は {required} 個{}",
            if optional > 0 {
                format!("(任意で {optional} 個まで追加)")
            } else {
                String::new()
            }
        )));
    }
    Ok(a.iter().map(String::as_str).collect())
}

/// 結果を、(標準出力, 標準エラー, 終了コード)に描く。純粋関数。
pub fn render(name: &str, usage: &str, v: &Verdict) -> (String, String, u8) {
    match v {
        Verdict::Pass => ("pass\n".to_string(), String::new(), PASS),
        Verdict::Deny(reason) => (format!("deny: {reason}\n"), String::new(), DENY),
        Verdict::Usage(message) => (
            String::new(),
            format!("{name}: {message}\nusage: {usage}\n"),
            USAGE,
        ),
    }
}

/// 結果を書いて、終了コードを返す。
pub fn finish(name: &str, usage: &str, v: Verdict) -> ExitCode {
    let (out, err, code) = render(name, usage, &v);
    let _ = std::io::stdout().write_all(out.as_bytes());
    let _ = std::io::stderr().write_all(err.as_bytes());
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn the_three_results_render_to_the_agreed_shape() {
        assert_eq!(
            render("g", "g --check <cmd>", &Verdict::Pass),
            ("pass\n".into(), String::new(), 0)
        );
        assert_eq!(
            render(
                "g",
                "g --check <cmd>",
                &Verdict::Deny("理由\n2 行目".into())
            ),
            ("deny: 理由\n2 行目\n".into(), String::new(), 1)
        );
        assert_eq!(
            render(
                "g",
                "g --check <cmd>",
                &Verdict::Usage("引数が足りない".into())
            ),
            (
                String::new(),
                "g: 引数が足りない\nusage: g --check <cmd>\n".into(),
                2
            )
        );
    }

    #[test]
    fn a_reason_decides_between_deny_and_pass() {
        assert_eq!(Verdict::from(None), Verdict::Pass);
        assert_eq!(Verdict::from(Some("r".into())), Verdict::Deny("r".into()));
    }

    #[test]
    fn the_check_flag_is_found_anywhere_and_its_arguments_follow() {
        assert_eq!(
            manual(&s(&["--check", "a", "b"])),
            Some(&s(&["a", "b"])[..])
        );
        assert_eq!(manual(&s(&["x", "--check", "a"])), Some(&s(&["a"])[..]));
        assert_eq!(manual(&s(&["pre"])), None);
        assert_eq!(manual(&[]), None);
    }

    #[test]
    fn too_few_or_too_many_arguments_are_a_usage_error() {
        assert!(matches!(args(&s(&[]), 1, 1), Err(Verdict::Usage(_))));
        assert!(matches!(
            args(&s(&["a", "b", "c"]), 1, 1),
            Err(Verdict::Usage(_))
        ));
        assert_eq!(args(&s(&["a"]), 1, 1), Ok(vec!["a"]));
        assert_eq!(args(&s(&["a", "b"]), 1, 1), Ok(vec!["a", "b"]));
    }

    #[test]
    fn exit_codes_never_collide_with_the_block_meaning() {
        // 手動モードの 2 は使い方の誤り。hook の bin では使わない(entry::enter は 1)。
        assert_eq!((PASS, DENY, USAGE), (0, 1, 2));
    }
}
