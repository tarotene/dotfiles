//! detect-drift のコアロジック(#4)。
//!
//! home-manager が user 環境の単一正本(ADR-0001)である一方、apt /
//! `cargo install` / `npm -g` / `pipx` を経由した ad-hoc install は宣言と
//! 無関係に蓄積し、気づく仕組みが無かった(Issue #4)。このクレートは
//! **検出専用** — 宣言と実機在庫の diff を報告するだけで、一切変更しない
//! (`scripts/git-audit-worktrees` / `scripts/dotfiles-doctor` と同じ
//! detector-only の型)。
//!
//! ADR-0029(PATH 優先順位)がカバーする「順序由来の shadow」はここでは
//! 再検出しない — その ADR が Consequences で明示的に「在庫の削除は
//! 別 Issue」と切り出した在庫側を、この crate が引き受ける。
//!
//! パーサ(このファイル)とプロセス起動(main.rs)を分離しているのは、
//! 実コマンドの stdout を文字列として直接テストできるようにするため —
//! `cargo test` は apt/cargo/npm/pipx を一切実行しない。

use std::collections::BTreeSet;

/// 1 レイヤー(apt/cargo/npm/pipx)の diff 結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerDrift {
    pub layer: &'static str,
    /// 実機にあるが宣言に無いもの(ソート済み、重複無し)。
    pub undeclared: Vec<String>,
}

impl LayerDrift {
    pub fn is_clean(&self) -> bool {
        self.undeclared.is_empty()
    }
}

/// `packages/declarative/apt-packages.txt` を解析する。`#` 始まりはコメント、
/// 空行は無視。前後空白は trim する。
pub fn parse_apt_declared(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// `apt-mark showmanual` の出力(1行1パッケージ名)を解析する。
pub fn parse_apt_installed(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// apt レイヤーの diff: 手動インストール済みだが宣言に無いもの。
pub fn diff_apt(declared: &BTreeSet<String>, installed: &BTreeSet<String>) -> LayerDrift {
    let undeclared = installed.difference(declared).cloned().collect();
    LayerDrift {
        layer: "apt",
        undeclared,
    }
}

/// `cargo install --list` の出力を解析し、パッケージ名の集合を返す。
/// 形式は `<pkgname> v<version>:` の見出し行に続けて、インデントされた
/// バイナリ名の行が並ぶ(`<pkgname> v<version> (<path>):` の派生形もある
/// — git/path 由来のインストールで括弧内にソース情報が付く)。バイナリ名
/// 自体ではなくパッケージ名だけを拾う: 見出し行は行頭が空白でなく `:` で
/// 終わる行。
pub fn parse_cargo_installed(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| !l.starts_with(char::is_whitespace))
        .filter_map(|l| {
            let l = l.trim_end();
            let l = l.strip_suffix(':')?;
            // "<name> v<ver>" または "<name> v<ver> (<path>)" の先頭トークン。
            let name = l.split_whitespace().next()?;
            Some(name.to_string())
        })
        .collect()
}

/// cargo レイヤーの diff: この repo は cargo install に対する宣言ファイルを
/// 持たない(Issue #4 の設計どおり)ので、インストール済み全件が候補。
pub fn diff_cargo(installed: &BTreeSet<String>) -> LayerDrift {
    LayerDrift {
        layer: "cargo",
        undeclared: installed.iter().cloned().collect(),
    }
}

/// `npm ls -g --depth=0 --json` の出力から `.dependencies` のキー(パッケージ
/// 名)を抽出する。`npm` 自身は常に存在し ad-hoc install の対象ではないため
/// 除外する。壊れた JSON は空集合を返す(呼び出し側が WARN として扱う)。
pub fn parse_npm_global(json_text: &str) -> BTreeSet<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return BTreeSet::new();
    };
    let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) else {
        return BTreeSet::new();
    };
    deps.keys()
        .filter(|k| k.as_str() != "npm")
        .cloned()
        .collect()
}

