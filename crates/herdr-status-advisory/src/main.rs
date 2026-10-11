//! herdr-status-advisory — SessionStart で `herdr status --json` を読み、herdr の
//! client と server が食い違っているときだけ、先に知らせる。設計と根拠:
//! docs/claude/herdr-status-advisory.md。
//!
//! 食い違いに気づくのが、作業ツリーを切る瞬間(`herdr worktree create` が
//! `protocol_mismatch` で失敗する)になってから、という遅さを直す。照合は herdr
//! 自身が `herdr status --json` で公開しているので、ここでは呼んで読むだけ。
//! 復旧(server の再起動)は全ペインのプロセスを終了させるので、hook は何も
//! 実行しない。本人に手順と副作用を先に伝えるだけ。
//!
//! 全失敗経路は fail-open(無出力 exit 0): `herdr` が無い、server が動いていない、
//! `herdr status` が失敗・タイムアウト、JSON を読めない、のいずれでも何も出さない。
//! 食い違い(またはその前兆)を見つけたときだけ `additionalContext` を stdout に出す。

use serde_json::{json, Value};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// `herdr status --json` の stdout。起動失敗・非 0 終了・タイムアウトは `None`。
fn herdr_status() -> Option<String> {
    let mut child = Command::new("herdr")
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // 出力は小さい(数 KB)ので、読み終えてから待つ。パイプが詰まる心配は無い。
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        stdout.read_to_string(&mut buf).ok().map(|_| buf)
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let out = reader.join().ok().flatten();
                return if st.success() { out } else { None };
            }
            Ok(None) if start.elapsed() < STATUS_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn bool_at(v: &Value, path: &[&str]) -> Option<bool> {
    let mut cur = v;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_bool()
}

fn protocol_at(v: &Value, side: &str) -> String {
    v.get(side)
        .and_then(|s| s.get("protocol"))
        .and_then(Value::as_u64)
        .map_or_else(|| "不明".to_string(), |n| n.to_string())
}

/// `herdr status --json` の応答から、知らせる文面を作る。知らせることが無ければ `None`。
fn advisory(status: &Value) -> Option<String> {
    // server が動いていなければ、食い違いは存在しない(次に起動するときは新しい版)。
    if bool_at(status, &["server", "running"]) != Some(true) {
        return None;
    }
    let incompatible = bool_at(status, &["server", "compatible"]) == Some(false)
        || bool_at(status, &["server", "endpoint_compatible"]) == Some(false);
    let restart_hint = bool_at(status, &["server", "restart_needed"]) == Some(true)
        || bool_at(status, &["server", "server_binary_stale"]) == Some(true)
        || bool_at(status, &["update", "restart_needed"]) == Some(true)
        || bool_at(status, &["update", "server_binary_stale"]) == Some(true);

    let restart = "復旧は本人の操作で、herdr の外のターミナルから `herdr server stop` を実行してから `herdr` を起動し直す。副作用として、動作中の全ペインのプロセスが終了するので、実行中の作業(このセッションを含む)を先に区切ること。詳しくは docs/operations.md の「Restarting herdr after a switch that changes its binary or hooks」。";
    if incompatible {
        return Some(format!(
            "[herdr-status-advisory] herdr の client と server の版が食い違っています(client protocol {}、server protocol {})。このままでは `herdr worktree create` が `protocol_mismatch` で失敗し、作業ツリーを切れません。エージェントが自分で復旧する手段はありません(server を止めるとペインのプロセスが終了する)。{restart}",
            protocol_at(status, "client"),
            protocol_at(status, "server"),
        ));
    }
    if restart_hint {
        return Some(format!(
            "[herdr-status-advisory] 稼働中の herdr server は、更新後の版に入れ替わっていません(再起動が必要と herdr が報告)。今は client と server の protocol が互換でも、次の更新で `herdr worktree create` が `protocol_mismatch` で失敗しうるので、区切りのよいときに再起動してください。{restart}"
        ));
    }
    None
}

fn run() -> Option<()> {
    // hook の入力(stdin)は使わない。読まずに閉じてよい。
    let raw = herdr_status()?;
    let status: Value = serde_json::from_str(&raw).ok()?;
    let ctx = advisory(&status)?;
    println!(
        "{}",
        json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": ctx}})
    );
    Some(())
}

fn main() {
    let _ = run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(server: Value, update: Value) -> Value {
        json!({"client": {"protocol": 22}, "server": server, "update": update})
    }

    #[test]
    fn healthy_is_silent() {
        let s = status(
            json!({"running": true, "protocol": 22, "compatible": true, "endpoint_compatible": true, "restart_needed": false, "server_binary_stale": false}),
            json!({"restart_needed": false, "server_binary_stale": false}),
        );
        assert_eq!(advisory(&s), None);
    }

    #[test]
    fn server_not_running_is_silent() {
        let s = status(
            json!({"running": false, "compatible": false}),
            json!({"restart_needed": true}),
        );
        assert_eq!(advisory(&s), None);
    }

    #[test]
    fn incompatible_protocol_warns_with_both_protocols() {
        let s = status(
            json!({"running": true, "protocol": 20, "compatible": false, "endpoint_compatible": true}),
            json!({"restart_needed": true}),
        );
        let m = advisory(&s).unwrap();
        assert!(m.contains("client protocol 22"));
        assert!(m.contains("server protocol 20"));
        assert!(m.contains("protocol_mismatch"));
        assert!(m.contains("herdr server stop"));
        assert!(m.contains("全ペイン"));
    }

    #[test]
    fn incompatible_endpoint_warns() {
        let s = status(
            json!({"running": true, "protocol": 22, "compatible": true, "endpoint_compatible": false}),
            json!({}),
        );
        assert!(advisory(&s).unwrap().contains("食い違っています"));
    }

    #[test]
    fn restart_needed_only_is_a_soft_hint() {
        let s = status(
            json!({"running": true, "protocol": 22, "compatible": true, "endpoint_compatible": true, "restart_needed": true}),
            json!({}),
        );
        let m = advisory(&s).unwrap();
        assert!(m.contains("入れ替わっていません"));
        assert!(!m.contains("食い違っています"));
    }

    #[test]
    fn stale_binary_in_update_section_is_a_soft_hint() {
        let s = status(
            json!({"running": true, "compatible": true, "endpoint_compatible": true}),
            json!({"server_binary_stale": true}),
        );
        assert!(advisory(&s).is_some());
    }

    #[test]
    fn missing_fields_are_silent() {
        assert_eq!(advisory(&json!({})), None);
        assert_eq!(advisory(&json!({"server": {"running": true}})), None);
    }
}
