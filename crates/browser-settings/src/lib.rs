//! browser-settings — サービスの設定画面の 1 項目を、読み取り・貼り付け・読み戻しする手動の CLI(#1420)。
//!
//! 月に 1 回、複数のチャットサービスの設定画面で、保存してある設定文を読み取り、更新した文を貼り付け、
//! 貼れたことを読み戻して比べる作業がある。毎回その場でスクリプトを書かずに、1 本の CLI で行う。
//!
//! - **専用プロファイルの Chrome**(`--user-data-dir`)を、起動のたびに起動して接続し、終わったら閉じる。
//!   普段のブラウザには触れない。普段のブラウザを CDP 用のポート付きで常時起動する案は、そのポートに認証が無く、
//!   手元のどのプロセスでもログイン済みのブラウザを操作して Cookie を読めるので採らない
//!   (`docs/claude/claude-mcp-servers.md`)。Chrome 136 以降は、普段のプロファイルに対するこのポートを無視する。
//!   各サービスへのログインは、専用プロファイルで、最初の 1 回だけ人が手で行う(`login`)。
//! - **サービスごとの画面の探索はデータ**(`services.toml`)にする。コードにサービスを足さない。
//! - **`paste` は、保存の確定の手前で必ず止まる**。入力欄に文を入れて読み戻すだけで、保存ボタンは押さない。
//!   保存は人が行う(外向きの操作を代行しない)。
//!
//! この module は、設定・JavaScript の組み立て・比較の純粋な部分を持つ(Chrome の操作は `chrome`)。

pub mod chrome;

use serde::Deserialize;
use std::collections::BTreeMap;

/// 画面を開いたあと、項目を出すまでの操作 1 つ。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// この選択子の要素が出るまで待つ。
    pub wait: Option<String>,
    /// この選択子の要素をクリックする(出るまで待つ)。
    pub click: Option<String>,
}

/// サービス 1 つの、設定画面の探索。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    /// 設定画面の URL。
    pub url: String,
    /// 画面を開いたあと、項目を出すまでの操作(順に実行する)。
    #[serde(default)]
    pub steps: Vec<Step>,
    /// 読み書きする項目(入力欄 `input`・`textarea`、または contenteditable)の CSS 選択子。
    pub field: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    service: BTreeMap<String, Service>,
}

/// `services.toml` を読む。サービスの名前から探索への写し。
pub fn parse_services(text: &str) -> Result<BTreeMap<String, Service>, String> {
    let f: File = toml::from_str(text).map_err(|e| format!("services.toml を読めない: {e}"))?;
    for (name, s) in &f.service {
        for (i, step) in s.steps.iter().enumerate() {
            if step.wait.is_some() == step.click.is_some() {
                return Err(format!(
                    "service.{name} の steps[{i}] は、wait か click のどちらか 1 つだけを持つ"
                ));
            }
        }
        if s.url.is_empty() || s.field.is_empty() {
            return Err(format!("service.{name} の url と field は空にできない"));
        }
    }
    Ok(f.service)
}

/// 選択子を JavaScript の文字列リテラルにする(JSON の文字列は、そのまま JavaScript の文字列になる)。
fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// 項目の現在の値を返す式。入力欄は `value`、contenteditable は `innerText`。無ければ `null`。
pub fn js_read(selector: &str) -> String {
    format!(
        "(function(){{const e=document.querySelector({sel});if(!e)return null;\
         if(e.tagName==='INPUT'||e.tagName==='TEXTAREA')return e.value;\
         return e.innerText;}})()",
        sel = js_str(selector)
    )
}

/// 項目に文を入れる式。入力欄は、フレームワークが管理する値でも反映されるよう、ネイティブの setter で値を
/// 入れて `input` と `change` の事象を送る。contenteditable は、全選択して置き換える。入れたら `true`、
/// 項目が無ければ `false`。保存の操作は含まない。
pub fn js_paste(selector: &str, text: &str) -> String {
    format!(
        "(function(){{const e=document.querySelector({sel});if(!e)return false;e.focus();\
         const t={text};\
         if(e.tagName==='INPUT'||e.tagName==='TEXTAREA'){{\
         const p=e.tagName==='INPUT'?HTMLInputElement.prototype:HTMLTextAreaElement.prototype;\
         Object.getOwnPropertyDescriptor(p,'value').set.call(e,t);\
         e.dispatchEvent(new Event('input',{{bubbles:true}}));\
         e.dispatchEvent(new Event('change',{{bubbles:true}}));}}\
         else{{document.execCommand('selectAll',false,null);document.execCommand('insertText',false,t);}}\
         return true;}})()",
        sel = js_str(selector),
        text = js_str(text)
    )
}

