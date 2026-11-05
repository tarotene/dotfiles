//! Write の拒否(PreToolUse)。`hook-router` の規則が `hook(raw)` を呼ぶ。
//!
//! 宣言(`.config/record-layers.toml`)のあるリポジトリで、次の Write を拒否する:
//! - 宣言に無いパス(どの glob にも当たらない): 作る瞬間に層を決めさせる。
//! - L3・L4 と宣言したパス: Git に置かない層。
//!
//! 素通りにするもの(fail-open): 宣言の無いリポジトリ・git の外、宣言が壊れている(読めない・空・glob の誤り)、
//! 宣言のどの glob にも重なって当たる(宣言の欠陥)、`.gitignore` の対象、`.git/` の中。
//! 宣言の欠陥は、日次のずれ検出(`promotion-detect`)と `cargo test` の網羅検査が拾う。
//!
//! 拒否するのは Write だけ。Edit と Bash の書き込みは、日次のずれ検出で拾う(ADR-1358 record-layers)。

use crate::{Declaration, Layer, Lookup, DECLARATION_PATH};
use agent_session::{HookInput, Outcome};
use std::path::{Component, Path, PathBuf};

/// Write 1 件の判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// どの glob にも当たらない。
    Undeclared,
    /// L3・L4 と宣言されたパス。
    OutsideGit(Layer),
}

/// 宣言に照らした判定(git を呼ばない純粋関数)。重なり(宣言の欠陥)は拒否の根拠にしない。
pub fn decide(decl: &Declaration, rel: &str) -> Verdict {
    match decl.lookup(rel) {
        Lookup::One(layer) if layer.in_git() => Verdict::Allow,
        Lookup::One(layer) => Verdict::OutsideGit(layer),
        Lookup::Undeclared => Verdict::Undeclared,
        Lookup::Overlap(_) => Verdict::Allow,
    }
}

/// hook 本体(`main` が標準入力を読んだあとの処理に当たる)。入力が読めない・Write でない・対象外は `None`。
pub fn hook(raw: &str) -> Option<Outcome> {
    let input = HookInput::parse(raw)?;
    if input.hook_event_name != "PreToolUse" || input.tool_name != "Write" {
        return None;
    }
    let file_path = input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    let cwd = input.effective_cwd();
    let (top, rel) = locate(file_path, cwd.as_deref())?;
    if rel == ".git" || rel.starts_with(".git/") {
        return None;
    }
    let text = std::fs::read_to_string(top.join(DECLARATION_PATH)).ok()?;
    let decl = Declaration::parse(&text).ok()?;
    let verdict = decide(&decl, &rel);
    if verdict == Verdict::Allow {
        return None;
    }
    // 宣言のあるリポジトリでも、`.gitignore` の対象(`target/` など)は記録ではない。
    if git_repository::ok(&top, &["check-ignore", "-q", "--", &rel]) {
        return None;
    }
    Outcome::deny(message(&verdict, &rel))
}

fn message(verdict: &Verdict, rel: &str) -> String {
    match verdict {
        Verdict::Allow => String::new(),
        Verdict::Undeclared => format!(
            "`{rel}` は、記録の層の宣言(`{DECLARATION_PATH}`)のどの glob にも当たりません。\n\
             作る前に、この記録が現在の事実として効く期間で層を決めてください: \
             L0 不変(追記のみ)・L1 年〜月・L2 週は Git に、L3 日〜都度・L4 機械生成は Git の外に置きます。\n\
             Git に置く層なら、宣言に `[[path]] glob = \"…\" layer = \"L1\"` のように足してから Write し直してください \
             (宣言ファイル自身への Write は通ります)。\n\
             L3・L4 なら、Git の外(L3 は Obsidian Vault、L4 は vault の外)に置いてください。"
        ),
        Verdict::OutsideGit(layer) => format!(
            "`{rel}` は、記録の層の宣言(`{DECLARATION_PATH}`)で {layer}({})と宣言されたパスです。\n\
             {layer} は Git に置かない層なので、Write を拒否します。置き場は、{}。\n\
             層の宣言が誤りなら、宣言を直してから Write し直してください。",
            layer.title(),
            if *layer == Layer::L3 {
                "Git の外(Obsidian Vault が第一候補)"
            } else {
                "Git の外(vault の外)"
            }
        ),
    }
}

/// 書き込み先 `file_path` が属する git toplevel と、その toplevel からの相対パス(`/` 区切り)。
/// ファイルも親ディレクトリもまだ無いことがあるので、実在する最も近い祖先で解決する。
/// git の外・toplevel の外に出るパスは `None`(セッションの project dir には落とさない:
/// 別のリポジトリへの Write を、project dir の宣言で裁かないため)。
fn locate(file_path: &str, cwd: Option<&Path>) -> Option<(PathBuf, String)> {
    let abs = normalize(&if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else {
        cwd?.join(file_path)
    });
    let mut dir = abs.parent();
    while let Some(d) = dir {
        if d.is_dir() {
            let real = d.canonicalize().ok()?;
            let top = git_repository::toplevel(&real)?;
            let full = real.join(abs.strip_prefix(d).ok()?);
            let rel = full.strip_prefix(&top).ok()?;
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()?
                .join("/");
            return Some((top, rel));
        }
        dir = d.parent();
    }
    None
}

