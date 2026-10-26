//! settings.json を「jq が保つ挿入順のまま」読み書きするための小さな補助。
//!
//! bash 版は `jq '.env.X = ... | del(...)'` で書き換えており、jq は入力のキー順を
//! 保つ。パーサは `jq_json_compat::jqparse`(数値は書かれた表記のまま保つ
//! [`Numbers::Verbatim`] — 利用者の `~/.claude/settings.json` の数値の表記を
//! 変えないため)、値型は `jq_json_compat::jqfmt::J`(挿入順を保つ)が持つ。ここには、
//! オブジェクトのキーを引く・設定する・削除する補助だけが残る。

use jq_json_compat::jqfmt::J;
use jq_json_compat::jqparse;
use jq_json_compat::jqparse::Numbers;

pub fn parse(text: &str) -> Result<J, String> {
    jqparse::parse(text, Numbers::Verbatim).map_err(|e| e.to_string())
}

/// オブジェクトのキーを引く(無ければ None)。
pub fn get<'a>(v: &'a J, key: &str) -> Option<&'a J> {
    match v {
        J::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// オブジェクトのキーを設定する(既存なら位置を保って置換、無ければ末尾に追加)。
pub fn set(pairs: &mut Vec<(String, J)>, key: &str, val: J) {
    match pairs.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = val,
        None => pairs.push((key.to_string(), val)),
    }
}

/// オブジェクトのキーを削除する。
pub fn del(pairs: &mut Vec<(String, J)>, key: &str) {
    pairs.retain(|(k, _)| k != key);
}
