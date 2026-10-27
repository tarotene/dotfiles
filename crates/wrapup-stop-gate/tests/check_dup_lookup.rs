//! `--check-dup` の照会(`gh issue list`)の失敗は、判定不能(exit 3)として、原因の診断を残す。
//!
//! 以前は、Stop が起票を求めていたので、失敗した照会のあと、変化の無い行を保留する台帳(#1129)を持って
//! いた。気づきは Stop で起票されなくなった(ADR-1136)ので、台帳は無い。照会の上限(10 秒、
//! プロセスグループごと打ち切る)と、診断の保存だけが残る。
mod common;
use common::*;
use std::time::{Duration, Instant};

fn check(e: &Env) -> Run {
    run_args(e.gate(), &["--check-dup", "dup title"])
}

#[test]
fn a_hung_lookup_is_bounded_and_reported_as_unknown() {
    let e = Env::new();
    write_exec(&e.bin_dir().join("gh"), "#!/bin/sh\nexec sleep 30\n");
    let started = Instant::now();
    let result = check(&e);
    assert_eq!(result.code, 3);
    assert!(
        result.stderr.contains("timed out after 10 seconds"),
        "{}",
        result.stderr
    );
    assert!(started.elapsed() >= Duration::from_secs(10));
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn authentication_and_network_failure_are_unknown_and_preserve_diagnostics() {
    for diagnostic in [
        "HTTP 401 Requires authentication",
        "Could not resolve host api.github.com",
    ] {
        let e = Env::new();
        write_exec(
            &e.bin_dir().join("gh"),
            &format!("#!/bin/sh\necho '{diagnostic}' >&2\nexit 1\n"),
        );
        let result = check(&e);
        assert_eq!(result.code, 3);
        assert!(result.stderr.contains(diagnostic), "{}", result.stderr);
    }
}
