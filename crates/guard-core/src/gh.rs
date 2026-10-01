//! 1 範囲(1 つの `gh …` 呼び出し)のトークン列からフラグを抜き出す。
//!
//! 吸収元:
//! - 本文: attribution-guard.sh `decide_tokens` 前半 / pr-confirm-guard.sh
//!   `extract_body` / feedback-target-guard.sh `decide_tokens` /
//!   stack-base-guard.sh `parse_pr_tokens` の body 部分(4 箇所の同一式)
//! - `gh api`: attribution-guard.sh `decide_api_tokens` /
//!   repo-create-guard.sh `decide_api_tokens`
//! - 値フラグ: pr-title-guard.sh `parse_pr_title_tokens` /
//!   decision-colocation-guard.sh `parse_tokens`
//!
//! 本文の値だけを取り出すのは、`--title "🤖 Generated with …" --body "x"` の
//! ようにほかのフラグのマーカーで通ってしまうのを防ぐため(bash 版の
//! 「本文の抽出」の節)。

use crate::shell::is_posix_space;
use serde_json::Value;
use std::path::Path;

/// 範囲内のどこかに heredoc リダイレクト(`<<`)があるか。
///
/// `--body "$(cat <<'TAG' … TAG)"` のように値トークンの内側に来ることが
/// あるので、独立トークンではなく部分文字列で見る(独立トークンだけを
/// 見る実装では pass に倒れていた、bash 版 selftest 21 の実測)。
pub fn has_heredoc(tokens: &[String]) -> bool {
    tokens.iter().any(|t| t.contains("<<"))
}

/// `$(cat -- "$p")` 相当: 通常ファイルで読めれば中身(末尾の改行をすべて
/// 落とす — コマンド置換の規則)。bash の `[[ -f $p && -r $p ]]` が偽なら
/// `None`。
pub fn read_body_file(p: &str) -> Option<String> {
    if p.is_empty() {
        return None;
    }
    let path = Path::new(p);
    if !path.metadata().ok()?.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(command_substitution(&String::from_utf8_lossy(&bytes)))
}

/// bash の `$(…)` が値に施す変形: NUL を落とし、末尾の改行をすべて落とす。
pub fn command_substitution(s: &str) -> String {
    let s: String = s.chars().filter(|&c| c != '\0').collect();
    s.trim_end_matches('\n').to_string()
}

/// `gh pr|issue create|edit|comment|review` の本文フラグ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BodyFlags {
    /// `--body` / `-b` / `--body=` / `--body-file` / `-F` / `--body-file=` の
    /// いずれかがあった(値が取れたかどうかは問わない)。
    pub have_flag: bool,
    /// 取れた本文の値(出現順)。`--body-file` は読めたものだけ。
    pub texts: Vec<String>,
}

impl BodyFlags {
    /// bash の本文抽出ループ(4 guard で同一式)。値を取るフラグだけが値
    /// トークンを消費し、それ以外のトークンは読み飛ばす。
    pub fn scan(tokens: &[String]) -> Self {
        let n = tokens.len();
        let mut out = BodyFlags::default();
        let mut i = 0;
        while i < n {
            let t = tokens[i].as_str();
            match t {
                "--body" | "-b" => {
                    out.have_flag = true;
                    if i + 1 < n {
                        out.texts.push(tokens[i + 1].clone());
                        i += 2;
                        continue;
                    }
                }
                "--body-file" | "-F" => {
                    out.have_flag = true;
                    if i + 1 < n {
                        if let Some(s) = read_body_file(&tokens[i + 1]) {
                            out.texts.push(s);
                        }
                        i += 2;
                        continue;
                    }
                }
                _ => {
                    if let Some(v) = t.strip_prefix("--body=") {
                        out.have_flag = true;
                        out.texts.push(v.to_string());
                    } else if let Some(p) = t.strip_prefix("--body-file=") {
                        out.have_flag = true;
                        if let Some(s) = read_body_file(p) {
                            out.texts.push(s);
                        }
                    }
                }
            }
            i += 1;
        }
        out
    }
}

