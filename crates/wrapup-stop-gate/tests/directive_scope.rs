//! ADR-625 Amendment(#624): `<hook-directive>` 外枠 + 英語本文の書式は、
//! wrapup 系の出力(このクレート)だけに限る。他のクレートが同じ外枠を出し始めたら
//! fail する — 展開するなら、ADR-625 を更新したうえで、この許可リストを広げる。
//!
//! 他 hook の deny 理由・各 SessionStart は日本語のまま(文言依存の selftest が
//! 約 70 箇所あり、効果の証拠が弱い、というのが評価の結論)。

use std::fs;
use std::path::{Path, PathBuf};

/// 外枠を出してよいクレート(ディレクトリ名)。
const ALLOWED: &[&str] = &["wrapup-stop-gate"];

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn hook_directive_stays_within_wrapup_crate() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&crates).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if ALLOWED.contains(&name.as_str()) {
            continue;
        }
        let mut files = Vec::new();
        rust_sources(&entry.path().join("src"), &mut files);
        for f in files {
            // このファイル自身(別クレートの tests/)は src の外なので対象外。
            if fs::read_to_string(&f).is_ok_and(|s| s.contains("<hook-directive")) {
                offenders.push(f.strip_prefix(&crates).unwrap().display().to_string());
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "`<hook-directive>` を出してよいのは wrapup 系だけです(ADR-625 Amendment)。\
         展開するなら ADR-625 を更新して ALLOWED を広げてください: {offenders:?}"
    );
}
