//! manifest の不変条件: 外部依存の版は root の `[workspace.dependencies]` が唯一の正本。
//!
//! - メンバーの外部依存は `name.workspace = true`(feature を足すときは
//!   `{ workspace = true, features = [...] }`)で継承する。`path` の依存(workspace 内の crate)は対象外。
//! - root の表の各項目は、`version` と `default-features` だけを持つ(feature はメンバーが足す)。
//!   どのメンバーも使っていない項目は残さない。
//! - 例外は [`ZERO_DEPENDENCY_CRATES`] だけ。他リポジトリの CI が 1 crate ずつ sparse-checkout して
//!   `cargo build` するので、標準ライブラリだけに保つ(`[dependencies]` と `[build-dependencies]` を空にする。
//!   `[dev-dependencies]` は明示の指定を許す)。

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use toml::{Table, Value};

/// 他リポジトリの CI が、1 crate ずつ sparse-checkout して `cargo build` する crate
/// (`.github/actions/pr-title/action.yml`)。標準ライブラリだけに保つ。
pub const ZERO_DEPENDENCY_CRATES: &[&str] = &["pr-title-check", "pr-contract-check"];

const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// root の `[workspace.dependencies]` の項目が持ってよいキー。
const WORKSPACE_ENTRY_KEYS: [&str; 3] = ["version", "default-features", "path"];

/// 不変条件の違反 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// 違反の主語(crate 名、または root)。
    pub subject: String,
    /// 何が、なぜ違反か。
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.subject, self.detail)
    }
}

fn violation(subject: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: subject.to_string(),
        detail: detail.into(),
    }
}

/// manifest の依存表を、(表の名前, 表)で列挙する。`[target.<cfg>.<表>]` も含める。
fn dependency_tables(manifest: &Table) -> Vec<(String, &Table)> {
    let mut out = Vec::new();
    for name in DEP_TABLES {
        if let Some(Value::Table(t)) = manifest.get(name) {
            out.push((name.to_string(), t));
        }
    }
    if let Some(Value::Table(targets)) = manifest.get("target") {
        for (cfg, target) in targets {
            let Value::Table(target) = target else {
                continue;
            };
            for name in DEP_TABLES {
                if let Some(Value::Table(t)) = target.get(name) {
                    out.push((format!("target.{cfg}.{name}"), t));
                }
            }
        }
    }
    out
}

fn inherits(spec: &Value) -> bool {
    matches!(spec, Value::Table(t) if t.get("workspace") == Some(&Value::Boolean(true)))
}

fn is_internal(spec: &Value) -> bool {
    matches!(spec, Value::Table(t) if t.contains_key("path"))
}

/// メンバー 1 つの manifest を検査する。
pub fn check_member(crate_name: &str, text: &str) -> Vec<Violation> {
    let manifest: Table = match text.parse() {
        Ok(m) => m,
        Err(e) => return vec![violation(crate_name, format!("manifest を読めない: {e}"))],
    };
    let zero = ZERO_DEPENDENCY_CRATES.contains(&crate_name);
    let mut out = Vec::new();
    for (table_name, table) in dependency_tables(&manifest) {
        if zero {
            if !table_name.ends_with("dev-dependencies") && !table.is_empty() {
                let names: Vec<&str> = table.keys().map(String::as_str).collect();
                out.push(violation(
                    crate_name,
                    format!(
                        "[{table_name}] は空に保つ(他リポジトリの CI が 1 crate だけを sparse-checkout して build する): {}",
                        names.join(", ")
                    ),
                ));
            }
            continue;
        }
        for (dep, spec) in table {
            if !inherits(spec) && !is_internal(spec) {
                out.push(violation(
                    crate_name,
                    format!(
                        "[{table_name}] の `{dep}` は root の [workspace.dependencies] から継承する(`{dep}.workspace = true`)"
                    ),
                ));
            }
        }
    }
    out
}

/// メンバーが `workspace = true` で継承している依存の名前。
fn inherited_names(manifest: &Table) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, table) in dependency_tables(manifest) {
        for (dep, spec) in table {
            if inherits(spec) {
                out.insert(dep.clone());
            }
        }
    }
    out
}

/// root の `[workspace.dependencies]` を、全メンバーの manifest と突き合わせて検査する。
pub fn check_root(root_text: &str, members: &[(String, String)]) -> Vec<Violation> {
    let root: Table = match root_text.parse() {
        Ok(m) => m,
        Err(e) => return vec![violation("root", format!("Cargo.toml を読めない: {e}"))],
    };
    let Some(declared) = root
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table)
    else {
        return vec![violation("root", "[workspace.dependencies] が無い")];
    };
    let mut out = Vec::new();
    for (name, spec) in declared {
        match spec {
            Value::String(_) => {}
            Value::Table(t) => {
                for key in t.keys() {
                    if !WORKSPACE_ENTRY_KEYS.contains(&key.as_str()) {
                        out.push(violation(
                            "root",
                            format!(
                                "[workspace.dependencies] の `{name}` が `{key}` を持つ(root は version と default-features だけを持ち、feature はメンバーが足す)"
                            ),
                        ));
                    }
                }
            }
            _ => out.push(violation(
                "root",
                format!("[workspace.dependencies] の `{name}` の書式が不正"),
            )),
        }
    }
    let mut used = BTreeSet::new();
    for (_, text) in members {
        if let Ok(manifest) = text.parse::<Table>() {
            used.extend(inherited_names(&manifest));
        }
    }
    for name in declared.keys() {
        if !used.contains(name) {
            out.push(violation(
                "root",
                format!("[workspace.dependencies] の `{name}` を、どのメンバーも使っていない(使わない依存は残さない)"),
            ));
        }
    }
    out
}

