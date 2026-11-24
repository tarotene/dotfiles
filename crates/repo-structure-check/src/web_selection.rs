//! 選び方の対応表の不変条件: `config/web/selection.toml` は、作業の種類ごとに、ブラウザ標準・共有の部品・
//! 外部ライブラリ・手書き・使わない、のどれを選ぶかを、閉じた語彙で持つ(ADR-1703)。
//!
//! - 表のキーと値は閉じている。知らないキー・語彙の外の `way`・`reason`・重複した `id` は落ちる。
//! - `platform` の行は、`baseline = "widely"` を持つ。Widely available でない機能は、標準の行にできない。
//! - `shared` の行は、`config/web/` の下の、実在するファイルを `files` に持つ。
//! - `external` の行は、版(`pin`)と、同梱するファイルの `sha256`(64 桁の 16 進)を持つ。
//! - `not-used` の行は、`reason` が `requirement` なら、`page-types.toml` に実在する要件の ID を `req` に持ち、
//!   `no-surface` なら `req` を持たない。
//! - `handwritten` の行は、`reason = "few-surfaces"` を持つ。
//! - 行ごとに、書けるキーが決まっている(`platform` の行に `files` を書けない、など)。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use toml::{Table, Value};

use crate::manifest::Violation;

/// 対応表のパス(リポジトリの根から)。
pub const SELECTION_PATH: &str = "config/web/selection.toml";
/// 要件の一覧のパス(`not-used` の `req` の突き合わせ先)。
pub const PAGE_TYPES_PATH: &str = "config/web/page-types.toml";

/// 選び方(`way`)の語彙。
const WAYS: [&str; 5] = ["platform", "shared", "external", "handwritten", "not-used"];
/// 理由(`reason`)の語彙。
const REASONS: [&str; 3] = ["requirement", "no-surface", "few-surfaces"];
/// 行が持てるキーの全体。
const TASK_KEYS: [&str; 11] = [
    "id", "title", "way", "use", "baseline", "files", "pin", "sha256", "reason", "req", "note",
];

fn violation(path: &str, detail: String) -> Violation {
    Violation {
        subject: path.to_string(),
        detail,
    }
}

/// `way` ごとの、必須のキーと、書いてよいキー(必須を除く)。
fn keys_of(way: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match way {
        "platform" => (&["use", "baseline"], &["note"]),
        "shared" => (&["use", "files"], &["note"]),
        "external" => (&["use", "pin", "sha256"], &["note"]),
        "handwritten" => (&["reason"], &["use", "note"]),
        // `not-used` の `req` は、`reason = "requirement"` のときだけ(別に検査する)。
        _ => (&["reason"], &["req", "note"]),
    }
}

