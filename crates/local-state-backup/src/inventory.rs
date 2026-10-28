//! Read-only inventory and validation before a snapshot.
use crate::profile::Profile;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug)]
pub struct Count {
    pub path: PathBuf,
    pub files: u64,
    pub bytes: u64,
}

fn walk(path: &Path, device: u64, files: &mut u64, bytes: &mut u64) -> Result<(), &'static str> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "cannot inspect backup source")?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.dev() != device {
        return Err("backup source crosses a filesystem boundary");
    }
    if meta.is_file() {
        *files += 1;
        *bytes = bytes
            .checked_add(meta.len())
            .ok_or("inventory size overflow")?;
    }
    if meta.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|_| "cannot read backup source")? {
            let entry = entry.map_err(|_| "cannot read backup source")?;
            if [
                "node_modules",
                "target",
                ".direnv",
                ".venv",
                "dist",
                "result",
            ]
            .iter()
            .any(|v| entry.file_name() == *v)
            {
                continue;
            }
            walk(&entry.path(), device, files, bytes)?;
        }
    }
    Ok(())
}

pub fn inspect(profile: &Profile, home: &Path, git: &Path) -> Result<Vec<Count>, &'static str> {
    let home = std::fs::canonicalize(home).map_err(|_| "cannot resolve home directory")?;
    let forbidden = [
        ".ssh",
        ".gnupg",
        ".aws",
        ".config/gcloud",
        ".config/rclone",
        ".config/clasp",
        ".clasprc.json",
        ".claude/.credentials.json",
        ".codex/auth.json",
        ".local/share/keyrings",
        "Downloads",
    ];
    let mut roots = Vec::new();
    for source in &profile.sources {
        if std::fs::symlink_metadata(&source.path)
            .map_err(|_| "backup source is missing")?
            .file_type()
            .is_symlink()
        {
            return Err("backup source must not be a symlink");
        }
        let root = std::fs::canonicalize(&source.path).map_err(|_| "backup source is missing")?;
        for entry in &forbidden {
            let path = home.join(entry);
            if root.starts_with(&path) || path.starts_with(&root) {
                return Err("backup source overlaps a prohibited path");
            }
        }
        roots.push(root);
    }
    for (source, root) in profile.sources.iter().zip(&roots) {
        if source.kind == "git" {
            let result = Command::new(git)
                .arg("-C")
                .arg(root)
                .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
                .stderr(Stdio::null())
                .output()
                .map_err(|_| "cannot inspect Git repository")?;
            if !result.status.success() {
                return Err("cannot inspect Git repository");
            }
            let path = std::str::from_utf8(&result.stdout)
                .map_err(|_| "invalid Git common directory")?
                .trim_end();
            let common = std::fs::canonicalize(path).map_err(|_| "missing Git common directory")?;
            if !common.starts_with(root)
                && !profile
                    .sources
                    .iter()
                    .zip(&roots)
                    .any(|(s, p)| s.kind == "git-common" && p == &common)
            {
                return Err("unregistered Git common directory");
            }
        }
    }
    let mut counts = Vec::new();
    for path in roots {
        let metadata = std::fs::metadata(&path).map_err(|_| "cannot inspect backup source")?;
        let mut count = Count {
            path,
            files: 0,
            bytes: 0,
        };
        walk(
            &count.path,
            metadata.dev(),
            &mut count.files,
            &mut count.bytes,
        )?;
        counts.push(count);
    }
    Ok(counts)
}

