//! hook の逃げ道の不変条件: 名前・スコープ・真偽は `agent_session::escape` の表が唯一の正本である。
//!
//! - **表の外の綴り**: guard の `src/` に、逃げ道の名前の文字列(`SKIP_…`・`…_ALLOW`・`…_BYPASS`・`…_SKIP` の形)を
//!   直書きしない。名前は表(`escape::ALL`)から導く。拒否文に `NAME=1` の形で書くのも同じ(案内は `how_to`)。
//! - **skip ファイル**: `join("skip")` を直書きしない。置き場所のディレクトリは各機能が持ち、ファイル名は
//!   `escape::SKIP_FILE` と `Hatch::active` が持つ。
//! - **退役した名前**: 改名した旧名と、廃止した `--check-plan` が、凍結する ADR(`docs/adr/`)と、この検査自身を除く
//!   ツリーのどこにも残っていない(参照元を同じ段で直した、という完了の恒久の保証)。旧名は別名として読まない。
//!
//! 検査は、`hook_io` と同じく、コメントの行と、ファイル末尾のテストの節を読まない(退役した名前の検査は、
//! テストとコメントも含めて、ファイル全体を読む)。例外は無い。

use std::fs;
use std::path::Path;

use crate::hook_io::{code_lines, is_owner, read_sources, Source};
use crate::manifest::Violation;

/// 退役した名前と、その後継。
pub const RETIRED: &[(&str, &str)] = &[
    ("SKIP_PLAN_REVIEW", "SKIP_COPILOT_PLAN_REVIEW"),
    ("PLAN_VIEW_SKIP", "SKIP_PLAN_VIEW"),
    ("PR_TITLE_GUARD_ALLOW", "SKIP_PR_TITLE_GUARD"),
    ("PR_CONFIRM_GUARD_ALLOW", "SKIP_PR_FOLLOWUP_GUARD"),
    ("EXTERNAL_SEND_GUARD_ALLOW", "SKIP_EXTERNAL_SEND_GUARD"),
    ("JA_POLISH_GATE_ALLOW", "SKIP_JA_POLISH_GATE"),
    ("REPO_CREATE_GUARD_BYPASS", "SKIP_REPO_CREATE_GUARD"),
    ("RULESETS_WRITE_GUARD_BYPASS", "RULESETS_WRITE_GUARD_ALLOW"),
    ("--check-plan", "--check"),
];

/// 退役した名前を探さない場所(リポジトリの根からの相対パスの接頭辞)。ADR は受理後に変えない履歴で、
/// この検査の crate は、退役した名前を列挙する側である。
const RETIRED_EXEMPT: &[&str] = &["docs/adr/", "crates/repo-structure-check/"];

/// 走査から外すディレクトリ名。
const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules", ".direnv", "result"];

/// 1 ファイルの大きさの上限(これより大きいものは、テキストの走査から外す)。
const MAX_FILE_BYTES: u64 = 2_000_000;

fn violation(path: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: path.to_string(),
        detail: detail.into(),
    }
}

/// 逃げ道の名前の形か(全体が大文字・数字・`_`で、`SKIP_` で始まるか、`_ALLOW`・`_BYPASS`・`_SKIP` で終わる)。
fn is_hatch_name(word: &str) -> bool {
    word.len() >= 4
        && word.starts_with(|c: char| c.is_ascii_uppercase())
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && (word.starts_with("SKIP_")
            || word.ends_with("_ALLOW")
            || word.ends_with("_BYPASS")
            || word.ends_with("_SKIP"))
}

/// 行にある文字列リテラルの中身(`\"` は区切りにしない)。
fn literals(line: &str) -> Vec<String> {
    let flat = line.replace("\\\"", "'");
    flat.split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.to_string())
        .collect()
}

/// `NAME=1` の形で書かれた逃げ道の名前(拒否文への直書き)。
fn assignment_names(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(rel) = line[from..].find("=1") {
        let eq = from + rel;
        let mut start = eq;
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' {
                start -= 1;
            } else {
                break;
            }
        }
        if start < eq {
            out.push(line[start..eq].to_string());
        }
        from = eq + 2;
    }
    out
}

/// 逃げ道の名前の直書きと、`join("skip")` が、`agent-session` のほかに無いことを検査する。
pub fn check_hatch_literals(sources: &[Source]) -> Vec<Violation> {
    let mut out = Vec::new();
    for s in sources.iter().filter(|s| !is_owner(&s.path)) {
        for (n, line) in code_lines(&s.text) {
            let named = literals(line)
                .into_iter()
                .chain(assignment_names(line))
                .find(|w| is_hatch_name(w));
            if let Some(name) = named {
                out.push(violation(
                    &s.path,
                    format!(
                        "{n} 行目に逃げ道の名前 `{name}` を直書きしている。名前と案内は agent_session::escape の表(Hatch)から導く"
                    ),
                ));
            }
            if line.contains(".join(\"skip\")") {
                out.push(violation(
                    &s.path,
                    format!(
                        "{n} 行目で `join(\"skip\")` と直書きしている。skip ファイルは Hatch::active が見る(置き場のディレクトリだけを渡す)"
                    ),
                ));
            }
        }
    }
    out
}

/// 検査する 1 ファイル(リポジトリの根からの相対パスと、テキスト全体)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub path: String,
    pub text: String,
}

fn collect_text(dir: &Path, root: &Path, out: &mut Vec<TextFile>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                collect_text(&p, root, out);
            }
        } else if meta.is_file() && meta.len() <= MAX_FILE_BYTES {
            let Ok(text) = fs::read_to_string(&p) else {
                continue; // テキストでない(または読めない)ものは、走査から外す。
            };
            let Ok(rel) = p.strip_prefix(root) else {
                continue;
            };
            out.push(TextFile {
                path: rel.to_string_lossy().replace('\\', "/"),
                text,
            });
        }
    }
}