/// 抽出した本文の値を 1 つの判定対象テキストにまとめる(bash の
/// `text="$(printf '%s\n' "${texts[@]}")"` 以降)。判定不能なら `None`:
/// - heredoc を使う範囲なら heredoc 本体を本文候補に加える
/// - そうでなく本文がコマンド置換(`$(` / バッククォート)なら中身が不明 →
///   `None`(`--body "$(cat body.md)"` を常に弾かないため)
/// - 空白しか残らなければ `None`
pub fn assemble_body_text(texts: &[String], has_hd: bool, heredoc_bodies: &str) -> Option<String> {
    let mut text = command_substitution(&texts.join("\n"));
    if has_hd && !heredoc_bodies.is_empty() {
        text.push('\n');
        text.push_str(heredoc_bodies);
    } else if text.contains("$(") || text.contains('`') {
        return None;
    }
    if text.bytes().all(is_posix_space) {
        return None;
    }
    Some(text)
}

/// 本文フラグの値を取り出して判定対象テキストにする(pr-confirm-guard.sh の
/// `extract_body` と同じ)。本文フラグが無い・値が取れない・コマンド置換で
/// 中身が不明なら `None`(判定不能 → 通す)。
pub fn extract_body(tokens: &[String], heredoc_bodies: &str) -> Option<String> {
    let flags = BodyFlags::scan(tokens);
    if !flags.have_flag {
        return None;
    }
    assemble_body_text(&flags.texts, has_heredoc(tokens), heredoc_bodies)
}

/// 値フラグ 1 種の仕様(例: `--base` / `-B` / `--base=`)。
#[derive(Debug, Clone, Copy)]
pub struct ValueFlag<'a> {
    /// 次のトークンを値に取る綴り(`["--base", "-B"]`)。
    pub names: &'a [&'a str],
    /// `=` 付きの綴りの接頭辞(`"--base="`)。
    pub eq_prefix: &'a str,
}

/// [`scan_value_flags`] の 1 フラグぶんの結果(bash の `F_HAS_X` / `F_X`)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlagValue {
    /// フラグが現れた(末尾にあって値が無い場合も真)。
    pub present: bool,
    /// 最後に現れた値(bash は後勝ちで上書きする)。値が無ければ空文字。
    pub value: String,
}

/// 値フラグを 1 パスで抜き出す(pr-title-guard.sh `parse_pr_title_tokens` /
/// decision-colocation-guard.sh `parse_tokens` と同じループ)。
///
/// 1 パスで全フラグを見るのは、`--title -R` のように値トークンが別フラグの
/// 綴りを持つ場合に bash と同じ結果にするため(フラグごとに独立に走査すると
/// `-R` を `--repo` と誤読する)。`specs` に無いトークンは値を消費せず
/// 読み飛ばす。
pub fn scan_value_flags(tokens: &[String], specs: &[ValueFlag<'_>]) -> Vec<FlagValue> {
    let n = tokens.len();
    let mut out = vec![FlagValue::default(); specs.len()];
    let mut i = 0;
    'outer: while i < n {
        let t = tokens[i].as_str();
        for (k, spec) in specs.iter().enumerate() {
            if spec.names.contains(&t) {
                out[k].present = true;
                if i + 1 < n {
                    out[k].value = tokens[i + 1].clone();
                    i += 2;
                    continue 'outer;
                }
                break;
            }
            if let Some(v) = t.strip_prefix(spec.eq_prefix) {
                out[k].present = true;
                out[k].value = v.to_string();
                break;
            }
        }
        i += 1;
    }
    out
}

/// `gh api` の `-X` / `--method` / `--method=` の最後の値(bash は後勝ち)。
/// 大文字化はしない(呼び出し側が `to_ascii_uppercase` で比べる — bash の
/// `${method^^}`)。
pub fn api_method(tokens: &[String]) -> Option<String> {
    let n = tokens.len();
    let mut method = None;
    for i in 0..n {
        let t = tokens[i].as_str();
        if t == "-X" || t == "--method" {
            if i + 1 < n {
                method = Some(tokens[i + 1].clone());
            }
        } else if let Some(v) = t.strip_prefix("--method=") {
            method = Some(v.to_string());
        }
    }
    method
}

