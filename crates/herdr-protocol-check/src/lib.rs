//! herdr-protocol-check — herdr のクライアントとサーバの protocol 不一致を
//! SessionStart で先に伝える hook の判定エンジン。設計と根拠:
//! docs/claude/herdr-protocol-check.md。
//!
//! 入力は `herdr status --json` の出力。`server.running` かつ
//! (`server.compatible == false` または `server.restart_needed` または
//! `server.server_binary_stale`)のときだけ警告文を返す。読めない・欠けている
//! 入力は `None`(fail-open)。

use serde_json::Value;

/// `herdr status --json` の出力から、警告があれば `additionalContext` の文面。
pub fn decide(status_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(status_json).ok()?;
    let server = v.get("server")?;
    if !server.get("running")?.as_bool()? {
        return None;
    }
    let flag = |k: &str| server.get(k).and_then(Value::as_bool);
    let incompatible = flag("compatible") == Some(false);
    let restart_needed = flag("restart_needed") == Some(true);
    let stale = flag("server_binary_stale") == Some(true);
    if !(incompatible || restart_needed || stale) {
        return None;
    }
    let proto = |p: Option<&Value>| match p.and_then(|x| x.get("protocol")) {
        Some(Value::Number(n)) => n.to_string(),
        _ => "不明".to_string(),
    };
    let client = proto(v.get("client"));
    let server_p = proto(Some(server));
    Some(format!(
        "[herdr-protocol-check] client protocol {client} / server protocol {server_p}。\
`herdr worktree create` は protocol_mismatch で失敗する。\
サーバの再起動はペインのプロセスを終了させる副作用がある。\
再起動の可否を本人に先に伝える。"
    ))
}

#[cfg(test)]
mod tests {
    use super::decide;

    /// 実機(herdr 0.9.3)の `herdr status --json` の形に合わせた入力。
    fn status(proto_server: u32, compatible: bool, restart: bool, stale: bool) -> String {
        format!(
            r#"{{"client":{{"version":"0.9.3","protocol":22}},"server":{{"status":"running","running":true,"version":"0.9.3","protocol":{proto_server},"compatible":{compatible},"restart_needed":{restart},"server_binary_stale":{stale}}}}}"#
        )
    }

    #[test]
    fn compatible_is_silent() {
        assert_eq!(decide(&status(22, true, false, false)), None);
    }

    #[test]
    fn protocol_mismatch_warns_with_both_protocols() {
        let m = decide(&status(21, false, false, false)).unwrap();
        assert!(m.contains("client protocol 22"), "{m}");
        assert!(m.contains("server protocol 21"), "{m}");
        assert!(m.contains("protocol_mismatch"), "{m}");
        assert!(m.contains("再起動の可否を本人に先に伝える"), "{m}");
    }

    #[test]
    fn restart_needed_or_stale_binary_warns() {
        assert!(decide(&status(22, true, true, false)).is_some());
        assert!(decide(&status(22, true, false, true)).is_some());
    }

    #[test]
    fn not_running_or_garbage_is_silent() {
        let down = r#"{"client":{"protocol":22},"server":{"running":false,"compatible":false}}"#;
        assert_eq!(decide(down), None);
        assert_eq!(decide(""), None);
        assert_eq!(decide("not json"), None);
        assert_eq!(decide("{}"), None);
    }
}
