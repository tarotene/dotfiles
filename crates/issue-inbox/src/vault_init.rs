//! Explicit, non-destructive preparation of an Obsidian vault.
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub fn prepare(root: &Path, initialize: bool) -> io::Result<()> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "vault path must be absolute",
        ));
    }
    let obsidian = root.join(".obsidian");
    if obsidian.is_dir() {
        // Obsidian may omit app.json until a preference is changed. If it is
        // present, validate it without changing any existing preferences.
        let app = obsidian.join("app.json");
        match fs::read(&app) {
            Ok(data) => {
                let value: serde_json::Value = serde_json::from_slice(&data)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                if !value.is_object() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "app.json is not an object",
                    ));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        return Ok(());
    }
    if !initialize {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "vault is absent or invalid; initialization was not requested",
        ));
    }
    if root.exists() {
        if !root.is_dir() || fs::read_dir(root)?.next().transpose()?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "vault directory is not empty",
            ));
        }
    } else {
        fs::create_dir_all(root)?;
    }
    DirBuilder::new().mode(0o700).create(&obsidian)?;
    let app = obsidian.join("app.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&app)?;
    file.write_all(b"{}\n")?;
    file.sync_all()?;
    let data = fs::read(&app)?;
    if data != b"{}\n"
        || fs::metadata(&app)?.permissions().mode() & 0o777 != 0o600
        || fs::metadata(&obsidian)?.permissions().mode() & 0o777 != 0o700
    {
        return Err(io::Error::other("initialized vault failed verification"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_only_when_explicit_and_preserves_existing_settings() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("vault");
        assert!(prepare(&root, false).is_err());
        assert!(!root.exists());
        prepare(&root, true).unwrap();
        let app = root.join(".obsidian/app.json");
        fs::write(&app, "{\"setting\":true}").unwrap();
        prepare(&root, true).unwrap();
        assert_eq!(fs::read_to_string(app).unwrap(), "{\"setting\":true}");
        let bad = d.path().join("bad");
        fs::create_dir(&bad).unwrap();
        fs::write(bad.join("existing"), "keep").unwrap();
        assert!(prepare(&bad, true).is_err());
        assert_eq!(fs::read_to_string(bad.join("existing")).unwrap(), "keep");
        assert!(prepare(Path::new("relative"), true).is_err());
    }
}
