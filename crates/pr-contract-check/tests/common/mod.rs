//! テスト用の `gh` スタブ。`gh api <path> [-X/-F/-H <値>] [--jq <式>]` だけを受け、
//! path から固定のファイルを返す(`--jq` があれば `jq -r` を通す。実物の `gh api --jq` は
//! top-level の scalar を raw output で返す)。
//!
//! かつて bash の selftest が heredoc で持っていたスタブと同じ規則(path → file、
//! `/` と `.` を `_` に、`-F` の値は path の選択に使わない)。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const GH_STUB: &str = r#"#!/usr/bin/env bash
set -euo pipefail
dir="$(dirname "$0")"
[[ "$1" == api ]] || exit 1
shift
path="$1"
shift
jqfilter=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --jq) jqfilter="$2"; shift 2 ;;
    -X | -F | -H) shift 2 ;;
    *) shift ;;
  esac
done
key="$(printf '%s' "$path" | tr '/.' '__')"
if [[ -f "$dir/fail-$key" ]]; then
  echo "gh: HTTP 500" >&2
  exit 1
fi
if [[ ! -f "$dir/fixtures/$key" ]]; then
  echo "gh: Not Found (HTTP 404)" >&2
  exit 1
fi
if [[ -n "$jqfilter" ]]; then
  jq -r "$jqfilter" "$dir/fixtures/$key"
else
  cat "$dir/fixtures/$key"
fi
"#;

pub struct Stub {
    dir: PathBuf,
}

impl Stub {
    pub fn new() -> Stub {
        let dir = std::env::temp_dir().join(format!(
            "pr-contract-check-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(dir.join("fixtures")).unwrap();
        let gh = dir.join("gh");
        std::fs::write(&gh, GH_STUB).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        Stub { dir }
    }

    pub fn gh(&self) -> PathBuf {
        self.dir.join("gh")
    }

    fn key(api_path: &str) -> String {
        api_path.replace(['/', '.'], "_")
    }

    /// `gh api <api_path>` が返す中身を置く。
    pub fn put(&self, api_path: &str, content: &str) {
        std::fs::write(self.dir.join("fixtures").join(Self::key(api_path)), content).unwrap();
    }

    /// `gh api <api_path>` を、HTTP 500 で失敗させる。
    pub fn fail(&self, api_path: &str) {
        std::fs::write(self.dir.join(format!("fail-{}", Self::key(api_path))), "").unwrap();
    }

    pub fn unfail(&self, api_path: &str) {
        let _ = std::fs::remove_file(self.dir.join(format!("fail-{}", Self::key(api_path))));
    }

    pub fn remove(&self, api_path: &str) {
        let _ = std::fs::remove_file(self.dir.join("fixtures").join(Self::key(api_path)));
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Run {
    pub fn both(&self) -> String {
        format!("{}{}", self.out, self.err)
    }
}

pub fn run(bin: &str, envs: &[(&str, &str)], args: &[&str]) -> Run {
    let o: Output = Command::new(bin)
        .args(args)
        .envs(envs.iter().copied())
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}