/// Count recently modified repositories under explicit scan roots that are
/// not in the allowlist; names stay out of timer logs.
pub fn audit(profile: &Profile) -> Result<Vec<PathBuf>, &'static str> {
    use std::collections::HashSet;
    use std::time::{Duration, SystemTime};
    fn scan(
        path: &Path,
        device: u64,
        registered: &HashSet<PathBuf>,
        seen: &mut HashSet<PathBuf>,
        threshold: SystemTime,
    ) -> Result<SystemTime, &'static str> {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| "cannot inspect audit root")?;
        if metadata.file_type().is_symlink() || metadata.dev() != device {
            return Ok(SystemTime::UNIX_EPOCH);
        }
        let mut modified = metadata
            .modified()
            .map_err(|_| "cannot inspect audit timestamp")?;
        if !metadata.is_dir() {
            return Ok(modified);
        }
        let is_repo = path.join(".git").exists();
        for entry in std::fs::read_dir(path).map_err(|_| "cannot read audit root")? {
            let entry = entry.map_err(|_| "cannot read audit root")?;
            if [
                ".git",
                ".ssh",
                ".gnupg",
                ".aws",
                "node_modules",
                "target",
                ".direnv",
                ".venv",
                "dist",
                "result",
            ]
            .iter()
            .any(|name| entry.file_name() == *name)
            {
                continue;
            }
            modified = modified.max(scan(&entry.path(), device, registered, seen, threshold)?);
        }
        if is_repo && modified >= threshold {
            let canonical =
                std::fs::canonicalize(path).map_err(|_| "cannot resolve audited repository")?;
            if !registered.contains(&canonical) {
                seen.insert(canonical);
            }
        }
        Ok(modified)
    }
    let registered = profile
        .sources
        .iter()
        .filter(|source| source.kind == "git")
        .map(|source| {
            std::fs::canonicalize(&source.path).map_err(|_| "backup repository is missing")
        })
        .collect::<Result<HashSet<_>, _>>()?;
    let threshold = SystemTime::now()
        .checked_sub(Duration::from_secs(30 * 24 * 60 * 60))
        .ok_or("invalid audit clock")?;
    let mut seen = HashSet::new();
    for root in &profile.audit_roots {
        let metadata = std::fs::symlink_metadata(root).map_err(|_| "audit root is missing")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("invalid audit root");
        }
        scan(root, metadata.dev(), &registered, &mut seen, threshold)?;
    }
    let mut paths: Vec<_> = seen.into_iter().collect();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Source;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn refuses_a_real_object_on_another_device() {
        let tmp = tempfile::tempdir().unwrap();
        let source = std::path::Path::new("/dev/null");
        let expected = std::fs::metadata(tmp.path()).unwrap().dev();
        let actual = std::fs::symlink_metadata(source).unwrap().dev();
        assert_ne!(expected, actual, "fixture needs distinct devices");
        assert_eq!(
            walk(source, expected, &mut 0, &mut 0),
            Err("backup source crosses a filesystem boundary")
        );
    }

    #[test]
    fn requires_linked_worktree_common_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let repository = dir.join("repo");
        let common = dir.join("common");
        let home = dir.join("home");
        std::fs::create_dir(&repository).unwrap();
        std::fs::create_dir(&common).unwrap();
        std::fs::create_dir(&home).unwrap();
        std::fs::write(repository.join("untracked"), b"data").unwrap();
        let git = dir.join("git");
        std::fs::write(
            &git,
            format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", common.display()),
        )
        .unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = Profile {
            identity: String::new(),
            bucket_id: String::new(),
            prefix: String::new(),
            sources: vec![Source {
                kind: "git".into(),
                path: repository,
            }],
            excludes: vec![],
            audit_roots: vec![],
        };
        assert!(inspect(&profile, &home, &git).is_err());
        profile.sources.push(Source {
            kind: "git-common".into(),
            path: common,
        });
        let count = inspect(&profile, &home, &git).unwrap();
        assert_eq!(count[0].files, 1);
        assert_eq!(count[0].bytes, 4);
        profile.sources.push(Source {
            kind: "jsonl".into(),
            path: home.join(".aws"),
        });
        std::fs::create_dir(home.join(".aws")).unwrap();
        assert!(inspect(&profile, &home, &git).is_err());
    }

    #[test]
    fn audit_counts_unregistered_repository_once() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let known = root.join("known");
        let extra = root.join("extra");
        std::fs::create_dir_all(known.join(".git")).unwrap();
        std::fs::create_dir_all(extra.join(".git")).unwrap();
        std::fs::write(extra.join("untracked"), b"new").unwrap();
        let profile = Profile {
            identity: String::new(),
            bucket_id: String::new(),
            prefix: String::new(),
            sources: vec![Source {
                kind: "git".into(),
                path: known,
            }],
            excludes: vec![],
            audit_roots: vec![root.clone(), root],
        };
        assert_eq!(audit(&profile).unwrap().len(), 1);
    }
}
