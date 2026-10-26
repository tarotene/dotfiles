//! Same-directory atomic files, write-ahead recovery journal, exact-byte CAS.
//! A journal never authorizes overwriting a concurrently modified file.
use super::{digest, read, state_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    entries: Vec<Entry>,
}

fn root(home: &Path) -> PathBuf {
    home.join(".local/state/dotfiles/agent-permissions-transactions")
}
fn pending(home: &Path) -> PathBuf {
    root(home).join("pending")
}

fn get(path: &Path) -> Result<Option<Vec<u8>>, String> {
    reject_symlinks(path)?;
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn reject_symlinks(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("refusing symlink write: {}", ancestor.display()))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", ancestor.display())),
        }
    }
    Ok(())
}

pub fn check_pending(home: &Path) -> Result<(), String> {
    if let Some(bytes) = get(&pending(home))? {
        return Err(format!(
            "incomplete permission migration: {}; inspect then migrate --restore JOURNAL",
            String::from_utf8_lossy(&bytes)
        ));
    }
    Ok(())
}

fn private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    reject_symlinks(path)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}

fn replace(path: &Path, before: &Option<Vec<u8>>, after: &Option<Vec<u8>>) -> Result<(), String> {
    if &get(path)? != before {
        return Err(format!("concurrent modification: {}", path.display()));
    }
    if before == after {
        return Ok(());
    }
    let parent = path.parent().ok_or("file has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    match after {
        Some(bytes) => {
            let temp = parent.join(format!(
                ".agent-permissions-{}-{}.tmp",
                std::process::id(),
                digest(path.as_os_str().as_encoded_bytes())
            ));
            private_file(&temp, bytes)?;
            if let Ok(metadata) = fs::metadata(path) {
                fs::set_permissions(&temp, metadata.permissions()).map_err(|e| e.to_string())?;
            }
            if &get(path)? != before {
                let _ = fs::remove_file(&temp);
                return Err(format!("concurrent modification: {}", path.display()));
            }
            fs::rename(&temp, path).map_err(|e| e.to_string())?;
        }
        None => fs::remove_file(path).map_err(|e| e.to_string())?,
    }
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    if &get(path)? != after {
        return Err(format!("readback mismatch: {}", path.display()));
    }
    Ok(())
}

