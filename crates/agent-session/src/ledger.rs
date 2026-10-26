//! クラスタ G: session_id をキーにした状態台帳。
//!
//! 吸収元: `stack-base-guard.sh:64-84`(`state_file()` + `record_chain_head`)/
//! `plan-fresh-gate.sh:95-99`・`168-169`。session_id は `[^A-Za-z0-9._-]` を
//! `_` に置換してファイル名にし、ディレクトリは 0700 で作る。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<dir>/<sanitized session_id>.<ext>` に 1 行 1 レコードで追記する台帳。
#[derive(Debug, Clone)]
pub struct SessionLedger {
    dir: PathBuf,
    ext: String,
}

/// session_id をファイル名に使える形にする(bash の `${sid//[^A-Za-z0-9._-]/_}`)。
pub fn sanitize_session_id(sid: &str) -> String {
    sid.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl SessionLedger {
    pub fn new(dir: impl Into<PathBuf>, ext: &str) -> Self {
        Self {
            dir: dir.into(),
            ext: ext.to_string(),
        }
    }

    pub fn path(&self, session_id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.{}", sanitize_session_id(session_id), self.ext))
    }

    /// 1 レコード追記する。改行を含むレコードは台帳を壊すので拒否する。
    pub fn append(&self, session_id: &str, record: &str) -> std::io::Result<()> {
        if record.contains(['\n', '\r']) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "record must be a single line",
            ));
        }
        ensure_private_dir(&self.dir)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(session_id))?;
        writeln!(f, "{record}")
    }

    /// 全レコード。台帳が無ければ空。
    pub fn records(&self, session_id: &str) -> Vec<String> {
        fs::read_to_string(self.path(session_id))
            .map(|s| {
                s.lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn contains(&self, session_id: &str, record: &str) -> bool {
        self.records(session_id).iter().any(|r| r == record)
    }
}

fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_matches_bash() {
        assert_eq!(sanitize_session_id("abc-1.2_x"), "abc-1.2_x");
        assert_eq!(sanitize_session_id("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_session_id("a b/c"), "a_b_c");
        assert_eq!(sanitize_session_id("日本"), "__");
    }

    #[test]
    fn append_and_read() {
        let d = tempfile::tempdir().unwrap();
        let l = SessionLedger::new(d.path().join("state"), "ledger");
        assert!(l.records("s").is_empty());
        l.append("s", "pr o/r 1").unwrap();
        l.append("s", "issue o/r 2").unwrap();
        assert_eq!(l.records("s"), vec!["pr o/r 1", "issue o/r 2"]);
        assert!(l.contains("s", "pr o/r 1"));
        assert!(!l.contains("other", "pr o/r 1"));
        assert!(l.append("s", "bad\nline").is_err());
        assert_eq!(l.path("s"), d.path().join("state/s.ledger"));
    }

    #[cfg(unix)]
    #[test]
    fn dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let l = SessionLedger::new(d.path().join("state"), "ledger");
        l.append("s", "x").unwrap();
        let mode = fs::metadata(d.path().join("state"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
