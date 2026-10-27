//! Obsidian vault のノートを扱う共通部品(wrap-up の気づき・retro・裁定待ちの写しが使う)。
//!
//! 設計と根拠: docs/agents/wrapup-inbox.md、docs/adr/0000-wrapup-inbox-to-vault.md。
//!
//! vault は LiveSync でスマホと同期されるので、書き方は次の 3 つに限る。
//!
//! - 新しいノートの作成と、状態を変えるときの書き換えは、同じディレクトリの一時ファイルに
//!   書いて fsync し、rename で置き換える(途中の状態を見せない)。内容が同じなら書かない
//!   (同期を無駄に走らせない)。
//! - 追記は `O_APPEND` で 1 回の write にする。
//! - 排他ロックは vault の**外**に置く。rename でノートの inode が変わるので、ノート自体に
//!   かけた flock では排他にならないため。
//!
//! vault の場所は hub-resolve の `OBSIDIAN_VAULT`(環境変数、無ければマーカー)で解く。
//! 宣言していない機体では [`VaultNotes::open`] が `None` を返し、呼び出し側は何もしないか
//! 従来の経路に戻る。宣言しているのに読めないときは `Some(Err(_))` で、黙って落とさない。

use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

/// ノートのファイル名(拡張子を除く)の上限(バイト)。
const NAME_MAX_BYTES: usize = 150;

// ---------------------------------------------------------------- vault の解決

/// 宣言されている vault の場所。`None` は宣言なし(環境変数もマーカーも無い)。
pub fn resolve_with(getenv: hub_resolve::Getenv) -> Option<Result<PathBuf, String>> {
    let hub = &hub_resolve::OBSIDIAN_VAULT;
    let env_set = getenv(hub.env_var).is_some_and(|v| !v.is_empty());
    let marker_set = fs::metadata(hub_resolve::marker_path(hub, getenv)).is_ok();
    if !env_set && !marker_set {
        return None;
    }
    Some(
        hub_resolve::resolve(hub, getenv)
            .map(PathBuf::from)
            .map_err(|m| m.trim_end().to_string()),
    )
}

/// 実際の環境で [`resolve_with`] する。
pub fn resolve() -> Option<Result<PathBuf, String>> {
    resolve_with(&|k| std::env::var(k).ok())
}

/// vault の中のノートへの入口。ロックの置き場(`lock_dir`)は vault の外で、呼び出し側が渡す。
#[derive(Debug, Clone)]
pub struct VaultNotes {
    root: PathBuf,
    lock_dir: PathBuf,
}

impl VaultNotes {
    /// 宣言された vault を開く。宣言が無ければ `None`。
    pub fn open(lock_dir: &Path) -> Option<Result<VaultNotes, String>> {
        Some(resolve()?.map(|root| VaultNotes::at(&root, lock_dir)))
    }

    /// 場所を指定して開く(試験用、または呼び出し側が解決済みのとき)。
    pub fn at(root: &Path, lock_dir: &Path) -> VaultNotes {
        VaultNotes {
            root: root.to_path_buf(),
            lock_dir: lock_dir.to_path_buf(),
        }
    }

    /// 排他ロックを取る。以降の読み書きはすべて返り値の [`Locked`] を通す。
    /// 同じプロセスの中で入れ子にロックしない(別の fd の flock は自分自身をも待たせる)。
    pub fn lock(&self) -> io::Result<Locked<'_>> {
        fs::create_dir_all(&self.lock_dir)?;
        let f = File::options()
            .create(true)
            .append(true)
            .open(self.lock_dir.join("vault.lock"))?;
        f.lock()?;
        Ok(Locked {
            vault: self,
            _lock: f,
        })
    }
}

/// ロックを持っている間だけ使える操作。`rel` は vault からの相対パスで、絶対パスと `..` は拒否する。
pub struct Locked<'a> {
    vault: &'a VaultNotes,
    _lock: File,
}

fn check_rel(rel: &str) -> io::Result<()> {
    let p = Path::new(rel);
    let bad = rel.is_empty()
        || p.is_absolute()
        || p.components().any(|c| !matches!(c, Component::Normal(_)));
    if bad {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("vault 内の相対パスではない: {rel}"),
        ));
    }
    Ok(())
}