pub fn diff_npm(installed: &BTreeSet<String>) -> LayerDrift {
    LayerDrift {
        layer: "npm",
        undeclared: installed.iter().cloned().collect(),
    }
}

/// `pipx list --json` の出力から `.venvs` のキー(パッケージ名)を抽出する。
pub fn parse_pipx_venvs(json_text: &str) -> BTreeSet<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return BTreeSet::new();
    };
    let Some(venvs) = v.get("venvs").and_then(|d| d.as_object()) else {
        return BTreeSet::new();
    };
    venvs.keys().cloned().collect()
}

pub fn diff_pipx(installed: &BTreeSet<String>) -> LayerDrift {
    LayerDrift {
        layer: "pipx",
        undeclared: installed.iter().cloned().collect(),
    }
}

/// ADR-0025 対象(update-own-tools のホストローカルレジストリに載る
/// pre-release CLI 名)を drift レイヤーから除外する。cargo/npm/pipx
/// レイヤーはインストール済み全件を候補にするため、そこに ADR-0025 対象が
/// 混ざりうる — この名前を GitHub Issue へ書くことは ADR-0025 が無条件で
/// 禁じている(「存在すら書かない」)。除外は `--file-issue`(main.rs)経由の
/// 起票本文の組み立てでのみ使う。通常の(GitHub に出ない)ターミナル出力は
/// この関数を通さない — ホストローカルな自分の端末で自分のレジストリの
/// 中身を見ることは ADR-0025 の対象外。
pub fn filter_registry_excluded(
    drifts: &[LayerDrift],
    registry: &update_own_tools::Registry,
) -> (Vec<LayerDrift>, usize) {
    let registered: BTreeSet<&str> = registry.tools.iter().map(|t| t.name.as_str()).collect();
    let mut excluded_count = 0usize;
    let filtered = drifts
        .iter()
        .map(|d| {
            let kept: Vec<String> = d
                .undeclared
                .iter()
                .filter(|name| {
                    let is_registered = registered.contains(name.as_str());
                    if is_registered {
                        excluded_count += 1;
                    }
                    !is_registered
                })
                .cloned()
                .collect();
            LayerDrift {
                layer: d.layer,
                undeclared: kept,
            }
        })
        .collect();
    (filtered, excluded_count)
}

/// `--file-issue` の結果。起票そのものを要求しなかった(CLI 単独実行)か、
/// 配信(起票・コメント追記・報告対象ゼロによる無配信)に成功したか、
/// 配信に失敗したか — 3 択の閉じた語彙にすることで、`exit_code` が
/// 未定義の組み合わせを扱わずに済む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIssueOutcome {
    NotRequested,
    Delivered,
    Failed,
}

/// systemd/launchd の oneshot unit が「成功」と「要修復」のどちらに終わる
/// べきかを決める純粋関数。cargo/npm/pipx レイヤーは宣言ファイルを持たない
/// ため在庫全件が drift 扱いになり(#4 の設計)、`--file-issue` 運用では
/// drift の有無そのものを exit code の失敗条件にすると恒久的に failed に
/// なる(2026-09-24 の実機観測、hms が degraded と報告した原因)。
/// `--file-issue` を要求していない CLI 単独実行では、従来どおり
/// drift の有無を報告する(`detect-drift`/`detect-drift --porcelain` の
/// 既存契約を維持)。
///
/// 戻り値: 0 = clean、1 = drift(CLI 契約) / 0 = 配信済み、3 = 起票失敗
/// (`--file-issue` 契約。2 は usage error で既に main.rs が使用中)。
pub fn exit_code(any_drift: bool, outcome: FileIssueOutcome) -> u8 {
    match outcome {
        FileIssueOutcome::NotRequested => u8::from(any_drift),
        FileIssueOutcome::Delivered => 0,
        FileIssueOutcome::Failed => 3,
    }
}

