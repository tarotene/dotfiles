//! キー順を保つ JSON 値と、settings.json 系ファイルの読み書き。
//!
//! `serde_json::Value` は(`preserve_order` feature 無しでは)オブジェクトのキーを
//! 辞書順に並べ替える。feature を有効にするとワークスペース全体で
//! feature unification により他クレートの出力順まで変わるため、この
//! クレートだけの小さな順序保存型を持つ。jq は入力のキー順を保ったまま
//! 書き戻していたので、settings.json を switch のたびに並べ替えて差分を
//! 汚さないために必要。

use indexmap::IndexMap;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Number;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(Number),
    Str(String),
    Arr(Vec<J>),
    Obj(IndexMap<String, J>),
}

impl J {
    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_obj_mut(&mut self) -> Option<&mut IndexMap<String, J>> {
        match self {
            J::Obj(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&IndexMap<String, J>> {
        match self {
            J::Obj(m) => Some(m),
            _ => None,
        }
    }
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) => n.serialize(s),
            J::Str(v) => s.serialize_str(v),
            J::Arr(a) => {
                let mut seq = s.serialize_seq(Some(a.len()))?;
                for v in a {
                    seq.serialize_element(v)?;
                }
                seq.end()
            }
            J::Obj(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

struct JVisitor;

impl<'de> Visitor<'de> for JVisitor {
    type Value = J;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_unit<E>(self) -> Result<J, E> {
        Ok(J::Null)
    }
    fn visit_none<E>(self) -> Result<J, E> {
        Ok(J::Null)
    }
    fn visit_bool<E>(self, v: bool) -> Result<J, E> {
        Ok(J::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<J, E> {
        Ok(J::Num(v.into()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<J, E> {
        Ok(J::Num(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<J, E> {
        Number::from_f64(v)
            .map(J::Num)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E>(self, v: &str) -> Result<J, E> {
        Ok(J::Str(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<J, E> {
        Ok(J::Str(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
        let mut v = Vec::new();
        while let Some(x) = a.next_element()? {
            v.push(x);
        }
        Ok(J::Arr(v))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
        let mut m = IndexMap::new();
        // 重複キーは jq と同じく後勝ち(位置は最初の出現のまま)。
        while let Some((k, v)) = a.next_entry::<String, J>()? {
            m.insert(k, v);
        }
        Ok(J::Obj(m))
    }
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        d.deserialize_any(JVisitor)
    }
}

pub fn parse(text: &str) -> Result<J, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// jq の既定出力と同じ形(2 スペース字下げ、末尾改行)。
pub fn render(v: &J) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "null".into());
    s.push('\n');
    s
}

/// 設定ファイルを読む。無ければ親ディレクトリごと `{}` で作る(bash 版の
/// `mkdir -p` + `printf '{}\n'`)。トップレベルがオブジェクトでなければエラー。
pub fn load(path: &Path) -> Result<J, String> {
    if !path.is_file() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, "{}\n").map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if v.as_obj().is_none() {
        return Err(format!(
            "{}: トップレベルが JSON オブジェクトではない",
            path.display()
        ));
    }
    Ok(v)
}

/// `before` と `after` が同値なら何も書かない(定常状態で mtime を汚さない)。
/// 書くときは同じディレクトリの一時ファイルへ書いて rename する: 別 fs への
/// mv は copy+unlink に落ち、途中で落ちると settings.json が壊れる。mode は
/// 元ファイルに合わせる(取れなければ 0600)。
pub fn store_if_changed(path: &Path, before: &J, after: &J) -> Result<(), String> {
    if before == after {
        return Ok(());
    }
    let tmp = tmp_path(path);
    std::fs::write(&tmp, render(after)).map_err(|e| format!("{}: {e}", tmp.display()))?;
    set_mode_like(path, &tmp);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".hm.{}", std::process::id()));
    PathBuf::from(name)
}

#[cfg(unix)]
fn set_mode_like(orig: &Path, tmp: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let perm = std::fs::metadata(orig)
        .map(|m| m.permissions())
        .unwrap_or_else(|_| std::fs::Permissions::from_mode(0o600));
    let _ = std::fs::set_permissions(tmp, perm);
}

#[cfg(not(unix))]
fn set_mode_like(_orig: &Path, _tmp: &Path) {}