/// `gh api` の範囲から、`is_judged` に一致する最初のパスを返す。各トークンを
/// 先頭 `/` 1 つを外してから `/` を付け直した形(bash の `t="/${tok#/}"`)で
/// 判定する — `repos/o/r/…` と `/repos/o/r/…` を同一視するため。
pub fn api_path(tokens: &[String], mut is_judged: impl FnMut(&str) -> bool) -> Option<String> {
    tokens.iter().find_map(|tok| {
        let t = format!("/{}", tok.strip_prefix('/').unwrap_or(tok));
        is_judged(&t).then_some(t)
    })
}

/// `gh api` の本文(`-f`/`--field`/`-F`/`--raw-field` の `body=<値>`、または
/// `--input <file>` の `.body`)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiBody {
    /// 本文フラグがあった(`--input -` も真 — 値は取れない)。
    pub have_flag: bool,
    pub texts: Vec<String>,
}

impl ApiBody {
    /// bash の `decide_api_tokens` 後半のループ。
    pub fn scan(tokens: &[String]) -> Self {
        let n = tokens.len();
        let mut out = ApiBody::default();
        let mut i = 0;
        while i < n {
            let t = tokens[i].as_str();
            match t {
                "-f" | "--field" | "-F" | "--raw-field" => {
                    if i + 1 < n {
                        if let Some(v) = tokens[i + 1].strip_prefix("body=") {
                            out.have_flag = true;
                            out.texts.push(v.to_string());
                            i += 2;
                            continue;
                        }
                    }
                }
                "--input" => {
                    if i + 1 < n {
                        out.have_flag = true;
                        if let Some(b) = input_file_body(&tokens[i + 1]) {
                            out.texts.push(b);
                        }
                        i += 2;
                        continue;
                    }
                }
                _ => {
                    if t.starts_with("--field=body=") || t.starts_with("--raw-field=body=") {
                        out.have_flag = true;
                        // bash の `${tok#*=body=}`(最短一致の接頭辞を落とす)
                        let at = t.find("=body=").map_or(0, |p| p + "=body=".len());
                        out.texts.push(t[at..].to_string());
                    } else if let Some(p) = t.strip_prefix("--input=") {
                        out.have_flag = true;
                        if let Some(b) = input_file_body(p) {
                            out.texts.push(b);
                        }
                    }
                }
            }
            i += 1;
        }
        out
    }
}

/// `--input <file>` の `.body`(bash の `jq -r '.body // empty' -- "$p"`)。
/// `-`(stdin)・読めないファイル・jq がエラーになる内容・空の本文は `None`。
fn input_file_body(p: &str) -> Option<String> {
    if p == "-" || p.is_empty() {
        return None;
    }
    let path = Path::new(p);
    if !path.metadata().ok()?.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let src = String::from_utf8_lossy(&bytes);
    // jq は複数の JSON 値を順に処理し、途中で 1 つでもエラーになれば非 0 で
    // 終わる(`|| body=""` で全体が空になる)。
    let mut outs = Vec::new();
    for v in serde_json::Deserializer::from_str(&src).into_iter::<Value>() {
        let v = v.ok()?;
        match v {
            Value::Null => {}
            Value::Object(ref m) => {
                if let Some(s) = jq_raw(m.get("body").unwrap_or(&Value::Null)) {
                    outs.push(s);
                }
            }
            _ => return None,
        }
    }
    let body = command_substitution(&outs.join("\n"));
    (!body.is_empty()).then_some(body)
}

