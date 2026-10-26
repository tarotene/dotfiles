//! Same-directory atomic files, write-ahead recovery journal, exact-byte CAS.
//! A journal never authorizes overwriting a concurrently modified file.
use super::{digest, read, state_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Version 1 stored bytes as JSON number arrays (about 4x the file size) and
/// journaled unchanged files too. Version 2 stores lowercase hex and only the
/// files that change. Both versions stay restorable.
const JOURNAL_VERSION: u32 = 2;
/// Completed journals kept for `migrate --restore`; older ones are removed.
const KEEP_JOURNALS: usize = 5;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: PathBuf,
    #[serde(with = "file_bytes")]
    before: Option<Vec<u8>>,
    #[serde(with = "file_bytes")]
    after: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    entries: Vec<Entry>,
}

mod file_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Hex(String),
        Numbers(Vec<u8>),
    }

    pub fn serialize<S: Serializer>(value: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => s.serialize_some(&super::to_hex(bytes)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        match Option::<Raw>::deserialize(d)? {
            None => Ok(None),
            Some(Raw::Numbers(bytes)) => Ok(Some(bytes)),
            Some(Raw::Hex(text)) => super::from_hex(&text)
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

fn from_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(2) || !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err("journal bytes must be an even-length hex string".into());
    }
    Ok(digits
        .chunks(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap_or(0);
            let low = (pair[1] as char).to_digit(16).unwrap_or(0);
            (high * 16 + low) as u8
        })
        .collect())
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
        version: JOURNAL_VERSION,
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
        // An unchanged file needs no recovery record. Its reviewed hash is still
        // rechecked against `expected` before and after every write below.
        if before == after {
            continue;
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
    prune(&dir, KEEP_JOURNALS);
    Ok(journal_path)
}

/// Remove completed journals beyond the newest `keep`. Best effort: a failure
/// to prune never fails a migration that already read back correctly.
fn prune(dir: &Path, keep: usize) {
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    // `<nanoseconds>-<pid>.json`; anything else in the directory is not ours.
    let mut journals: Vec<(u128, PathBuf)> = listing
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (nonce, rest) = name.strip_suffix(".json")?.split_once('-')?;
            rest.bytes().all(|b| b.is_ascii_digit()).then_some(())?;
            Some((nonce.parse().ok()?, entry.path()))
        })
        .collect();
    journals.sort();
    let stale = journals.len().saturating_sub(keep);
    for (_, path) in journals.into_iter().take(stale) {
        let _ = fs::remove_file(path);
    }
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
    if !matches!(journal.version, 1 | JOURNAL_VERSION) {
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
    fn journal_records_only_changed_files_as_hex() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let changed = home.join("changed.json");
        let same = home.join("same.json");
        fs::write(&changed, b"before").unwrap();
        fs::write(&same, vec![b'x'; 4096]).unwrap();
        let writes = BTreeMap::from([
            (changed.clone(), Some(b"after".to_vec())),
            (same.clone(), Some(vec![b'x'; 4096])),
        ]);
        let expected = BTreeMap::from([
            (changed.clone(), digest(b"before")),
            (same.clone(), digest(&vec![b'x'; 4096])),
        ]);
        let journal = commit(home, writes, &expected, || Ok(b"record".to_vec())).unwrap();
        let text = fs::read_to_string(&journal).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], JOURNAL_VERSION);
        let paths: Vec<_> = value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| PathBuf::from(e["path"].as_str().unwrap()))
            .collect();
        // The changed file and the ownership record; the unchanged file is absent.
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&changed) && !paths.contains(&same));
        assert_eq!(value["entries"][0]["after"], to_hex(b"after"));
        assert!(text.len() < 2048, "journal stayed small: {}", text.len());
        check_pending(home).unwrap();
        restore(home, &journal).unwrap();
        assert_eq!(fs::read(&changed).unwrap(), b"before");
    }

    #[test]
    fn version_1_journals_with_number_arrays_still_restore() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        fs::write(&file, b"after").unwrap();
        fs::create_dir_all(root(home)).unwrap();
        let journal = root(home).join("1-1.json");
        let old = serde_json::json!({"version": 1, "entries": [
            {"path": file, "before": b"before".to_vec(), "after": b"after".to_vec()}
        ]});
        fs::write(&journal, old.to_string()).unwrap();
        restore(home, &journal).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"before");
    }

    #[test]
    fn hex_round_trips_and_rejects_malformed_text() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(from_hex(&to_hex(&all)).unwrap(), all);
        assert_eq!(from_hex("").unwrap(), Vec::<u8>::new());
        for bad in ["abc", "zz", "+1", "é1"] {
            assert!(from_hex(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn prune_keeps_the_newest_journals_and_leaves_other_files() {
        let dir = tempfile::tempdir().unwrap();
        for nonce in [1u128, 2, 3, 10, 11, 12, 13] {
            fs::write(dir.path().join(format!("{nonce}-7.json")), "{}").unwrap();
        }
        fs::write(dir.path().join("notes.json"), "x").unwrap();
        fs::write(dir.path().join("pending"), "x").unwrap();
        prune(dir.path(), 3);
        let mut left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        // Numeric order, so "10" is newer than "3".
        assert_eq!(
            left,
            [
                "11-7.json",
                "12-7.json",
                "13-7.json",
                "notes.json",
                "pending"
            ]
        );
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