/// `.` と `..` を字面で畳む(ファイルがまだ無いので `canonicalize` は使えない)。
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const DECL: &str = r#"
[[path]]
glob = ".config/record-layers.toml"
layer = "L1"
[[path]]
glob = "docs/**"
layer = "L1"
[[path]]
glob = "scratch/**"
layer = "L3"
[[path]]
glob = "gen/**"
layer = "L4"
"#;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// 宣言つきのリポジトリ(`.gitignore` に `target/` を持つ)。
    fn repo(decl: Option<&str>) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
        if let Some(text) = decl {
            std::fs::create_dir_all(d.path().join(".config")).unwrap();
            std::fs::write(d.path().join(DECLARATION_PATH), text).unwrap();
        }
        d
    }

    fn write_input(file_path: &str, cwd: &Path) -> String {
        serde_json::json!({
            "hook_event_name": "PreToolUse",
            "session_id": "s",
            "tool_name": "Write",
            "tool_input": {"file_path": file_path, "content": "x"},
            "cwd": cwd,
        })
        .to_string()
    }

    fn run(d: &tempfile::TempDir, rel: &str) -> Option<Outcome> {
        hook(&write_input(
            &d.path().join(rel).to_string_lossy(),
            d.path(),
        ))
    }

    fn deny_text(o: Option<Outcome>) -> String {
        match o {
            Some(Outcome::Deny(t)) => t,
            other => panic!("deny を期待: {other:?}"),
        }
    }

    #[test]
    fn a_declared_git_layer_is_allowed() {
        let d = repo(Some(DECL));
        assert_eq!(run(&d, "docs/a.md"), None);
        assert_eq!(run(&d, "docs/deep/new/dir/a.md"), None);
        assert_eq!(run(&d, ".config/record-layers.toml"), None);
    }

    #[test]
    fn an_undeclared_path_is_denied_with_the_way_out() {
        let d = repo(Some(DECL));
        let t = deny_text(run(&d, "src/new.rs"));
        assert!(
            t.contains("src/new.rs") && t.contains(DECLARATION_PATH),
            "{t}"
        );
    }

    #[test]
    fn l3_and_l4_paths_are_denied() {
        let d = repo(Some(DECL));
        let t = deny_text(run(&d, "scratch/today.md"));
        assert!(t.contains("L3") && t.contains("Obsidian Vault"), "{t}");
        let t = deny_text(run(&d, "gen/out.json"));
        assert!(t.contains("L4") && t.contains("vault の外"), "{t}");
    }

    #[test]
    fn a_repository_without_a_declaration_is_not_touched() {
        let d = repo(None);
        assert_eq!(run(&d, "anything/at/all.md"), None);
    }

    #[test]
    fn a_broken_declaration_fails_open() {
        for text in [
            "not toml [[[",
            "",
            "[[path]]\nglob = \"a\"\nlayer = \"L9\"\n",
        ] {
            let d = repo(Some(text));
            assert_eq!(run(&d, "src/new.rs"), None, "{text:?}");
        }
    }

    #[test]
    fn a_path_hit_by_two_globs_is_not_denied() {
        let d = repo(Some(
            "[[path]]\nglob = \"a/**\"\nlayer = \"L3\"\n[[path]]\nglob = \"a/b/**\"\nlayer = \"L1\"\n",
        ));
        assert_eq!(run(&d, "a/b/x"), None);
        assert!(run(&d, "a/y").is_some(), "重ならない L3 は拒否のまま");
    }

    #[test]
    fn gitignored_paths_and_the_git_dir_are_skipped() {
        let d = repo(Some(DECL));
        assert_eq!(run(&d, "target/debug/x"), None);
        assert_eq!(run(&d, ".git/hooks/pre-commit"), None);
    }

    #[test]
    fn a_path_outside_any_repository_is_skipped() {
        let d = repo(Some(DECL));
        let elsewhere = tempfile::tempdir().unwrap();
        let input = write_input(
            &elsewhere.path().join("note.md").to_string_lossy(),
            d.path(),
        );
        assert_eq!(
            hook(&input),
            None,
            "cwd の宣言で、別の場所への Write を裁かない"
        );
    }

    #[test]
    fn a_relative_path_is_resolved_against_cwd_and_dots_are_folded() {
        let d = repo(Some(DECL));
        let input = write_input("docs/../src/x.rs", d.path());
        assert!(deny_text(hook(&input)).contains("src/x.rs"));
        let input = write_input("./docs/a.md", d.path());
        assert_eq!(hook(&input), None);
    }

    #[test]
    fn other_tools_and_events_are_ignored() {
        let d = repo(Some(DECL));
        let edit = serde_json::json!({
            "hook_event_name": "PreToolUse", "tool_name": "Edit", "session_id": "s",
            "tool_input": {"file_path": d.path().join("src/x.rs")}, "cwd": d.path(),
        })
        .to_string();
        assert_eq!(hook(&edit), None, "拒否は Write だけ");
        let post = write_input(&d.path().join("src/x.rs").to_string_lossy(), d.path())
            .replace("PreToolUse", "PostToolUse");
        assert_eq!(hook(&post), None);
        assert_eq!(hook("not json"), None);
    }

    #[test]
    fn a_linked_worktree_reads_its_own_declaration() {
        let d = repo(Some(DECL));
        git(d.path(), &["add", "-A"]);
        git(
            d.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        );
        let wt = tempfile::tempdir().unwrap();
        let wt_path = wt.path().join("wt");
        git(
            d.path(),
            &[
                "worktree",
                "add",
                "-q",
                wt_path.to_str().unwrap(),
                "-b",
                "b",
            ],
        );
        let input = write_input(&wt_path.join("src/x.rs").to_string_lossy(), &wt_path);
        assert!(deny_text(hook(&input)).contains("src/x.rs"));
        assert_eq!(
            hook(&write_input(
                &wt_path.join("docs/a.md").to_string_lossy(),
                &wt_path
            )),
            None
        );
    }
}