/// `jq -r 'X // empty'` が値 X に対して出す文字列。`null` / `false` は
/// `None`(`//` の代替に落ちる)。
pub fn jq_raw(v: &Value) -> Option<String> {
    match v {
        Value::Null | Value::Bool(false) => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("true".into()),
        Value::Number(n) => Some(n.to_string()),
        other => serde_json::to_string_pretty(other).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn body_flags_consume_only_their_values() {
        let f = BodyFlags::scan(&v(&["gh", "pr", "create", "--title", "--body", "x", "-b"]));
        // `--title` は値を消費しないので `--body x` が本文になる(bash と同じ)
        assert!(f.have_flag);
        assert_eq!(f.texts, ["x"]);
        let f = BodyFlags::scan(&v(&["--body=a", "--body-file=/nonexistent/x"]));
        assert!(f.have_flag);
        assert_eq!(f.texts, ["a"]);
        assert!(!BodyFlags::scan(&v(&["--add-label", "bug"])).have_flag);
    }

    #[test]
    fn body_file_strips_trailing_newlines() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("b.md");
        std::fs::write(&p, "a\n\nb\n\n\n").unwrap();
        assert_eq!(read_body_file(p.to_str().unwrap()), Some("a\n\nb".into()));
        assert_eq!(read_body_file(d.path().to_str().unwrap()), None);
        assert_eq!(read_body_file(""), None);
    }

    #[test]
    fn assemble_rules() {
        assert_eq!(
            assemble_body_text(&v(&["a\n", "b\n\n"]), false, ""),
            Some("a\n\nb".into())
        );
        assert_eq!(assemble_body_text(&v(&["$(cat x)"]), false, ""), None);
        assert_eq!(assemble_body_text(&v(&["`x`"]), false, ""), None);
        // heredoc 併用時はコマンド置換でも本体を見る
        assert_eq!(
            assemble_body_text(&v(&["$(cat <<'E'"]), true, "body\n"),
            Some("$(cat <<'E'\nbody\n".into())
        );
        assert_eq!(assemble_body_text(&v(&[" \t"]), false, ""), None);
        assert_eq!(assemble_body_text(&[], true, ""), None);
    }

    #[test]
    fn value_flags_single_pass() {
        let specs = [
            ValueFlag {
                names: &["--title", "-t"],
                eq_prefix: "--title=",
            },
            ValueFlag {
                names: &["--repo", "-R"],
                eq_prefix: "--repo=",
            },
        ];
        let r = scan_value_flags(
            &v(&["gh", "pr", "create", "--title", "-R", "--repo", "o/r"]),
            &specs,
        );
        assert_eq!(
            r[0],
            FlagValue {
                present: true,
                value: "-R".into()
            }
        );
        assert_eq!(
            r[1],
            FlagValue {
                present: true,
                value: "o/r".into()
            }
        );
        let r = scan_value_flags(&v(&["--repo=a/b", "--title"]), &specs);
        assert_eq!(
            r[0],
            FlagValue {
                present: true,
                value: String::new()
            }
        );
        assert_eq!(r[1].value, "a/b");
    }

    #[test]
    fn api_helpers() {
        let t = v(&[
            "gh",
            "api",
            "-X",
            "post",
            "repos/o/r/issues",
            "--method=PATCH",
        ]);
        assert_eq!(api_method(&t).as_deref(), Some("PATCH"));
        assert_eq!(
            api_path(&t, |p| p.starts_with("/repos/")).as_deref(),
            Some("/repos/o/r/issues")
        );
        let b = ApiBody::scan(&v(&[
            "-f",
            "title=t",
            "-F",
            "body=x",
            "--raw-field=body=y=z",
            "--input",
            "-",
        ]));
        assert!(b.have_flag);
        assert_eq!(b.texts, ["x", "y=z"]);
    }

    #[test]
    fn input_file_body_like_jq() {
        let d = tempfile::tempdir().unwrap();
        let w = |name: &str, s: &str| {
            let p = d.path().join(name);
            std::fs::write(&p, s).unwrap();
            p.to_str().unwrap().to_string()
        };
        assert_eq!(
            input_file_body(&w("a", "{\"body\":\"x\"}\n")),
            Some("x".into())
        );
        assert_eq!(input_file_body(&w("b", "{\"title\":\"x\"}")), None);
        assert_eq!(input_file_body(&w("c", "[1]")), None);
        assert_eq!(input_file_body(&w("d", "{\"body\":\"x\"} {")), None);
        assert_eq!(input_file_body(&w("e", "{\"body\":1}")), Some("1".into()));
    }
}