/// 自動起票する GitHub Issue の title/body を組み立てる(純粋関数、
/// 副作用なし)。`drifts` は `filter_registry_excluded` を通した後のもの
/// でなければならない — この関数自身は ADR-0025 のフィルタを行わない。
pub fn compose_issue_report(
    hostname: &str,
    drifts: &[LayerDrift],
    excluded_count: usize,
) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "`detect-drift` が {hostname} で宣言外の ad-hoc install を検出しました。\n\n"
    ));
    for d in drifts {
        if d.undeclared.is_empty() {
            continue;
        }
        body.push_str(&format!("## {}\n\n", d.layer));
        for name in &d.undeclared {
            body.push_str(&format!("- `{name}`\n"));
        }
        body.push('\n');
    }
    if excluded_count > 0 {
        body.push_str(&format!(
            "({excluded_count} 件は update-own-tools のホストローカルレジストリに登録済みのため、ADR-0025 によりここには列挙していません。)\n\n"
        ));
    }
    body.push_str(
        "宣言(home-manager / packages/declarative/apt-packages.txt)へ取り込むか、\
         escape hatch として残すかは人間の判断です(Issue #4 の分類ルール)。\n",
    );
    body
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use update_own_tools::parse_registry;

    fn drift(layer: &'static str, names: &[&str]) -> LayerDrift {
        LayerDrift {
            layer,
            undeclared: names.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn filter_registry_excluded_removes_registered_tools() {
        let registry = parse_registry(
            r#"
            [[tool]]
            name = "telepath"
            repo = "/home/x/.ghr/example/telepath"
            "#,
        )
        .unwrap();
        let drifts = vec![drift("cargo", &["telepath", "bat", "starship"])];
        let (filtered, excluded) = filter_registry_excluded(&drifts, &registry);
        assert_eq!(excluded, 1);
        assert_eq!(
            filtered[0].undeclared,
            vec!["bat".to_string(), "starship".to_string()]
        );
    }

    #[test]
    fn filter_registry_excluded_noop_on_empty_registry() {
        let registry = parse_registry("").unwrap();
        let drifts = vec![drift("cargo", &["bat"])];
        let (filtered, excluded) = filter_registry_excluded(&drifts, &registry);
        assert_eq!(excluded, 0);
        assert_eq!(filtered[0].undeclared, vec!["bat".to_string()]);
    }

    #[test]
    fn compose_issue_report_lists_layers_and_notes_exclusions() {
        let drifts = vec![
            drift("apt", &["pandoc"]),
            drift("cargo", &[]),
            drift("npm", &["typescript"]),
        ];
        let body = compose_issue_report("vega", &drifts, 2);
        assert!(body.contains("vega"));
        assert!(body.contains("## apt"));
        assert!(body.contains("`pandoc`"));
        assert!(!body.contains("## cargo")); // empty layer omitted
        assert!(body.contains("## npm"));
        assert!(body.contains("`typescript`"));
        assert!(body.contains("2 件は"));
    }

    #[test]
    fn compose_issue_report_omits_exclusion_note_when_zero() {
        let drifts = vec![drift("apt", &["pandoc"])];
        let body = compose_issue_report("vega", &drifts, 0);
        assert!(!body.contains("ADR-0025"));
    }

    #[test]
    fn exit_code_not_requested_reports_drift_presence() {
        assert_eq!(exit_code(false, FileIssueOutcome::NotRequested), 0);
        assert_eq!(exit_code(true, FileIssueOutcome::NotRequested), 1);
    }

    #[test]
    fn exit_code_delivered_is_always_success_regardless_of_drift() {
        assert_eq!(exit_code(false, FileIssueOutcome::Delivered), 0);
        assert_eq!(exit_code(true, FileIssueOutcome::Delivered), 0);
    }

    #[test]
    fn exit_code_failed_is_always_nonzero_regardless_of_drift() {
        assert_eq!(exit_code(false, FileIssueOutcome::Failed), 3);
        assert_eq!(exit_code(true, FileIssueOutcome::Failed), 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apt_declared_skips_comments_and_blanks() {
        let text = "# header\nzsh\n\nbuild-essential\n# --- section ---\npkg-config\n";
        let got = parse_apt_declared(text);
        assert_eq!(
            got,
            BTreeSet::from([
                "zsh".to_string(),
                "build-essential".to_string(),
                "pkg-config".to_string(),
            ])
        );
    }

    #[test]
    fn diff_apt_reports_only_undeclared() {
        let declared = BTreeSet::from(["zsh".to_string(), "jq".to_string()]);
        let installed = BTreeSet::from(["zsh".to_string(), "jq".to_string(), "pandoc".to_string()]);
        let drift = diff_apt(&declared, &installed);
        assert_eq!(drift.undeclared, vec!["pandoc".to_string()]);
        assert!(!drift.is_clean());
    }

    #[test]
    fn diff_apt_clean_when_no_extra_installed() {
        let declared = BTreeSet::from(["zsh".to_string()]);
        let installed = BTreeSet::from(["zsh".to_string()]);
        let drift = diff_apt(&declared, &installed);
        assert!(drift.is_clean());
    }

    #[test]
    fn parse_cargo_installed_extracts_package_names() {
        let text = "\
alacritty v0.15.1:
    alacritty
cargo-flash v0.24.0:
    cargo-flash
    cargo-embed
ghr v1.2.3 (/home/x/.ghr/some/path):
    ghr
";
        let got = parse_cargo_installed(text);
        assert_eq!(
            got,
            BTreeSet::from([
                "alacritty".to_string(),
                "cargo-flash".to_string(),
                "ghr".to_string(),
            ])
        );
    }

    #[test]
    fn parse_cargo_installed_empty_on_no_packages() {
        assert_eq!(parse_cargo_installed(""), BTreeSet::new());
    }

    #[test]
    fn diff_cargo_reports_all_installed() {
        let installed = BTreeSet::from(["bat".to_string(), "starship".to_string()]);
        let drift = diff_cargo(&installed);
        assert_eq!(
            drift.undeclared,
            vec!["bat".to_string(), "starship".to_string()]
        );
    }

    #[test]
    fn parse_npm_global_extracts_deps_excluding_npm_itself() {
        let json = r#"{
          "dependencies": {
            "npm": {"version": "10.0.0"},
            "typescript": {"version": "5.0.0"},
            "eslint": {"version": "8.0.0"}
          }
        }"#;
        let got = parse_npm_global(json);
        assert_eq!(
            got,
            BTreeSet::from(["typescript".to_string(), "eslint".to_string()])
        );
    }

    #[test]
    fn parse_npm_global_empty_dependencies() {
        let json = r#"{"dependencies": {}}"#;
        assert_eq!(parse_npm_global(json), BTreeSet::new());
    }

    #[test]
    fn parse_npm_global_malformed_json_returns_empty() {
        assert_eq!(parse_npm_global("not json"), BTreeSet::new());
    }

    #[test]
    fn parse_pipx_venvs_extracts_keys() {
        let json = r#"{"venvs": {"black": {}, "ruff": {}}}"#;
        let got = parse_pipx_venvs(json);
        assert_eq!(
            got,
            BTreeSet::from(["black".to_string(), "ruff".to_string()])
        );
    }

    #[test]
    fn parse_pipx_venvs_malformed_json_returns_empty() {
        assert_eq!(parse_pipx_venvs("{{{"), BTreeSet::new());
    }

    #[test]
    fn diff_npm_and_pipx_report_all_installed() {
        let npm_installed = BTreeSet::from(["typescript".to_string()]);
        assert_eq!(
            diff_npm(&npm_installed).undeclared,
            vec!["typescript".to_string()]
        );
        let pipx_installed = BTreeSet::from(["black".to_string()]);
        assert_eq!(
            diff_pipx(&pipx_installed).undeclared,
            vec!["black".to_string()]
        );
    }
}
