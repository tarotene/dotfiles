//! `~/.codex/config.toml` の reconcile。
//!
//! このファイルは Codex 自身が実行時に書き戻す(hook trust の `hooks.state.*`、
//! project trust、モデル選択)ので、home-manager は read-only symlink にしない
//! (ADR-457 Amendment)。宣言したキーだけを常に宣言値へ合わせ、ほかのキー・
//! コメント・並び・空行には触らない(`toml_edit` が書式を保つ)。
//!
//! spec は「ドット区切りのキー → 値」のオブジェクト(JSON)。値は文字列・整数・
//! 真偽値と、それらだけの配列。キーの各区間は引用なしの名前に限る(ドットを含む
//! キー名は宣言できない — 今回の用途では要らない)。

use crate::json::write_atomic;
use serde_json::Value as Spec;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, Value};

/// `spec_json` は `{"tui.terminal_title": ["activity", ...], ...}`。
pub fn apply(path: &Path, spec_json: &str) -> Result<(), String> {
    let spec: Spec = serde_json::from_str(spec_json).map_err(|e| format!("spec-json: {e}"))?;
    let Spec::Object(spec) = spec else {
        return Err("spec-json: JSON オブジェクトが必要".into());
    };

    let before = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut doc: DocumentMut = before
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;

    for (dotted, want) in &spec {
        set_dotted(&mut doc, dotted, want)
            .map_err(|e| format!("{}: {dotted}: {e}", path.display()))?;
    }

    let after = doc.to_string();
    if after == before {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_atomic(path, &after)
}

fn set_dotted(doc: &mut DocumentMut, dotted: &str, want: &Spec) -> Result<(), String> {
    let segs: Vec<&str> = dotted.split('.').collect();
    if segs.iter().any(|s| s.is_empty()) {
        return Err("キーに空の区間がある".into());
    }
    let (leaf, parents) = segs.split_last().expect("split は 1 要素以上を返す");

    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for seg in parents {
        if !table.contains_key(seg) {
            // 途中の表は implicit にして、空の `[a]` 見出しを出さない。
            let mut t = Table::new();
            t.set_implicit(true);
            table.insert(seg, Item::Table(t));
        }
        table = table
            .get_mut(seg)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| format!("`{seg}` が表ではない"))?;
    }

    let new = to_toml(want)?;
    match table.get_mut(leaf) {
        Some(Item::Value(old)) => {
            if same(old, want) {
                return Ok(());
            }
            // 行末コメントなどの装飾は引き継ぐ。
            let mut new = new;
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        Some(_) => return Err("既存の値が単一の値ではない(表・表の配列)".into()),
        None => {
            table.insert(leaf, Item::Value(new));
        }
    }
    Ok(())
}

fn to_toml(v: &Spec) -> Result<Value, String> {
    match v {
        Spec::String(s) => Ok(Value::from(s.as_str())),
        Spec::Bool(b) => Ok(Value::from(*b)),
        Spec::Number(n) => n
            .as_i64()
            .map(Value::from)
            .ok_or_else(|| format!("整数だけ扱える: {n}")),
        Spec::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                if matches!(x, Spec::Array(_) | Spec::Object(_)) {
                    return Err("配列の要素は文字列・整数・真偽値に限る".into());
                }
                arr.push(to_toml(x)?);
            }
            Ok(Value::Array(arr))
        }
        Spec::Null | Spec::Object(_) => Err("null・オブジェクトは宣言できない".into()),
    }
}

/// 書式(空白・クォートの種類)に左右されず、値として同じかを見る。
fn same(have: &Value, want: &Spec) -> bool {
    match (have, want) {
        (Value::String(s), Spec::String(w)) => s.value() == w,
        (Value::Boolean(b), Spec::Bool(w)) => b.value() == w,
        (Value::Integer(i), Spec::Number(w)) => w.as_i64() == Some(*i.value()),
        (Value::Array(a), Spec::Array(w)) => {
            a.len() == w.len() && a.iter().zip(w).all(|(h, w)| same(h, w))
        }
        _ => false,
    }
}