/// リポジトリのテキストファイル全部を読む(`.git`・`target` などを除く)。
pub fn read_text_files(root: &Path) -> Vec<TextFile> {
    let mut out = Vec::new();
    collect_text(root, root, &mut out);
    out
}

/// 退役した名前が、ADR とこの検査のほかに無いことを検査する。
pub fn check_retired_names(files: &[TextFile]) -> Vec<Violation> {
    let mut out = Vec::new();
    for f in files
        .iter()
        .filter(|f| !RETIRED_EXEMPT.iter().any(|p| f.path.starts_with(p)))
    {
        for (old, new) in RETIRED {
            if let Some((i, _)) = f.text.lines().enumerate().find(|(_, l)| l.contains(old)) {
                out.push(violation(
                    &f.path,
                    format!(
                        "{} 行目に退役した名前 `{old}` が残っている。`{new}` に直す(旧名は別名として読まない)",
                        i + 1
                    ),
                ));
            }
        }
    }
    out
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let sources = read_sources(root)?;
    let mut out = check_hatch_literals(&sources);
    out.extend(check_retired_names(&read_text_files(root)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(path: &str, text: &str) -> Source {
        Source {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    fn file(path: &str, text: &str) -> TextFile {
        TextFile {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    // ---- 表の外の綴り: 正例 ----

    #[test]
    fn names_that_are_not_hatches_pass() {
        let text = "const A: &str = \"GH_EDIT_ALLOW_DIR\";\n\
                    let b = \"PR_GATE_ALLOWLIST\";\n\
                    let c = \"skip\";\n\
                    let d = \"SKIP\";\n\
                    let e = \"Skip_x\";\n";
        assert_eq!(
            check_hatch_literals(&[src("crates/x/src/lib.rs", text)]),
            vec![]
        );
    }

    #[test]
    fn the_owner_comments_and_tests_are_not_read() {
        let text = "//! `SKIP_FOO=1` と \"BAR_ALLOW\" の説明\n\
                    fn f() {}\n\
                    #[cfg(test)]\n\
                    mod tests { const X: &str = \"SKIP_FOO\"; fn g() { h(\"BAR_ALLOW=1 cmd\"); } }\n";
        let v = vec![
            src("crates/x/src/lib.rs", text),
            src(
                "crates/agent-session/src/escape.rs",
                "format!(\"SKIP_{}\", stem); let k = \"X_ALLOW=1\";\n",
            ),
        ];
        assert_eq!(check_hatch_literals(&v), vec![]);
    }

    // ---- 表の外の綴り: 負例(検査が実際に落ちること) ----

    #[test]
    fn a_hatch_name_literal_is_a_violation() {
        for line in [
            "let v = std::env::var(\"SKIP_FOO_GUARD\");",
            "let v = std::env::var(\"FOO_GUARD_ALLOW\");",
            "let v = std::env::var(\"FOO_GUARD_BYPASS\");",
            "let v = std::env::var(\"FOO_VIEW_SKIP\");",
        ] {
            let vs = check_hatch_literals(&[src(
                "crates/x/src/lib.rs",
                &format!("fn f() {{\n    {line}\n}}\n"),
            )]);
            assert_eq!(vs.len(), 1, "{line}: {vs:?}");
            assert!(vs[0].detail.contains("2 行目"), "{}", vs[0].detail);
        }
    }

    #[test]
    fn a_name_equals_one_in_a_message_is_a_violation() {
        let vs = check_hatch_literals(&[src(
            "crates/x/src/lib.rs",
            "let m = \"やむを得ないときだけ、コマンドに FOO_GUARD_ALLOW=1 を足してください。\";\n",
        )]);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert!(vs[0].detail.contains("FOO_GUARD_ALLOW"), "{}", vs[0].detail);
    }

    #[test]
    fn a_handwritten_skip_file_is_a_violation() {
        let vs =
            check_hatch_literals(&[src("crates/x/src/lib.rs", "let p = dir.join(\"skip\");\n")]);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert!(vs[0].detail.contains("1 行目"), "{}", vs[0].detail);
    }

    // ---- 退役した名前 ----

    #[test]
    fn retired_names_are_found_anywhere_but_in_adrs_and_the_check_itself() {
        let old = RETIRED[0].0;
        let files = vec![
            file("docs/agents/x.md", &format!("説明\n{old}=1 を設定する\n")),
            file("crates/x/tests/cli.rs", &format!("env(\"{old}\")\n")),
            file("docs/adr/0031-x.md", &format!("履歴: {old}\n")),
            file("crates/repo-structure-check/src/escape.rs", old),
        ];
        let vs = check_retired_names(&files);
        let paths: Vec<&str> = vs.iter().map(|v| v.subject.as_str()).collect();
        assert_eq!(paths, vec!["docs/agents/x.md", "crates/x/tests/cli.rs"]);
        assert!(vs[0].detail.contains("2 行目"), "{}", vs[0].detail);
    }

    #[test]
    fn every_retired_name_is_checked() {
        for (old, _) in RETIRED {
            let vs = check_retired_names(&[file("docs/x.md", &format!("a {old} b"))]);
            assert_eq!(vs.len(), 1, "{old}");
        }
    }

    #[test]
    fn a_successor_is_not_a_retired_name() {
        for (_, new) in RETIRED {
            assert_eq!(
                check_retired_names(&[file("docs/x.md", &format!("a {new} b"))]),
                vec![],
                "{new}"
            );
        }
    }
}
