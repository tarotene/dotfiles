//! git-worktree-allow — herdr worktree を外から駆動する `git -C <dir> …` を
//! 許可する PreToolUse hook の判定エンジン(`config/claude/hooks/
//! git-worktree-allow.sh` の移植、ADR-0024)。設計と根拠:
//! docs/claude/git-worktree-allow.md。
//!
//! 許可条件(すべて満たすときだけ allow):
//! 1. 複合コマンド・リダイレクト・展開(`;` `&` `|` `$(` バッククォート `>` `<`
//!    改行)を含まない単一の呼び出し
//! 2. 形が `git -C <dir> <subcommand> …` に厳密一致
//! 3. `<dir>` は `-` 始まりでない実在ディレクトリで、symlink 解決後が
//!    worktrees root 配下(root 自身は不可)
//! 4. `<subcommand>` が許可リスト内
//! 5. 残りの引数に git を別実行体へ向けるもの(`--receive-pack` /
//!    `--upload-pack` / `--exec-path` / `ext::`)がない

use std::path::Path;

pub const ALLOWED_SUBCOMMANDS: [&str; 7] =
    ["status", "diff", "log", "show", "add", "commit", "push"];

/// worktrees root: `HERDR_WORKTREES_DIR`(空なら未設定扱い)か
/// `$HOME/.herdr/worktrees`。解決できなければ `None`。
pub fn worktrees_root() -> Option<String> {
    match std::env::var("HERDR_WORKTREES_DIR") {
        Ok(d) if !d.is_empty() => Some(d),
        _ => Some(format!("{}/.herdr/worktrees", std::env::var("HOME").ok()?)),
    }
}

/// 許可なら理由文、フォールスルーなら `None`。
pub fn decide(cmd: &str, root: &str) -> Option<String> {
    // 1. 複合コマンド・リダイレクト・展開の拒否
    if [";", "&", "|", "$(", "`", ">", "<", "\n"]
        .iter()
        .any(|p| cmd.contains(p))
    {
        return None;
    }

    // 2. 形の厳密一致(素朴な空白分割)
    let tok: Vec<&str> = cmd
        .split([' ', '\t', '\n'])
        .filter(|t| !t.is_empty())
        .collect();
    if tok.len() < 4 || tok[0] != "git" || tok[1] != "-C" {
        return None;
    }

    // 3. 実在する worktree 配下のディレクトリか(symlink は解決してから)
    let dir = tok[2];
    if dir.starts_with('-') {
        return None;
    }
    let real = std::fs::canonicalize(Path::new(dir)).ok()?;
    if !real.is_dir() {
        return None;
    }
    let real = real.to_str()?;
    // bash: case "$real/" in "$root"/?*) — root 自身は不可(? が 1 文字要る)。
    let real_slash = format!("{real}/");
    let prefix = format!("{root}/");
    if !(real_slash.starts_with(&prefix) && real_slash.len() > prefix.len()) {
        return None;
    }

    // 4. サブコマンドの許可リスト照合
    let sub = tok[3];
    if !ALLOWED_SUBCOMMANDS.contains(&sub) {
        return None;
    }

    // 5. git を別実行体へ向けられる引数の拒否
    if tok[4..].iter().any(|t| {
        t.starts_with("--receive-pack")
            || t.starts_with("--upload-pack")
            || t.starts_with("--exec-path")
            || t.contains("ext::")
    }) {
        return None;
    }

    Some(format!("git -C {real} {sub} (herdr worktree)"))
}
