//! `rust-migration.toml`(移植対象の allowlist、#389/#391)と実体の突き合わせ。
//!
//! 検査は [`audit`] が行い、`tests/allowlist.rs` がリポジトリ本体に対して
//! 呼ぶ(`cargo test` の一部として CI で走る)。

use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub scan: Scan,
    pub limits: Limits,
    #[serde(default)]
    pub target: Vec<Target>,
    #[serde(default)]
    pub inline: Vec<Inline>,
    #[serde(default)]
    pub excluded: Vec<Excluded>,
}

#[derive(Debug, Deserialize)]
pub struct Scan {
    pub dirs: Vec<String>,
    #[serde(default)]
    pub extra: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Limits {
    pub max_remaining: usize,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    pub path: String,
    pub stage: String,
}

#[derive(Debug, Deserialize)]
pub struct Inline {
    pub name: String,
    pub stage: String,
}

#[derive(Debug, Deserialize)]
pub struct Excluded {
    pub path: String,
    pub reason: String,
}

/// 検査結果。`problems` が空なら合格。
#[derive(Debug, Default)]
pub struct Report {
    pub remaining: usize,
    pub max_remaining: usize,
    pub problems: Vec<String>,
}

pub fn load(root: &Path) -> Result<Manifest, String> {
    let p = root.join("rust-migration.toml");
    let s = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    toml::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
}

/// スキャン対象ディレクトリ配下の通常ファイル(再帰)を、root 相対パスで返す。
fn scanned_files(root: &Path, dirs: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.iter().map(|d| root.join(d)).collect();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
        }
    }
    out.sort();
    out
}

/// home/ 配下の nix から、本体が `''` リテラルの `writeShellScript "<name>"` を集める。
/// `(builtins.readFile ...)` のようにファイルを読む形は scripts/ 側で数えるので除く。
pub fn inline_shell_scripts(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for f in scanned_files(root, &["home".to_string()]) {
        if !f.ends_with(".nix") {
            continue;
        }
        let Ok(src) = fs::read_to_string(root.join(&f)) else {
            continue;
        };
        let mut rest = src.as_str();
        while let Some(i) = rest.find("writeShellScript \"") {
            rest = &rest[i + "writeShellScript \"".len()..];
            let Some(end) = rest.find('"') else { break };
            let name = &rest[..end];
            let after = rest[end + 1..].trim_start();
            if after.starts_with("''") {
                names.insert(name.to_string());
            }
        }
    }
    names
}

pub fn audit(root: &Path, m: &Manifest) -> Report {
    let mut r = Report {
        max_remaining: m.limits.max_remaining,
        ..Default::default()
    };

    let mut classified = BTreeSet::new();
    for (path, kind) in m
        .target
        .iter()
        .map(|t| (&t.path, "target"))
        .chain(m.excluded.iter().map(|e| (&e.path, "excluded")))
    {
        if !classified.insert(path.clone()) {
            r.problems.push(format!("{path}: 重複して分類されている"));
        }
        if !root.join(path).is_file() {
            r.problems.push(format!(
                "{path}: {kind} に載っているが存在しない(移植済みなら行を消し、max_remaining を下げる)"
            ));
        }
    }
    for e in &m.excluded {
        if e.reason.trim().is_empty() {
            r.problems
                .push(format!("{}: excluded に理由が無い", e.path));
        }
    }
    for t in &m.target {
        if t.stage.trim().is_empty() {
            r.problems
                .push(format!("{}: target に stage が無い", t.path));
        }
    }

    let mut seen: Vec<String> = scanned_files(root, &m.scan.dirs);
    seen.extend(m.scan.extra.iter().cloned());
    for f in &seen {
        if !classified.contains(f) {
            r.problems.push(format!(
                "{f}: rust-migration.toml で未分類(新しい hook/CLI は Rust 既定 — ADR-0024。対象外なら [[excluded]] に理由付きで載せる)"
            ));
        }
    }

    let declared: BTreeSet<String> = m.inline.iter().map(|i| i.name.clone()).collect();
    let actual = inline_shell_scripts(root);
    for n in actual.difference(&declared) {
        r.problems.push(format!(
            "writeShellScript \"{n}\": nix 埋め込みのシェルが [[inline]] に未宣言"
        ));
    }
    for n in declared.difference(&actual) {
        r.problems.push(format!(
            "[[inline]] {n}: 宣言されているが home/ に見当たらない(移植済みなら行を消し、max_remaining を下げる)"
        ));
    }

    r.remaining = m.target.len() + m.inline.len();
    if r.remaining > r.max_remaining {
        r.problems.push(format!(
            "残り {} 件が max_remaining = {} を超えている",
            r.remaining, r.max_remaining
        ));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn manifest(s: &str) -> Manifest {
        toml::from_str(s).unwrap()
    }

    const BASE: &str = r#"
[scan]
dirs = ["hooks"]
[limits]
max_remaining = 2
[[target]]
path = "hooks/a.sh"
stage = "4a"
[[inline]]
name = "reg"
stage = "4e"
[[excluded]]
path = "hooks/b.sh"
reason = "escape hatch"
"#;

    fn fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "hooks/a.sh", "#!/bin/sh\n");
        write(d.path(), "hooks/b.sh", "#!/bin/sh\n");
        write(
            d.path(),
            "home/m.nix",
            "x = pkgs.writeShellScript \"reg\" ''\n echo\n'';\ny = pkgs.writeShellScript \"file\" (builtins.readFile ./f);\n",
        );
        d
    }

    #[test]
    fn clean() {
        let d = fixture();
        let r = audit(d.path(), &manifest(BASE));
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.remaining, 2);
    }

    #[test]
    fn unclassified_new_file() {
        let d = fixture();
        write(d.path(), "hooks/new.sh", "#!/bin/sh\n");
        let r = audit(d.path(), &manifest(BASE));
        assert!(r.problems.iter().any(|p| p.starts_with("hooks/new.sh")));
    }

    #[test]
    fn ported_target_left_behind() {
        let d = fixture();
        fs::remove_file(d.path().join("hooks/a.sh")).unwrap();
        let r = audit(d.path(), &manifest(BASE));
        assert!(r.problems.iter().any(|p| p.contains("hooks/a.sh")));
    }

    #[test]
    fn undeclared_inline() {
        let d = fixture();
        write(
            d.path(),
            "home/n.nix",
            "z = pkgs.writeShellScript \"sneaky\" ''\n true\n'';\n",
        );
        let r = audit(d.path(), &manifest(BASE));
        assert!(r.problems.iter().any(|p| p.contains("sneaky")));
    }

    #[test]
    fn over_limit() {
        let d = fixture();
        let m = manifest(&BASE.replace("max_remaining = 2", "max_remaining = 1"));
        let r = audit(d.path(), &m);
        assert!(r.problems.iter().any(|p| p.contains("max_remaining")));
    }
}