pub fn commit(
    home: &Path,
    writes: BTreeMap<PathBuf, Option<Vec<u8>>>,
    expected: &BTreeMap<PathBuf, String>,
    record: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<PathBuf, String> {
    check_pending(home)?;
    let dir = root(home);
    reject_symlinks(&dir)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let journal_path = dir.join(format!("{nonce}-{}.json", std::process::id()));
    let mut journal = Journal {
        version: 1,
        entries: Vec::new(),
    };
    for (path, after) in writes {
        let before = get(&path)?;
        if let Some(hash) = expected.get(&path) {
            if before.as_ref().map(|b| digest(b)).as_ref() != Some(hash) {
                return Err(format!("stale review: {}", path.display()));
            }
        } else if before.is_some() {
            return Err(format!("new file appeared: {}", path.display()));
        }
        journal.entries.push(Entry {
            path,
            before,
            after,
        });
    }
    // Ownership is journaled too. Initially unchanged; updated before its write.
    let state = state_path(home);
    let before_state = get(&state)?;
    journal.entries.push(Entry {
        path: state.clone(),
        before: before_state.clone(),
        after: before_state,
    });
    private_file(&journal_path, &serde_json::to_vec_pretty(&journal).unwrap())?;
    private_file(&pending(home), journal_path.as_os_str().as_encoded_bytes())?;
    fs::File::open(&dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    // Recheck *all* observations, including retained sources, after journaling.
    for (path, hash) in expected {
        if get(path)?.as_ref().map(|b| digest(b)).as_ref() != Some(hash) {
            return Err(format!(
                "concurrent modification: {} (journal {})",
                path.display(),
                journal_path.display()
            ));
        }
    }
    let n = journal.entries.len() - 1;
    for entry in &journal.entries[..n] {
        replace(&entry.path, &entry.before, &entry.after)?;
    }
    for entry in &journal.entries[..n] {
        if get(&entry.path)? != entry.after {
            return Err(format!(
                "concurrent modification after write: {}",
                entry.path.display()
            ));
        }
    }
    for (path, hash) in expected {
        if !journal.entries.iter().any(|e| e.path == *path)
            && get(path)?.as_ref().map(|b| digest(b)).as_ref() != Some(hash)
        {
            return Err(format!("retained source changed: {}", path.display()));
        }
    }
    let bytes = record()?;
    for entry in &journal.entries[..n] {
        if get(&entry.path)? != entry.after {
            return Err(format!(
                "concurrent modification during readback: {}",
                entry.path.display()
            ));
        }
    }
    journal.entries[n].after = Some(bytes);
    let old = Some(read(&journal_path)?);
    replace(
        &journal_path,
        &old,
        &Some(serde_json::to_vec_pretty(&journal).unwrap()),
    )?;
    let entry = &journal.entries[n];
    replace(&entry.path, &entry.before, &entry.after)?;
    fs::remove_file(pending(home)).map_err(|e| e.to_string())?;
    Ok(journal_path)
}

pub fn restore(home: &Path, path: &Path) -> Result<(), String> {
    reject_symlinks(path)?;
    let parent = fs::canonicalize(root(home)).map_err(|e| e.to_string())?;
    let actual = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if actual.parent() != Some(parent.as_path()) {
        return Err("restore requires a journal from this HOME".into());
    }
    if let Some(current) = get(&pending(home))? {
        if current != path.as_os_str().as_encoded_bytes() {
            return Err("different migration is pending".into());
        }
    }
    let journal: Journal = serde_json::from_slice(&read(path)?).map_err(|e| e.to_string())?;
    if journal.version != 1 {
        return Err("unsupported journal".into());
    }
    // Validate the entire set before restoring any file.
    for e in &journal.entries {
        let current = get(&e.path)?;
        if current != e.before && current != e.after {
            return Err(format!("restore conflict: {}", e.path.display()));
        }
    }
    for e in journal.entries.iter().rev() {
        let current = get(&e.path)?;
        if current != e.before && current != e.after {
            return Err(format!("restore conflict: {}", e.path.display()));
        }
        replace(&e.path, &current, &e.before)?;
    }
    if let Some(current) = get(&pending(home))? {
        if current != path.as_os_str().as_encoded_bytes() {
            return Err("different migration is pending".into());
        }
        fs::remove_file(pending(home)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_transaction_blocks_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        fs::write(&file, b"before").unwrap();
        let writes = BTreeMap::from([(file.clone(), Some(b"after".to_vec()))]);
        let expected = BTreeMap::from([(file.clone(), digest(b"before"))]);
        assert!(commit(home, writes, &expected, || Err(
            "readback interrupted".into()
        ))
        .is_err());
        assert_eq!(fs::read(&file).unwrap(), b"after");
        assert!(check_pending(home).is_err());
        let journal = PathBuf::from(fs::read_to_string(pending(home)).unwrap());
        restore(home, &journal).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"before");
        check_pending(home).unwrap();
    }

    #[test]
    fn edit_during_readback_is_not_adopted_as_owned() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        fs::write(&file, b"before").unwrap();
        let writes = BTreeMap::from([(file.clone(), Some(b"after".to_vec()))]);
        let expected = BTreeMap::from([(file.clone(), digest(b"before"))]);
        let result = commit(home, writes, &expected, || {
            fs::write(&file, b"concurrent").unwrap();
            Ok(b"record".to_vec())
        });
        assert!(result.unwrap_err().contains("concurrent"));
        assert!(!state_path(home).exists());
        assert_eq!(fs::read(&file).unwrap(), b"concurrent");
    }
}
