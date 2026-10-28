//! Runtime-only, non-public backup configuration.
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, PathBuf};

#[derive(Debug)]
pub struct Source {
    pub kind: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct Profile {
    pub identity: String,
    pub bucket_id: String,
    pub prefix: String,
    pub sources: Vec<Source>,
    pub excludes: Vec<String>,
    pub audit_roots: Vec<PathBuf>,
}

pub fn parse(bytes: &[u8]) -> Result<Profile, &'static str> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "invalid backup profile")?;
    let identity = value
        .get("identity")
        .and_then(Value::as_str)
        .ok_or("missing backup identity")?;
    if identity.len() != 36
        || identity.chars().enumerate().any(|(index, c)| {
            if [8, 13, 18, 23].contains(&index) {
                c != '-'
            } else {
                !c.is_ascii_hexdigit()
            }
        })
    {
        return Err("invalid backup identity");
    }
    let bucket_id = value
        .get("source_bucket_id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or("missing source bucket")?;
    let prefix = value
        .get("source_prefix")
        .and_then(Value::as_str)
        .ok_or("missing source prefix")?;
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    for source in value
        .get("sources")
        .and_then(Value::as_array)
        .ok_or("missing backup sources")?
    {
        let kind = source
            .get("kind")
            .and_then(Value::as_str)
            .ok_or("missing source kind")?;
        if !["git", "git-common", "sqlite", "mozc", "jsonl"].contains(&kind) {
            return Err("invalid source kind");
        }
        let path = PathBuf::from(
            source
                .get("path")
                .and_then(Value::as_str)
                .ok_or("missing source path")?,
        );
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            || !seen.insert(path.clone())
        {
            return Err("invalid or repeated source path");
        }
        sources.push(Source {
            kind: kind.to_owned(),
            path,
        });
    }
    if sources.is_empty() {
        return Err("backup sources are empty");
    }
    let excludes = value
        .get("excludes")
        .and_then(Value::as_array)
        .ok_or("missing backup excludes")?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or("invalid backup exclude")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let audit_roots = value
        .get("audit_roots")
        .and_then(Value::as_array)
        .ok_or("missing audit roots")?
        .iter()
        .map(|v| {
            let path = PathBuf::from(v.as_str().ok_or("invalid audit root")?);
            if !path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            {
                return Err("invalid audit root");
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if audit_roots.is_empty() {
        return Err("audit roots are empty");
    }
    Ok(Profile {
        identity: identity.into(),
        bucket_id: bucket_id.into(),
        prefix: prefix.into(),
        sources,
        excludes,
        audit_roots,
    })
}

/// Persist only a validated profile; the caller supplies its private XDG directory.
pub fn persist(bytes: &[u8], directory: &std::path::Path) -> Result<(), &'static str> {
    parse(bytes)?;
    if !directory.is_absolute() {
        return Err("invalid private configuration directory");
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|_| "cannot create private configuration directory")?;
    let metadata = std::fs::symlink_metadata(directory)
        .map_err(|_| "cannot inspect private configuration directory")?;
    use std::os::unix::fs::PermissionsExt;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("private configuration directory is not private");
    }
    let mut staged = tempfile::Builder::new()
        .prefix("profile-")
        .tempfile_in(directory)
        .map_err(|_| "cannot stage private profile")?;
    staged
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| "cannot protect private profile")?;
    staged
        .write_all(bytes)
        .map_err(|_| "cannot write private profile")?;
    staged
        .as_file()
        .sync_all()
        .map_err(|_| "cannot sync private profile")?;
    staged
        .persist(directory.join("profile.json"))
        .map_err(|_| "cannot install private profile")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_incomplete_and_duplicate_sources() {
        let valid = br#"{"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"bucket","source_prefix":"prefix/","sources":[{"kind":"git","path":"/tmp/example"}],"excludes":[],"audit_roots":["/tmp"]}"#;
        assert!(parse(valid).is_ok());
        assert!(parse(br#"{}"#).is_err());
        let duplicate = br#"{"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"bucket","source_prefix":"prefix/","sources":[{"kind":"git","path":"/tmp/example"},{"kind":"jsonl","path":"/tmp/example"}],"excludes":[]}"#;
        assert!(parse(duplicate).is_err());
    }
}
