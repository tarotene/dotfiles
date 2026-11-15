//! desk-github-gate — 机の段 3a(GitHub 通知の消化)の関門(ADR-523、#1575 で公開側へ移した)。
//!
//! GitHub のアカウントは個人と会社で共用なので、どちらの機体で見ているかが確かめられたときだけ、
//! エージェントが GitHub の通知に触れてよい。次の 3 つがすべて成り立ったときだけ成功し、通知ツールの
//! 範囲の報告(`<通知ツール> scope --json`)をそのまま返す。1 つでも崩れたら失敗で、理由を返す(その場合、通知には
//! 1 件も触れない)。
//!
//! 1. この機体の identity と、ログイン中の Claude アカウントが一致する(`dotfiles-identity verify`。
//!    `docs/dotfiles-identity.md`)。
//! 2. 通知ツールの `[github]` の範囲が設定されていて、その側が 1. の identity と同じ。
//! 3. 通知ツールのキャッシュが、現在の範囲で作られている(古ければ `<通知ツール> sync` で作り直す)。
//!
//! 通知ツールは非公開のリポジトリの道具なので、名前を公開のソースに書かない(ADR-0034)。コマンド名は、環境変数
//! `DESK_NOTIFY_TOOL`(セッション限定の上書き)か、目印ファイル `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/desk-notify-tool`
//! の 1 行目から解決する([`resolve_tool`])。呼び出しの形は `<通知ツール> scope --json` と `<通知ツール> sync` だけ。
//!
//! 報告(JSON)は `{"side","stale","synced","other_side":[…],"unassigned":[…]}`(`<通知ツール> scope --json` の応答)。`unassigned` は、会社側にも
//! 個人側にも表の無い持ち主で、机は、持ち主ごとにどちら側かを本人に聞く。
//!
//! この関門は、かつて個人側のリポジトリのスクリプトにあった。中身は、`dotfiles-identity` と通知ツールを呼ぶだけで
//! 個人側のデータに依らないので、公開側に置き、個人と会社の両方の机が同じ道具を使う(ADR-1573)。

use serde_json::Value;

/// 通知ツールのコマンド名を解く。環境変数(空でないもの)、無ければ目印ファイルの 1 行目。どちらも無ければ `None`。
/// 値は、空白を含まない 1 語(コマンド名か絶対パス)でなければ `None`(引数の注入を避ける)。
pub fn resolve_tool(env: Option<&str>, marker: Option<&str>) -> Option<String> {
    let first_line = |s: &str| s.lines().next().map(|l| l.trim().to_string());
    let pick = env
        .and_then(first_line)
        .filter(|v| !v.is_empty())
        .or_else(|| marker.and_then(first_line).filter(|v| !v.is_empty()))?;
    (!pick.contains(char::is_whitespace)).then_some(pick)
}

/// 外のコマンドの呼び出し(試験で差し替える)。
pub trait Runner {
    /// コマンドが PATH にあるか。
    fn exists(&self, cmd: &str) -> bool;
    /// コマンドを実行し、(成功か, 標準出力)を返す。標準エラーは、そのまま引き継ぐ(`identity verify` の失敗の
    /// 理由を、本人が読めるように)。起動できなければ `None`。
    fn run(&self, cmd: &str, args: &[&str]) -> Option<(bool, String)>;
}

/// 関門の失敗の理由。標準エラーに `desk-github-gate: <理由>` として出す。
pub type Failure = String;

/// 3 つの条件を確かめる。成功なら、通知ツールの範囲の報告(JSON の文字列)。
pub fn gate(r: &dyn Runner, tool: &str) -> Result<String, Failure> {
    for cmd in ["dotfiles-identity", tool] {
        if !r.exists(cmd) {
            return Err(format!("{cmd} が見つからない"));
        }
    }

    // 1. 機体の宣言と、ログイン中の Claude アカウントの一致。失敗の理由は verify が標準エラーに出す。
    let identity = match r.run("dotfiles-identity", &["verify"]) {
        Some((true, out)) => out.trim().to_string(),
        _ => {
            return Err(
                "この機体の identity とログイン中の Claude アカウントが一致しない".to_string(),
            )
        }
    };

    let mut report = read_report(r, tool)?;
    check_side(&report, &identity)?;

    // 3. キャッシュが現在の範囲で作られている。まだ無い、または別の範囲で作られていたら作り直す。
    if needs_sync(&report) {
        match r.run(tool, &["sync"]) {
            Some((true, out)) => {
                // 同期の経過は、報告(標準出力)に混ぜず、標準エラーに出す。
                eprint!("{out}");
            }
            _ => return Err("通知ツールの sync が失敗した".to_string()),
        }
        report = read_report(r, tool)?;
        check_side(&report, &identity)?;
    }
    if report.json.get("synced").and_then(Value::as_bool) != Some(true) {
        return Err("sync のあとも、キャッシュが無い".to_string());
    }
    if report.json.get("stale").and_then(Value::as_bool) != Some(false) {
        return Err("sync のあとも、キャッシュが現在の範囲で作られていない".to_string());
    }
    Ok(report.text)
}