impl Locked<'_> {
    fn path(&self, rel: &str) -> io::Result<PathBuf> {
        check_rel(rel)?;
        Ok(self.vault.root.join(rel))
    }

    /// 読む。無ければ `None`。
    pub fn read(&self, rel: &str) -> io::Result<Option<String>> {
        match fs::read_to_string(self.path(rel)?) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 一時ファイル + rename で置き換える。内容が同じなら何もせず `false`。
    pub fn replace(&self, rel: &str, content: &str) -> io::Result<bool> {
        let path = self.path(rel)?;
        if fs::read(&path).is_ok_and(|old| old == content.as_bytes()) {
            return Ok(false);
        }
        let dir = path.parent().expect("rel は空でないので親がある");
        fs::create_dir_all(dir)?;
        let name = path.file_name().expect("rel は空でないので名前がある");
        let tmp = dir.join(format!(
            ".{}.tmp-{}",
            name.to_string_lossy(),
            std::process::id()
        ));
        let result = (|| -> io::Result<()> {
            let mut f = File::options()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o644)
                .open(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.sync_all()?;
            fs::rename(&tmp, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map(|()| true)
    }

    /// 末尾に 1 行足す(`line` に改行は含めない)。無ければ作る。末尾に改行が無いファイルには、
    /// 先に改行を足してから書く(行が前の行に連結しない)。
    pub fn append_line(&self, rel: &str, line: &str) -> io::Result<()> {
        if line.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "追記する行に改行は含められない",
            ));
        }
        let path = self.path(rel)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let needs_newline = fs::read(&path).is_ok_and(|b| b.last().is_some_and(|c| *c != b'\n'));
        let mut f = File::options().create(true).append(true).open(&path)?;
        let text = if needs_newline {
            format!("\n{line}\n")
        } else {
            format!("{line}\n")
        };
        f.write_all(text.as_bytes())
    }

    /// 消す。無ければ `false`。
    pub fn remove(&self, rel: &str) -> io::Result<bool> {
        match fs::remove_file(self.path(rel)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// ディレクトリ直下の `.md` ファイル名(拡張子つき)を、名前順で返す。無ければ空。
    /// 先頭が `.` のファイル(一時ファイル)は含めない。
    pub fn list_md(&self, rel_dir: &str) -> io::Result<Vec<String>> {
        let dir = self.path(rel_dir)?;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".md") && !n.starts_with('.'))
            .collect();
        names.sort();
        Ok(names)
    }
}

// ---------------------------------------------------------------- ノート名

/// FNV-1a(64 bit)の先頭 8 桁。Rust の版に依らず同じ値になる(名前の同一性に使うため)。
pub fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", h)[..8].to_string()
}

/// 題名からノート名(拡張子を除く)を作る。ファイル名とリンクに使えない文字(`\ / : * ? " < > | # ^ [ ]`
/// と制御文字)は `-` に置き換え、前後の空白と先頭の `.` を除く。`NAME_MAX_BYTES` を超えるときは
/// 文字の境界で切り、末尾に ` ~<hash 8 桁>` を付ける。
pub fn note_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|#^[]".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    let cleaned = if cleaned.is_empty() {
        "untitled".to_string()
    } else {
        cleaned
    };
    if cleaned.len() <= NAME_MAX_BYTES {
        return cleaned;
    }
    let suffix = format!(" ~{}", short_hash(title));
    let mut end = NAME_MAX_BYTES - suffix.len();
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", cleaned[..end].trim_end())
}

// ---------------------------------------------------------------- frontmatter

/// frontmatter(`---` で囲んだ先頭の `key: value` の列)と本文。値はすべて文字列として扱い、
/// 書くときは JSON の二重引用符文字列にする(YAML としても正しい)。キーの順と、知らないキーは保つ。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Note {
    pub front: Vec<(String, String)>,
    pub body: String,
}

fn parse_value(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with('"') {
        if let Ok(s) = serde_json::from_str::<String>(raw) {
            return s;
        }
    }
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return raw[1..raw.len() - 1].replace("''", "'");
    }
    raw.to_string()
}

