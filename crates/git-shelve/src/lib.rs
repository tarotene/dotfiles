//! git-shelve / git-unshelve の共有部(旧 scripts/git-shelve, scripts/git-unshelve)。
//!
//! stash スタックは git ではリポジトリ単位なので、message に worktree の
//! 絶対パスをタグ(`shelve:<toplevel>:`)として埋め込んで所有権を示す。
//! `git shelve` / `git unshelve` はともに `<git-common-dir>/claude-shelve.lock`
//! を flock で保持して直列化する(設計根拠は docs/claude/git-stash-guard.md)。

use std::fs::{File, OpenOptions};
use std::io;
use std::process::{Command, Stdio};

/// 失敗した git の終了コードをそのまま返したい呼び出し側のための型。
pub type ExitCode = i32;

fn trim_nl(mut s: String) -> String {
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    s
}

/// `git <args>` の stdout を取る。stderr は継承する(bash の `$(git ...)` と同じ)。
/// 失敗時は git の終了コードを `Err` で返す。
pub fn git_capture(args: &[&str]) -> Result<String, ExitCode> {
    let out = Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| {
            eprintln!("git: {e}");
            127
        })?;
    if !out.status.success() {
        return Err(out.status.code().unwrap_or(1));
    }
    Ok(trim_nl(String::from_utf8_lossy(&out.stdout).into_owned()))
}

pub fn toplevel() -> Result<String, ExitCode> {
    git_capture(&["rev-parse", "--show-toplevel"])
}

/// `<git-common-dir>/claude-shelve.lock` のパス(絶対パスで返す)。
pub fn lockfile_path() -> Result<String, ExitCode> {
    let dir = git_capture(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Ok(format!("{dir}/claude-shelve.lock"))
}

/// ロックファイルを作成/open して排他 flock を取る(drop まで保持)。
/// bash の `exec 9> file; flock 9` 相当。
pub fn lock(path: &str) -> io::Result<File> {
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.lock()?;
    Ok(f)
}

/// この worktree の stash タグ。
pub fn tag(toplevel: &str) -> String {
    format!("shelve:{toplevel}:")
}

/// stash message を組み立てる。メモ無しはタグのみ。
pub fn message(toplevel: &str, note: &str) -> String {
    if note.is_empty() {
        tag(toplevel)
    } else {
        format!("shelve:{toplevel}: {note}")
    }
}

/// stash list の `"%H<TAB>%gs"` スナップショットから、タグを部分一致で含む
/// 最初(最新側)の行を返す(awk の `index($2, tag)`)。
pub fn find_entry(snapshot: &str, tag: &str) -> Option<String> {
    snapshot
        .lines()
        .find(|l| l.split('\t').nth(1).is_some_and(|f| f.contains(tag)))
        .map(str::to_string)
}

/// stash list の `"%gd<TAB>%H"` スナップショットから SHA の現在 index。
pub fn resolve_index(snapshot: &str, sha: &str) -> Option<String> {
    snapshot.lines().find_map(|l| {
        let mut f = l.split('\t');
        let gd = f.next()?;
        (f.next() == Some(sha)).then(|| gd.to_string())
    })
}

/// drop の出力から、削除された entry の SHA を取り出す
/// (`sed -n 's/.*(\([0-9a-f]\{40\}\))\.\{0,1\}$/\1/p'`)。複数行ならそれぞれ。
pub fn dropped_sha(out: &str) -> String {
    out.lines()
        .filter_map(|l| {
            let l = l.strip_suffix('.').unwrap_or(l);
            let l = l.strip_suffix(')')?;
            let at = l.len().checked_sub(40)?;
            let sha = l.get(at..)?;
            let head = l.get(..at)?;
            (head.ends_with('(')
                && sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            .then(|| sha.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `"%H<TAB>%gs"` スナップショットから SHA の元 message(2 番目のフィールド)。
pub fn message_for(snapshot: &str, sha: &str) -> Option<String> {
    snapshot.lines().find_map(|l| {
        let mut f = l.split('\t');
        (f.next() == Some(sha)).then(|| f.next().unwrap_or("").to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_forms() {
        assert_eq!(message("/w", ""), "shelve:/w:");
        assert_eq!(message("/w", "memo"), "shelve:/w: memo");
    }

    #[test]
    fn entry_matching_is_substring_of_field2_first_wins() {
        let snap =
            "aaa\tOn main: shelve:/other:\nbbb\tOn main: shelve:/w: x\nccc\tOn x: shelve:/w:";
        assert_eq!(
            find_entry(snap, "shelve:/w:").as_deref(),
            Some("bbb\tOn main: shelve:/w: x")
        );
        assert_eq!(find_entry("", "shelve:/w:"), None);
        // タグが field 1(SHA 側)にだけ現れても一致しない
        assert_eq!(find_entry("shelve:/w:\tfoo", "shelve:/w:"), None);
    }

    #[test]
    fn index_lookup() {
        let snap = "stash@{0}\taaa\nstash@{1}\tbbb";
        assert_eq!(resolve_index(snap, "bbb").as_deref(), Some("stash@{1}"));
        assert_eq!(resolve_index(snap, "zzz"), None);
    }

    #[test]
    fn dropped_sha_forms() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(dropped_sha(&format!("Dropped stash@{{0}} ({sha})")), sha);
        assert_eq!(
            dropped_sha(&format!("Dropped refs/stash@{{0}} ({sha}).")),
            sha
        );
        assert_eq!(dropped_sha(&format!("Dropped stash@{{0}} ({sha})..")), "");
        assert_eq!(dropped_sha("Dropped stash@{0} (abc)"), "");
        assert_eq!(dropped_sha(""), "");
        assert_eq!(dropped_sha(&format!("x ({})", sha.to_uppercase())), "");
    }

    #[test]
    fn message_lookup_takes_field2_only() {
        assert_eq!(
            message_for("aaa\tm1\nbbb\tm2", "bbb").as_deref(),
            Some("m2")
        );
        assert_eq!(message_for("aaa\tm1", "zzz"), None);
    }
}