/// `<通知ツール> scope --json` の応答(原文と、読んだ JSON)。
struct Report {
    text: String,
    json: Value,
}

fn read_report(r: &dyn Runner, tool: &str) -> Result<Report, Failure> {
    let failed = || "通知ツールの scope が失敗した(設定ファイルの誤りを含む)".to_string();
    let (ok, out) = r.run(tool, &["scope", "--json"]).ok_or_else(failed)?;
    if !ok {
        return Err(failed());
    }
    let json: Value = serde_json::from_str(&out).map_err(|_| failed())?;
    Ok(Report {
        text: out.trim_end_matches('\n').to_string(),
        json,
    })
}

/// 2. 通知ツールの側が、1. の identity と同じ。
fn check_side(report: &Report, identity: &str) -> Result<(), Failure> {
    let side = report
        .json
        .get("side")
        .and_then(Value::as_str)
        .unwrap_or("");
    if side.is_empty() {
        return Err("通知ツールの [github] 範囲が未設定(side が無い)".to_string());
    }
    if side != identity {
        return Err(format!(
            "通知ツールの側({side})が、この機体の identity({identity})と違う"
        ));
    }
    Ok(())
}

/// キャッシュを作り直す必要があるか: まだ無い(`synced` が真でない)、または別の範囲で作られている(`stale`)。
fn needs_sync(report: &Report) -> bool {
    let synced = report.json.get("synced").and_then(Value::as_bool);
    let stale = report.json.get("stale").and_then(Value::as_bool);
    synced != Some(true) || stale == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    const TOOL: &str = "notify-tool";

    /// 偽の外のコマンド。`<通知ツール> scope --json` は、呼ばれた順に応答を返す(最後の応答は繰り返す)。
    struct Fake {
        present: HashSet<&'static str>,
        identity: Option<&'static str>,
        scopes: Vec<Option<String>>,
        sync_ok: bool,
        calls: RefCell<Vec<String>>,
        scope_count: RefCell<HashMap<(), usize>>,
    }

    impl Fake {
        fn new(identity: Option<&'static str>, scopes: Vec<Option<String>>) -> Fake {
            Fake {
                present: ["dotfiles-identity", TOOL].into_iter().collect(),
                identity,
                scopes,
                sync_ok: true,
                calls: RefCell::new(Vec::new()),
                scope_count: RefCell::new(HashMap::new()),
            }
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Runner for Fake {
        fn exists(&self, cmd: &str) -> bool {
            self.present.contains(cmd)
        }
        fn run(&self, cmd: &str, args: &[&str]) -> Option<(bool, String)> {
            self.calls
                .borrow_mut()
                .push(format!("{cmd} {}", args.join(" ")));
            match (cmd, args) {
                ("dotfiles-identity", ["verify"]) => match self.identity {
                    Some(i) => Some((true, format!("{i}\n"))),
                    None => Some((false, String::new())),
                },
                (TOOL, ["scope", "--json"]) => {
                    let mut n = self.scope_count.borrow_mut();
                    let i = n.entry(()).or_insert(0);
                    let idx = (*i).min(self.scopes.len() - 1);
                    *i += 1;
                    match &self.scopes[idx] {
                        Some(s) => Some((true, format!("{s}\n"))),
                        None => Some((false, String::new())),
                    }
                }
                (TOOL, ["sync"]) => Some((self.sync_ok, String::new())),
                _ => panic!("想定外の呼び出し: {cmd} {args:?}"),
            }
        }
    }

    fn report(side: &str, stale: bool, synced: bool) -> Option<String> {
        Some(
            serde_json::json!({
                "side": side, "stale": stale, "synced": synced,
                "other_side": [], "unassigned": ["someone"]
            })
            .to_string(),
        )
    }

    #[test]
    fn a_fresh_matching_report_passes_and_is_returned_as_is() {
        let f = Fake::new(Some("personal"), vec![report("personal", false, true)]);
        let out = gate(&f, TOOL).unwrap();
        assert_eq!(out, report("personal", false, true).unwrap());
        assert_eq!(
            f.calls()
                .iter()
                .filter(|c| c.starts_with("notify-tool sync"))
                .count(),
            0
        );
    }

    #[test]
    fn missing_commands_fail_before_anything_runs() {
        let mut f = Fake::new(Some("personal"), vec![report("personal", false, true)]);
        f.present.remove(TOOL);
        assert_eq!(gate(&f, TOOL).unwrap_err(), "notify-tool が見つからない");
        assert!(f.calls().is_empty());
        let mut f = Fake::new(Some("personal"), vec![report("personal", false, true)]);
        f.present.remove("dotfiles-identity");
        assert_eq!(
            gate(&f, TOOL).unwrap_err(),
            "dotfiles-identity が見つからない"
        );
    }

    #[test]
    fn a_failed_identity_check_never_reads_the_notification_tool() {
        let f = Fake::new(None, vec![report("personal", false, true)]);
        let e = gate(&f, TOOL).unwrap_err();
        assert!(
            e.contains("identity とログイン中の Claude アカウントが一致しない"),
            "{e}"
        );
        assert_eq!(f.calls(), vec!["dotfiles-identity verify"]);
    }

    #[test]
    fn the_notification_scope_must_exist_and_match_the_identity() {
        let f = Fake::new(Some("company"), vec![report("personal", false, true)]);
        let e = gate(&f, TOOL).unwrap_err();
        assert_eq!(
            e,
            "通知ツールの側(personal)が、この機体の identity(company)と違う"
        );
        let f = Fake::new(
            Some("personal"),
            vec![Some(r#"{"stale":false,"synced":true}"#.into())],
        );
        assert!(gate(&f, TOOL).unwrap_err().contains("未設定"));
        let f = Fake::new(
            Some("personal"),
            vec![Some(r#"{"side":"","synced":true}"#.into())],
        );
        assert!(gate(&f, TOOL).unwrap_err().contains("未設定"));
    }

    #[test]
    fn a_failed_or_unreadable_scope_report_is_a_failure() {
        let f = Fake::new(Some("personal"), vec![None]);
        assert!(gate(&f, TOOL)
            .unwrap_err()
            .contains("通知ツールの scope が失敗した"));
        let f = Fake::new(Some("personal"), vec![Some("not json".into())]);
        assert!(gate(&f, TOOL)
            .unwrap_err()
            .contains("通知ツールの scope が失敗した"));
    }

    #[test]
    fn a_stale_or_missing_cache_is_rebuilt_once_and_rechecked() {
        for first in [
            report("personal", true, true),
            report("personal", false, false),
        ] {
            let f = Fake::new(
                Some("personal"),
                vec![first, report("personal", false, true)],
            );
            let out = gate(&f, TOOL).unwrap();
            assert_eq!(out, report("personal", false, true).unwrap());
            let sync = f
                .calls()
                .iter()
                .filter(|c| *c == "notify-tool sync")
                .count();
            assert_eq!(sync, 1, "{:?}", f.calls());
        }
    }

    #[test]
    fn the_side_is_checked_again_after_the_rebuild() {
        // 作り直したあとの範囲の側が、機体の identity と違えば、失敗(通知に触れない)。
        let f = Fake::new(
            Some("personal"),
            vec![
                report("personal", true, true),
                report("company", false, true),
            ],
        );
        assert!(gate(&f, TOOL)
            .unwrap_err()
            .contains("通知ツールの側(company)"));
    }

    #[test]
    fn a_failed_sync_or_a_cache_that_stays_stale_is_a_failure() {
        let mut f = Fake::new(Some("personal"), vec![report("personal", true, true)]);
        f.sync_ok = false;
        assert_eq!(gate(&f, TOOL).unwrap_err(), "通知ツールの sync が失敗した");
        let f = Fake::new(Some("personal"), vec![report("personal", true, true)]);
        assert_eq!(
            gate(&f, TOOL).unwrap_err(),
            "sync のあとも、キャッシュが現在の範囲で作られていない"
        );
        let f = Fake::new(Some("personal"), vec![report("personal", false, false)]);
        assert_eq!(
            gate(&f, TOOL).unwrap_err(),
            "sync のあとも、キャッシュが無い"
        );
    }

    #[test]
    fn a_missing_stale_field_is_not_treated_as_fresh() {
        // `stale` が無い報告は、新しいとみなさない(再構築しても無ければ失敗)。
        let f = Fake::new(
            Some("personal"),
            vec![Some(r#"{"side":"personal","synced":true}"#.into())],
        );
        assert_eq!(
            gate(&f, TOOL).unwrap_err(),
            "sync のあとも、キャッシュが現在の範囲で作られていない"
        );
        assert!(
            f.calls().iter().all(|c| c != "notify-tool sync"),
            "stale が無いだけでは作り直さない"
        );
    }

    #[test]
    fn the_tool_name_comes_from_the_env_then_the_marker_and_must_be_one_word() {
        assert_eq!(
            resolve_tool(Some("a-tool"), Some("b-tool")).as_deref(),
            Some("a-tool")
        );
        // 環境変数が空・空白だけなら、目印を使う。目印は 1 行目だけを読む。
        assert_eq!(
            resolve_tool(Some(" "), Some("b-tool\nignored")).as_deref(),
            Some("b-tool")
        );
        assert_eq!(
            resolve_tool(None, Some("  c-tool  \n")).as_deref(),
            Some("c-tool")
        );
        assert_eq!(
            resolve_tool(None, Some("/abs/path/tool")).as_deref(),
            Some("/abs/path/tool")
        );
        // どちらも無い・空は None。空白を含む値(引数の注入)は使わない。
        assert_eq!(resolve_tool(None, None), None);
        assert_eq!(resolve_tool(Some(""), Some("")), None);
        assert_eq!(resolve_tool(Some("tool --flag"), Some("b-tool")), None);
    }
}