impl Note {
    /// 読む。先頭が `---` でなければ、frontmatter なしで全体を本文とする。閉じる `---` が無いときも同じ。
    pub fn parse(text: &str) -> Note {
        let Some(rest) = text.strip_prefix("---\n") else {
            return Note {
                front: Vec::new(),
                body: text.to_string(),
            };
        };
        let mut front = Vec::new();
        let mut consumed = 0;
        for line in rest.split_inclusive('\n') {
            consumed += line.len();
            let t = line.trim_end_matches(['\n', '\r']);
            if t == "---" {
                return Note {
                    front,
                    body: rest[consumed..].to_string(),
                };
            }
            if let Some((k, v)) = t.split_once(':') {
                if !k.is_empty() && !k.starts_with([' ', '\t', '#']) {
                    front.push((k.trim().to_string(), parse_value(v)));
                }
            }
        }
        Note {
            front: Vec::new(),
            body: text.to_string(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.front
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// 値を設定する。キーが無ければ末尾に足す。
    pub fn set(&mut self, key: &str, value: &str) {
        match self.front.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value.to_string(),
            None => self.front.push((key.to_string(), value.to_string())),
        }
    }

    /// ノートの全文。frontmatter が空なら本文だけ。
    pub fn render(&self) -> String {
        if self.front.is_empty() {
            return self.body.clone();
        }
        let mut out = String::from("---\n");
        for (k, v) in &self.front {
            out.push_str(k);
            out.push_str(": ");
            out.push_str(&serde_json::to_string(v).expect("文字列は JSON になる"));
            out.push('\n');
        }
        out.push_str("---\n");
        out.push_str(&self.body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_name_replaces_forbidden_characters() {
        assert_eq!(note_name("fix: a/b [c] #d"), "fix- a-b -c- -d");
        assert_eq!(note_name("  .hidden  "), "hidden");
        assert_eq!(note_name("///"), "---");
        assert_eq!(note_name(""), "untitled");
        assert_eq!(note_name("a\nb"), "a-b");
    }

    #[test]
    fn note_name_truncates_on_a_char_boundary_with_a_stable_hash() {
        let title = "あ".repeat(100); // 300 バイト
        let n = note_name(&title);
        assert!(n.len() <= NAME_MAX_BYTES, "{}", n.len());
        assert!(n.ends_with(&format!(" ~{}", short_hash(&title))));
        assert_eq!(n, note_name(&title));
        // 同じ長い接頭辞で、後ろだけ違う題名は別の名前になる。
        let other = format!("{}い", "あ".repeat(100));
        assert_ne!(note_name(&other), n);
        // 短いときは hash を付けない。
        assert!(!note_name("short").contains('~'));
    }

    #[test]
    fn short_hash_is_fixed() {
        // FNV-1a 64 の既知値("a" → af63dc4c8601ec8c)の先頭 8 桁。
        assert_eq!(short_hash("a"), "af63dc4c");
    }

    #[test]
    fn frontmatter_round_trips_and_keeps_the_body_bytes() {
        let text =
            "---\ntitle: \"a \\\"b\\\"\"\nrepo: \"o/r\"\nextra: \"x\"\n---\n- 2026-01-01 one\n- 2026-01-02 two";
        let n = Note::parse(text);
        assert_eq!(n.get("title"), Some("a \"b\""));
        assert_eq!(n.get("extra"), Some("x"));
        assert_eq!(n.body, "- 2026-01-01 one\n- 2026-01-02 two");
        assert_eq!(n.render(), text);
    }

    #[test]
    fn frontmatter_accepts_unquoted_values_written_by_obsidian() {
        let n = Note::parse("---\nstatus: open\nrepo: 'o/r'\n---\nbody\n");
        assert_eq!(n.get("status"), Some("open"));
        assert_eq!(n.get("repo"), Some("o/r"));
        // 書き戻すと JSON の引用符に揃う。本文はそのまま。
        let mut n = n;
        n.set("status", "fixed");
        assert_eq!(
            n.render(),
            "---\nstatus: \"fixed\"\nrepo: \"o/r\"\n---\nbody\n"
        );
    }

    #[test]
    fn a_note_without_frontmatter_is_all_body() {
        let n = Note::parse("no front\n");
        assert!(n.front.is_empty());
        assert_eq!(n.render(), "no front\n");
        let unclosed = Note::parse("---\ntitle: \"x\"\nbody\n");
        assert!(unclosed.front.is_empty());
        assert_eq!(unclosed.render(), "---\ntitle: \"x\"\nbody\n");
    }

    fn vault() -> (tempfile::TempDir, VaultNotes) {
        let d = tempfile::tempdir().unwrap();
        let v = VaultNotes::at(&d.path().join("vault"), &d.path().join("state"));
        (d, v)
    }

    #[test]
    fn replace_is_atomic_and_skips_identical_content() {
        let (d, v) = vault();
        let g = v.lock().unwrap();
        assert!(g.replace("inbox/a.md", "one").unwrap());
        let path = d.path().join("vault/inbox/a.md");
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!g.replace("inbox/a.md", "one").unwrap());
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        assert!(g.replace("inbox/a.md", "two").unwrap());
        assert_eq!(g.read("inbox/a.md").unwrap().as_deref(), Some("two"));
        // 一時ファイルは残らず、列挙にも出ない。
        assert_eq!(g.list_md("inbox").unwrap(), vec!["a.md".to_string()]);
        let leftovers: Vec<_> = fs::read_dir(d.path().join("vault/inbox"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn append_line_joins_cleanly_after_a_missing_trailing_newline() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        g.append_line("n.md", "- a").unwrap();
        g.replace("m.md", "no newline").unwrap();
        g.append_line("m.md", "- b").unwrap();
        assert_eq!(g.read("n.md").unwrap().as_deref(), Some("- a\n"));
        assert_eq!(
            g.read("m.md").unwrap().as_deref(),
            Some("no newline\n- b\n")
        );
        assert!(g.append_line("n.md", "x\ny").is_err());
    }

    #[test]
    fn paths_must_stay_inside_the_vault() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        for bad in ["../x.md", "/etc/x", "", "a/../../x"] {
            assert!(g.read(bad).is_err(), "{bad}");
            assert!(g.replace(bad, "x").is_err(), "{bad}");
        }
        assert!(!g.remove("missing.md").unwrap());
    }

    #[test]
    fn concurrent_appends_lose_no_line() {
        let (_d, v) = vault();
        let v = std::sync::Arc::new(v);
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let v = v.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        let g = v.lock().unwrap();
                        g.append_line("c.md", &format!("- {t}-{i}")).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let g = v.lock().unwrap();
        assert_eq!(g.read("c.md").unwrap().unwrap().lines().count(), 200);
    }

    #[test]
    fn resolve_reports_undeclared_as_none_and_a_broken_declaration_as_err() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().to_string_lossy().into_owned();
        let none = |k: &str| match k {
            "HOME" => Some(home.clone()),
            _ => None,
        };
        assert!(resolve_with(&none).is_none());

        let gone = d.path().join("nope").to_string_lossy().into_owned();
        let broken = |k: &str| match k {
            "OBSIDIAN_VAULT" => Some(gone.clone()),
            "HOME" => Some(home.clone()),
            _ => None,
        };
        assert!(resolve_with(&broken).unwrap().is_err());

        let vault = d.path().join("v");
        fs::create_dir_all(vault.join(".obsidian")).unwrap();
        fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
        let vp = vault.to_string_lossy().into_owned();
        let ok = |k: &str| match k {
            "OBSIDIAN_VAULT" => Some(vp.clone()),
            "HOME" => Some(home.clone()),
            _ => None,
        };
        assert_eq!(resolve_with(&ok).unwrap().unwrap(), vault);

        // マーカーだけでも宣言になる。
        let marker_dir = d.path().join("cfg/dotfiles");
        fs::create_dir_all(&marker_dir).unwrap();
        fs::write(marker_dir.join("obsidian-vault"), format!("{vp}\n")).unwrap();
        let cfg = d.path().join("cfg").to_string_lossy().into_owned();
        let by_marker = |k: &str| match k {
            "XDG_CONFIG_HOME" => Some(cfg.clone()),
            "HOME" => Some(home.clone()),
            _ => None,
        };
        assert_eq!(resolve_with(&by_marker).unwrap().unwrap(), vault);
    }
}
