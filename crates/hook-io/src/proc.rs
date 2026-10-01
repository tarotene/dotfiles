//! クラスタ L: `timeout N cmd` と `date +FMT` の置き換え(ADR-0024 Stage 4b、#412)。
//!
//! 吸収元: `plan-fresh-gate.sh` の `timeout 15 git fetch`、
//! `copilot-plan-review.sh` の `timeout "$COPILOT_TIMEOUT" copilot …` /
//! `timeout "$secs" bash "$bin"`、各 plan hook の `date +…`。
//!
//! GNU `timeout(1)` は(`--foreground` 無しでは)自分と子を新しいプロセス
//! グループに置き、期限切れでグループ全体に TERM を送る。ここでも子を
//! `process_group(0)` で起動し、期限切れでグループに TERM を送ってから回収する
//! (`crates/git-checkout-freshness` の `run_with_timeout` と同じ方式)。

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

fn kill_group(child: &mut Child) {
    let pgid = child.id().to_string();
    let _ = Command::new("kill")
        .args(["-TERM", "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

/// 子を待つ。`limit` を過ぎたらプロセスグループごと kill して `None`。
pub fn wait_with_timeout(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Some(st),
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() >= limit {
            kill_group(child);
            let _ = child.wait();
            return None;
        }
        sleep(Duration::from_millis(20));
    }
}

/// 新しいプロセスグループで起動する(`timeout(1)` と同じ配置)。
pub fn spawn_group(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.process_group(0).spawn()
}

/// `printf '%s' "$stdin" | timeout <limit> cmd` の stdout を取る。stderr は捨てる。
/// 起動失敗・タイムアウトは `None`。終了コードは呼び出し側が見る。
pub fn output_with_timeout(
    cmd: &mut Command,
    limit: Duration,
    stdin: Option<&[u8]>,
) -> Option<(ExitStatus, Vec<u8>)> {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = spawn_group(cmd).ok()?;
    let writer = stdin.map(|data| {
        let data = data.to_vec();
        let mut pipe = child.stdin.take().expect("piped stdin");
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        })
    });
    let mut out_pipe = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let status = wait_with_timeout(&mut child, limit);
    let out = reader.join().unwrap_or_default();
    if let Some(w) = writer {
        let _ = w.join();
    }
    Some((status?, out))
}

/// `date +FMT` の出力(末尾改行除去)。ローカル時刻の整形を std だけで
/// 行えないため、bash 版と同じく `date(1)` に委ねる。失敗時は空文字。
pub fn date(fmt: &str) -> String {
    Command::new("date")
        .arg(format!("+{fmt}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            while s.ends_with('\n') {
                s.pop();
            }
            s
        })
        .unwrap_or_default()
}

/// `command -v name` 相当: `/` を含めばそのパスが実行可能か、含まなければ
/// `PATH` 上に実行可能ファイルがあるか。
pub fn command_exists(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let is_exec = |p: &std::path::Path| {
        p.is_file()
            && std::fs::metadata(p)
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    };
    if name.is_empty() {
        return false;
    }
    if name.contains('/') {
        return is_exec(std::path::Path::new(name));
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|d| is_exec(&d.join(name))))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_and_feeds_stdin() {
        let (st, out) = output_with_timeout(
            &mut Command::new("cat"),
            Duration::from_secs(5),
            Some(b"hello"),
        )
        .unwrap();
        assert!(st.success());
        assert_eq!(out, b"hello");
    }

    #[test]
    fn timeout_kills() {
        let t = Instant::now();
        let mut c = Command::new("sleep");
        c.arg("10");
        assert!(output_with_timeout(&mut c, Duration::from_millis(200), None).is_none());
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn date_and_command_exists() {
        assert_eq!(date("%Y").len(), 4);
        assert!(command_exists("sh"));
        assert!(!command_exists("definitely-not-a-real-binary-xyz"));
        assert!(!command_exists("/nonexistent/x"));
    }
}