/// `crates/*/Cargo.toml`(`.` で始まる名前を除く。root の `members` の glob と同じ)を、名前順に読む。
pub fn read_members(root: &Path) -> Result<Vec<(String, String)>, String> {
    let dir = root.join("crates");
    let entries = fs::read_dir(&dir).map_err(|e| format!("{} を読めない: {e}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().join("Cargo.toml").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let path = dir.join(&n).join("Cargo.toml");
            fs::read_to_string(&path)
                .map(|text| (n, text))
                .map_err(|e| format!("{} を読めない: {e}", path.display()))
        })
        .collect()
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let root_path = root.join("Cargo.toml");
    let root_text = fs::read_to_string(&root_path)
        .map_err(|e| format!("{} を読めない: {e}", root_path.display()))?;
    let members = read_members(root)?;
    let mut out = check_root(&root_text, &members);
    for (name, text) in &members {
        out.extend(check_member(name, text));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details(vs: &[Violation]) -> Vec<String> {
        vs.iter().map(ToString::to_string).collect()
    }

    // ---- メンバー: 正例 ----

    #[test]
    fn inherited_and_path_dependencies_pass() {
        let text = r#"
[package]
name = "x"
[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
other-crate = { path = "../other-crate" }
[dev-dependencies]
tempfile.workspace = true
"#;
        assert_eq!(check_member("x", text), vec![]);
    }

    // ---- メンバー: 負例(検査が実際に落ちること) ----

    #[test]
    fn a_plain_version_is_a_violation() {
        let text = "[dependencies]\nserde_json = \"1.0.151\"\n";
        let vs = check_member("x", text);
        assert_eq!(vs.len(), 1, "{:?}", details(&vs));
        assert!(vs[0].detail.contains("`serde_json`"));
    }

    #[test]
    fn an_inline_table_with_a_version_is_a_violation() {
        let text = "[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
        assert_eq!(check_member("x", text).len(), 1);
    }

    #[test]
    fn dev_and_build_and_target_tables_are_checked_too() {
        let text = r#"
[dev-dependencies]
tempfile = "3"
[build-dependencies]
cc = "1"
[target.'cfg(unix)'.dependencies]
libc = "0.2"
"#;
        let vs = check_member("x", text);
        assert_eq!(vs.len(), 3, "{:?}", details(&vs));
    }

    #[test]
    fn workspace_false_is_not_an_inheritance() {
        let text = "[dependencies]\nserde = { workspace = false, version = \"1\" }\n";
        assert_eq!(check_member("x", text).len(), 1);
    }

    #[test]
    fn an_unparsable_manifest_is_a_violation() {
        let vs = check_member("x", "[dependencies\n");
        assert_eq!(vs.len(), 1);
        assert!(vs[0].detail.contains("読めない"));
    }

    // ---- 依存ゼロの crate ----

    #[test]
    fn a_zero_dependency_crate_keeps_its_dependencies_empty() {
        let ok = "[dependencies]\n\n[dev-dependencies]\ntrycmd = \"1.2.1\"\n";
        assert_eq!(check_member("pr-title-check", ok), vec![]);

        let bad = "[dependencies]\nregex = \"1\"\n";
        let vs = check_member("pr-title-check", bad);
        assert_eq!(vs.len(), 1, "{:?}", details(&vs));
        assert!(vs[0].detail.contains("sparse-checkout"));

        let bad_build = "[build-dependencies]\ncc = \"1\"\n";
        assert_eq!(check_member("pr-contract-check", bad_build).len(), 1);
    }

    #[test]
    fn the_exception_applies_only_to_the_named_crates() {
        let text = "[dependencies]\n\n[dev-dependencies]\ntrycmd = \"1.2.1\"\n";
        // 同じ書き方でも、例外の crate でなければ違反になる
        assert_eq!(check_member("some-other-crate", text).len(), 1);
    }

    // ---- root ----

    fn member(deps: &str) -> (String, String) {
        ("m".to_string(), format!("[dependencies]\n{deps}\n"))
    }

    #[test]
    fn a_root_table_of_version_and_default_features_passes() {
        let root = r#"
[workspace.dependencies]
serde = "1"
toml = { version = "1", default-features = false }
"#;
        let members = [member(
            "serde.workspace = true\ntoml = { workspace = true, features = [\"parse\"] }",
        )];
        assert_eq!(check_root(root, &members), vec![]);
    }

    #[test]
    fn a_feature_in_the_root_table_is_a_violation() {
        let root =
            "[workspace.dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
        let members = [member("serde.workspace = true")];
        let vs = check_root(root, &members);
        assert_eq!(vs.len(), 1, "{:?}", details(&vs));
        assert!(vs[0].detail.contains("features"));
    }

    #[test]
    fn an_unused_root_entry_is_a_violation() {
        let root = "[workspace.dependencies]\nserde = \"1\"\nregex = \"1\"\n";
        let members = [member("serde.workspace = true")];
        let vs = check_root(root, &members);
        assert_eq!(vs.len(), 1, "{:?}", details(&vs));
        assert!(vs[0].detail.contains("`regex`"));
    }

    #[test]
    fn a_missing_root_table_is_a_violation() {
        let vs = check_root("[workspace]\nmembers = []\n", &[]);
        assert_eq!(vs.len(), 1);
        assert!(vs[0].detail.contains("無い"));
    }
}