fn string_of<'a>(task: &'a Table, key: &str) -> Option<&'a str> {
    match task.get(key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn is_kebab(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// 文字列だけの配列を取る。配列でない・文字列でない要素があれば `None`。
fn string_array(task: &Table, key: &str) -> Option<Vec<String>> {
    let Some(Value::Array(items)) = task.get(key) else {
        return None;
    };
    items
        .iter()
        .map(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// 要件の一覧(`page-types.toml`)から、要件の ID の集合を取る。
pub fn requirement_ids(page_types: &str) -> Result<BTreeSet<String>, String> {
    let table: Table = page_types
        .parse()
        .map_err(|e| format!("{PAGE_TYPES_PATH} を TOML として読めない: {e}"))?;
    let mut out = BTreeSet::new();
    let Some(Value::Array(types)) = table.get("type") else {
        return Err(format!("{PAGE_TYPES_PATH} に `[[type]]` が無い"));
    };
    for t in types {
        let Value::Table(t) = t else { continue };
        let Some(Value::Array(reqs)) = t.get("requirement") else {
            continue;
        };
        for r in reqs {
            if let Value::Table(r) = r {
                if let Some(id) = string_of(r, "id") {
                    out.insert(id.to_string());
                }
            }
        }
    }
    Ok(out)
}

/// 対応表の文字列を検査する。`requirement_ids` は要件の ID の集合、`exists` は、リポジトリの根からの
/// 相対パスが実在するファイルかを答える。
pub fn check_selection(
    path: &str,
    text: &str,
    requirement_ids: &BTreeSet<String>,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<Violation> {
    let table: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return vec![violation(path, format!("TOML として読めない: {e}"))],
    };
    let mut out = Vec::new();
    for key in table.keys() {
        if key != "task" {
            out.push(violation(
                path,
                format!("知らないキー `{key}` がある(表の先頭に書けるのは `[[task]]` だけ)"),
            ));
        }
    }
    let Some(Value::Array(tasks)) = table.get("task") else {
        out.push(violation(path, "`[[task]]` が 1 つも無い".to_string()));
        return out;
    };
    if tasks.is_empty() {
        out.push(violation(path, "`[[task]]` が 1 つも無い".to_string()));
    }

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (i, task) in tasks.iter().enumerate() {
        let Value::Table(task) = task else {
            out.push(violation(
                path,
                format!("{} 番目の `task` が表でない", i + 1),
            ));
            continue;
        };
        let id = string_of(task, "id").unwrap_or("");
        let label = if id.is_empty() {
            format!("{} 番目の task", i + 1)
        } else {
            format!("task `{id}`")
        };
        let mut bad = |detail: String| out.push(violation(path, format!("{label}: {detail}")));

        for key in task.keys() {
            if !TASK_KEYS.contains(&key.as_str()) {
                bad(format!("知らないキー `{key}` がある"));
            }
        }
        if id.is_empty() {
            bad("`id` が無い(文字列で書く)".to_string());
        } else {
            if !is_kebab(id) {
                bad("`id` は小文字・数字・`-` だけで、小文字で始める".to_string());
            }
            if !seen.insert(id.to_string()) {
                bad("`id` が重複している".to_string());
            }
        }
        if string_of(task, "title").is_none_or(|t| t.trim().is_empty()) {
            bad("`title` が無い、または空".to_string());
        }
        let Some(way) = string_of(task, "way") else {
            bad("`way` が無い".to_string());
            continue;
        };
        if !WAYS.contains(&way) {
            bad(format!("`way = \"{way}\"` は語彙の外(使えるのは {WAYS:?})"));
            continue;
        }

        let (required, optional) = keys_of(way);
        for key in required {
            if !task.contains_key(*key) {
                bad(format!("`way = \"{way}\"` の行には `{key}` が要る"));
            }
        }
        for key in task.keys() {
            let k = key.as_str();
            let base = ["id", "title", "way"].contains(&k);
            if TASK_KEYS.contains(&k) && !base && !required.contains(&k) && !optional.contains(&k) {
                bad(format!("`{key}` は、`way = \"{way}\"` の行には書けない"));
            }
        }
        // 文字列であるべき欄が文字列でない形も落とす。
        for key in ["use", "note", "pin"] {
            if task.contains_key(key) && string_of(task, key).is_none_or(|s| s.trim().is_empty()) {
                bad(format!("`{key}` は、空でない文字列で書く"));
            }
        }

        match way {
            "platform" => {
                let baseline = string_of(task, "baseline").unwrap_or("");
                if baseline != "widely" {
                    bad(format!(
                        "`baseline = \"{baseline}\"`: Baseline が widely でない機能は、platform の行にできない(使えるのは `widely` だけ)"
                    ));
                }
            }
            "shared" => match string_array(task, "files") {
                Some(files) if !files.is_empty() => {
                    for f in files {
                        if !f.starts_with("config/web/") {
                            bad(format!(
                                "`files` の `{f}` は、`config/web/` の下のファイルでない"
                            ));
                        } else if !exists(&f) {
                            bad(format!("`files` の `{f}` が実在しない"));
                        }
                    }
                }
                _ => bad("`files` は、空でない文字列の配列で書く".to_string()),
            },
            "external" => {
                if !string_of(task, "sha256").is_some_and(is_sha256) {
                    bad("`sha256` は、64 桁の小文字の 16 進で書く".to_string());
                }
            }
            "handwritten" => {
                let reason = string_of(task, "reason").unwrap_or("");
                if reason != "few-surfaces" {
                    bad(format!(
                        "`reason = \"{reason}\"`: handwritten の理由は `few-surfaces` だけ"
                    ));
                }
            }
            _ => {
                let reason = string_of(task, "reason").unwrap_or("");
                if !REASONS.contains(&reason) || reason == "few-surfaces" {
                    bad(format!(
                        "`reason = \"{reason}\"`: not-used の理由は `requirement` か `no-surface`"
                    ));
                } else if reason == "requirement" {
                    match string_array(task, "req") {
                        Some(reqs) if !reqs.is_empty() => {
                            for r in reqs {
                                if !requirement_ids.contains(&r) {
                                    bad(format!(
                                        "`req` の `{r}` が `{PAGE_TYPES_PATH}` に実在しない"
                                    ));
                                }
                            }
                        }
                        _ => bad(
                            "`reason = \"requirement\"` の行には、要件の ID の配列 `req` が要る"
                                .to_string(),
                        ),
                    }
                } else if task.contains_key("req") {
                    bad("`reason = \"no-surface\"` の行に `req` は書けない".to_string());
                }
            }
        }
    }
    out
}

/// ツリーの対応表を、要件の一覧と、共有のファイルの実在に突き合わせて検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    let text = read(SELECTION_PATH)?;
    let ids = requirement_ids(&read(PAGE_TYPES_PATH)?)?;
    Ok(check_selection(SELECTION_PATH, &text, &ids, &|rel| {
        root.join(rel).is_file()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[[task]]
id = "dialog"
title = "ダイアログ"
way = "platform"
use = "`<dialog>`"
baseline = "widely"

[[task]]
id = "buttons"
title = "ボタン"
way = "shared"
use = "`.btn`"
files = ["config/web/components.css"]

[[task]]
id = "chart-lib"
title = "グラフ(外部)"
way = "external"
use = "chart"
pin = "1.2.3"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"

[[task]]
id = "tabs"
title = "タブ"
way = "handwritten"
reason = "few-surfaces"

[[task]]
id = "toast"
title = "トースト"
way = "not-used"
reason = "requirement"
req = ["F3"]

[[task]]
id = "drawer"
title = "引き出し"
way = "not-used"
reason = "no-surface"
"#;

    fn ids() -> BTreeSet<String> {
        ["F3", "C8"].iter().map(|s| s.to_string()).collect()
    }

    fn details(text: &str) -> String {
        check_selection(SELECTION_PATH, text, &ids(), &|p| {
            p == "config/web/components.css"
        })
        .iter()
        .map(|v| v.detail.clone())
        .collect::<Vec<_>>()
        .join("\n")
    }

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn a_table_with_every_way_is_clean() {
        assert_eq!(details(GOOD), "");
    }

    #[test]
    fn the_real_table_passes() {
        let vs = check_tree(&root()).expect("read the tree");
        let report: Vec<String> = vs.iter().map(ToString::to_string).collect();
        assert!(vs.is_empty(), "{}", report.join("\n"));
    }

    #[test]
    fn a_way_outside_the_vocabulary_is_reported() {
        let d = details(&GOOD.replace("way = \"platform\"", "way = \"library\""));
        assert!(d.contains("語彙の外"), "{d}");
    }

    #[test]
    fn a_platform_row_must_be_widely_available() {
        let d = details(&GOOD.replace("baseline = \"widely\"", "baseline = \"newly\""));
        assert!(
            d.contains("widely でない機能は、platform の行にできない"),
            "{d}"
        );
    }

    #[test]
    fn a_platform_row_without_a_baseline_is_reported() {
        let d = details(&GOOD.replace("baseline = \"widely\"\n", ""));
        assert!(d.contains("`baseline` が要る"), "{d}");
    }

    #[test]
    fn a_shared_row_must_name_an_existing_file_under_config_web() {
        let missing = details(&GOOD.replace("config/web/components.css", "config/web/gone.css"));
        assert!(missing.contains("が実在しない"), "{missing}");
        let outside = details(&GOOD.replace("config/web/components.css", "docs/a.css"));
        assert!(
            outside.contains("`config/web/` の下のファイルでない"),
            "{outside}"
        );
    }

    #[test]
    fn an_external_row_needs_a_pin_and_a_sha256() {
        let no_pin = details(&GOOD.replace("pin = \"1.2.3\"\n", ""));
        assert!(no_pin.contains("`pin` が要る"), "{no_pin}");
        let bad_hash = details(&GOOD.replace(
            "sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"",
            "sha256 = \"ABC\"",
        ));
        assert!(bad_hash.contains("64 桁"), "{bad_hash}");
    }

    #[test]
    fn a_requirement_reason_needs_known_requirement_ids() {
        let unknown = details(&GOOD.replace("req = [\"F3\"]", "req = [\"F99\"]"));
        assert!(unknown.contains("`F99` が"), "{unknown}");
        let none = details(&GOOD.replace("req = [\"F3\"]\n", ""));
        assert!(none.contains("`req` が要る"), "{none}");
    }

    #[test]
    fn a_no_surface_row_cannot_carry_requirements() {
        let d = details(&GOOD.replace(
            "reason = \"no-surface\"",
            "reason = \"no-surface\"\nreq = [\"F3\"]",
        ));
        assert!(d.contains("`req` は書けない"), "{d}");
    }

    #[test]
    fn a_handwritten_row_needs_its_reason() {
        let d = details(&GOOD.replace("reason = \"few-surfaces\"", "reason = \"no-surface\""));
        assert!(d.contains("few-surfaces"), "{d}");
    }

    #[test]
    fn a_key_the_way_does_not_allow_is_reported() {
        let d = details(&GOOD.replace(
            "baseline = \"widely\"",
            "baseline = \"widely\"\nfiles = [\"config/web/components.css\"]",
        ));
        assert!(
            d.contains("`files` は、`way = \"platform\"` の行には書けない"),
            "{d}"
        );
    }

    #[test]
    fn unknown_keys_duplicate_ids_and_bad_ids_are_reported() {
        let unknown =
            details(&GOOD.replace("title = \"タブ\"", "title = \"タブ\"\ncolor = \"red\""));
        assert!(unknown.contains("知らないキー `color`"), "{unknown}");
        let dup = details(&GOOD.replace("id = \"drawer\"", "id = \"toast\""));
        assert!(dup.contains("`id` が重複"), "{dup}");
        let bad = details(&GOOD.replace("id = \"drawer\"", "id = \"Drawer_1\""));
        assert!(bad.contains("小文字・数字・`-`"), "{bad}");
        let top = details(&format!("version = 1\n{GOOD}"));
        assert!(top.contains("知らないキー `version`"), "{top}");
    }

    #[test]
    fn an_empty_or_unparsable_table_is_reported() {
        assert!(details("").contains("`[[task]]` が 1 つも無い"));
        assert!(details("[[task").contains("TOML として読めない"));
    }

    #[test]
    fn requirement_ids_are_read_from_the_real_page_types() {
        let text = fs::read_to_string(root().join(PAGE_TYPES_PATH)).expect("read page types");
        let ids = requirement_ids(&text).expect("parse");
        for expected in ["C1", "C12", "F3", "F4", "D6", "L8"] {
            assert!(
                ids.contains(expected),
                "{expected} が読まれていない: {ids:?}"
            );
        }
    }
}
