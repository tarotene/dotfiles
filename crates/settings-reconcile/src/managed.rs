//! hook の「所有の記録」: settings-reconcile が前回の適用で書いた `(event, command)` の
//! 集合を、ホストごとのファイルに残す。
//!
//! 宣言から外した hook を settings.json から消すために、退役の一覧(`retire`、command の
//! 完全一致)を人が書き足す方式は、書き忘れると旧エントリが残り続けた(消えたパスを指して
//! 毎イベントで ENOENT を出す、#44)。代わりに「前回書いたが、今回の宣言に無いもの」を
//! 消す。所有は **この記録だけ**で決める — command の接頭辞(`~/.claude/hooks/`)では
//! 決めない。同じディレクトリに、別のツールが自分で足したエントリ(herdr の
//! `herdr-agent-state.sh` など)があり、それは記録に無いので触らない。
//!
//! 先行例: Kubernetes Server-Side Apply の managedFields(manager が所有する項目を記録し、
//! 次の apply で外れた項目を消す)。違いは、所有を API サーバではなくホストごとの
//! ファイルに持つことと、粒度が `(event, command)` であること。
//!
//! 形式: `{"version":1,"entries":[{"command":"…","event":"…"},…]}`(並びは固定)。

use crate::hooks::Managed;
use serde_json::{json, Value};
use std::path::Path;

/// 記録を読む。無い・読めない・壊れているときは `None`(初回と同じ扱い)。壊れていても
/// settings.json の適用を止めない: 止めると、記録 1 つの破損で home-manager の
/// activation 全体が落ちる。`None` のときは、凍結した退役の一覧だけが効く。
pub fn load(path: &Path) -> Option<Managed> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!(
                "settings-reconcile: {}: 所有の記録を読めない({e})。初回として扱います",
                path.display()
            );
            return None;
        }
    };
    match parse(&text) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!(
                "settings-reconcile: {}: 所有の記録が壊れている({e})。初回として扱います",
                path.display()
            );
            None
        }
    }
}

fn parse(text: &str) -> Result<Managed, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if v.get("version") != Some(&json!(1)) {
        return Err("version が 1 ではない".into());
    }
    let entries = v
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("entries が配列ではない")?;
    let mut out = Managed::new();
    for e in entries {
        let field = |k: &str| {
            e.get(k)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("entries[].{k} が文字列ではない"))
        };
        out.insert((field("event")?, field("command")?));
    }
    Ok(out)
}

pub fn render(m: &Managed) -> String {
    let entries: Vec<Value> = m
        .iter()
        .map(|(event, command)| json!({"event": event, "command": command}))
        .collect();
    let mut s = serde_json::to_string_pretty(&json!({"version": 1, "entries": entries}))
        .unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

/// 前回と同じなら何も書かない(定常状態で mtime を汚さない)。親ディレクトリは作る。
pub fn store_if_changed(
    path: &Path,
    before: Option<&Managed>,
    after: &Managed,
) -> Result<(), String> {
    if before == Some(after) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::json::write_atomic(path, &render(after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(pairs: &[(&str, &str)]) -> Managed {
        pairs
            .iter()
            .map(|(e, c)| (e.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn render_parse_roundtrip_is_stable() {
        let m = set(&[("Stop", "'/x/b'"), ("PreToolUse", "'/x/a' pre")]);
        let text = render(&m);
        assert_eq!(parse(&text).unwrap(), m);
        assert_eq!(render(&parse(&text).unwrap()), text);
        // 並びは (event, command) の辞書順で固定。
        assert!(text.find("PreToolUse").unwrap() < text.find("Stop").unwrap());
    }

    #[test]
    fn parse_rejects_wrong_shapes() {
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"version":2,"entries":[]}"#).is_err());
        assert!(parse(r#"{"version":1}"#).is_err());
        assert!(parse(r#"{"version":1,"entries":[{"event":"Stop"}]}"#).is_err());
    }

    #[test]
    fn missing_and_corrupt_are_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(&d.path().join("absent.json")), None);
        let bad = d.path().join("bad.json");
        std::fs::write(&bad, "{").unwrap();
        assert_eq!(load(&bad), None);
    }

    #[test]
    fn store_skips_identical_and_creates_parents() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/managed.json");
        let m = set(&[("Stop", "c")]);
        store_if_changed(&p, None, &m).unwrap();
        assert_eq!(load(&p), Some(m.clone()));
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        store_if_changed(&p, Some(&m), &m).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
    }
}