/// 比べた結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 一致。
    Match,
    /// 不一致。最初に違う行(1 始まり)と、その行の期待と実際。
    Mismatch {
        line: usize,
        expected: String,
        actual: String,
    },
}

/// 改行を `\n` にそろえ、末尾の改行だけを落として比べる(画面と、ファイルの末尾の改行の違いを無視する)。
pub fn compare(expected: &str, actual: &str) -> Verdict {
    let norm = |s: &str| s.replace("\r\n", "\n").trim_end_matches('\n').to_string();
    let (e, a) = (norm(expected), norm(actual));
    if e == a {
        return Verdict::Match;
    }
    let (mut el, mut al) = (e.lines(), a.lines());
    let mut line = 0;
    loop {
        line += 1;
        match (el.next(), al.next()) {
            (Some(x), Some(y)) if x == y => continue,
            (x, y) => {
                return Verdict::Mismatch {
                    line,
                    expected: x.unwrap_or("(行が無い)").to_string(),
                    actual: y.unwrap_or("(行が無い)").to_string(),
                }
            }
        }
    }
}

/// 専用プロファイルの既定の置き場: `${XDG_STATE_HOME:-~/.local/state}/browser-settings/profile`。
pub fn default_profile_dir() -> Option<std::path::PathBuf> {
    Some(
        process_env::Xdg::State
            .resolve()?
            .join("browser-settings")
            .join("profile"),
    )
}

/// `services.toml` の既定の置き場: `${XDG_CONFIG_HOME:-~/.config}/browser-settings/services.toml`。
pub fn default_services_file() -> Option<std::path::PathBuf> {
    Some(
        process_env::Xdg::Config
            .resolve()?
            .join("browser-settings")
            .join("services.toml"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"
[service.alpha]
url = "https://example.invalid/settings"
steps = [ { click = "button.open" }, { wait = "textarea.instructions" } ]
field = "textarea.instructions"

[service.beta]
url = "https://example.invalid/b"
field = "div[contenteditable]"
"#;

    #[test]
    fn services_are_data_and_steps_are_optional() {
        let s = parse_services(TOML).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s["alpha"].steps.len(), 2);
        assert_eq!(s["alpha"].steps[0].click.as_deref(), Some("button.open"));
        assert!(s["beta"].steps.is_empty());
        // 何も書かなくても、空の一覧として読める。
        assert!(parse_services("").unwrap().is_empty());
    }

    #[test]
    fn a_step_must_have_exactly_one_of_wait_or_click() {
        for bad in [
            "[service.a]\nurl=\"u\"\nfield=\"f\"\nsteps=[{}]",
            "[service.a]\nurl=\"u\"\nfield=\"f\"\nsteps=[{wait=\"x\",click=\"y\"}]",
        ] {
            assert!(
                parse_services(bad).unwrap_err().contains("どちらか 1 つ"),
                "{bad}"
            );
        }
    }

    #[test]
    fn typos_and_empty_values_are_rejected_not_ignored() {
        assert!(parse_services("[service.a]\nurl=\"u\"\nfeild=\"f\"").is_err());
        assert!(parse_services("[service.a]\nurl=\"\"\nfield=\"f\"").is_err());
        assert!(parse_services("[sevice.a]\nurl=\"u\"").is_err());
    }

    #[test]
    fn the_javascript_carries_selector_and_text_only_as_json_strings() {
        // 引用符・改行・`</script>` を含んでも、文字列の外へ出ない。
        let sel = "a[title=\"x\"]'\\";
        let text = "1行目\n\"2行目\"; alert(1) //";
        let js = js_paste(sel, text);
        assert!(js.contains(&serde_json::to_string(sel).unwrap()));
        assert!(js.contains(&serde_json::to_string(text).unwrap()));
        assert!(!js.contains("\n"), "式は 1 行: {js}");
        assert!(js_read(sel).contains(&serde_json::to_string(sel).unwrap()));
    }

    #[test]
    fn paste_never_clicks_anything_to_save() {
        let js = js_paste("textarea", "x");
        assert!(!js.contains(".click(") && !js.contains("submit"), "{js}");
    }

    #[test]
    fn compare_ignores_the_trailing_newline_and_crlf_but_finds_the_first_difference() {
        assert_eq!(compare("a\nb\n", "a\r\nb"), Verdict::Match);
        assert_eq!(
            compare("a\nb\nc", "a\nB\nc"),
            Verdict::Mismatch {
                line: 2,
                expected: "b".into(),
                actual: "B".into()
            }
        );
        assert_eq!(
            compare("a\nb", "a"),
            Verdict::Mismatch {
                line: 2,
                expected: "b".into(),
                actual: "(行が無い)".into()
            }
        );
        // 先頭と途中の空白は意味がある。
        assert_ne!(compare(" a", "a"), Verdict::Match);
    }
}
