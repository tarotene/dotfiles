//! git-worktree-create-guard — 直接の `git worktree add` を弾く PreToolUse hook
//! の判定エンジン(`scripts/git-worktree-create-guard` の移植、ADR-0024)。
//! worktree の作成と後始末は herdr が一緒に所有する。

pub const DENY_REASON: &str = "直接の git worktree add は一時ディレクトリ削除後に stale 登録を残すため拒否しました。herdr worktree create --cwd <repo> --branch <name> を使ってください。";

/// grep -E `worktree[[:space:]]+add` 相当(grep は行単位なので行をまたがない)。
pub fn has_worktree_add(s: &str) -> bool {
    s.split('\n').any(|line| {
        line.match_indices("worktree").any(|(i, m)| {
            let rest = &line[i + m.len()..];
            let t = rest.trim_start_matches(char::is_whitespace);
            t.len() < rest.len() && t.starts_with("add")
        })
    })
}

/// 複合コマンドを含まない単一の文 1 つ。`git [global-opts] worktree add` なら
/// deny 理由。
pub fn decide_segment(segment: &str) -> Option<&'static str> {
    let words: Vec<&str> = segment
        .split([' ', '\t', '\n'])
        .filter(|w| !w.is_empty())
        .collect();
    if words.first() != Some(&"git") {
        return None;
    }
    let mut idx = 1;
    while idx < words.len() {
        match words[idx] {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" => idx += 2,
            "--bare" | "--no-pager" | "--literal-pathspecs" => idx += 1,
            w if w.starts_with("--git-dir=")
                || w.starts_with("--work-tree=")
                || w.starts_with("--namespace=") =>
            {
                idx += 1
            }
            _ => break,
        }
    }
    (words.get(idx) == Some(&"worktree") && words.get(idx + 1) == Some(&"add"))
        .then_some(DENY_REASON)
}

/// Bash ツールのコマンド文字列全体。deny なら理由文。
pub fn decide(command: &str) -> Option<&'static str> {
    // heredoc の本文はコマンドではない(`python3 - <<'E'` の本文の行頭に
    // `git worktree add` があっても実行されない)。判定の前に本文を落とす。
    let command = guard_core::shell::split_heredoc(command).command;
    let command = command.as_str();
    if !has_worktree_add(command) {
        return None;
    }
    let normalized = command
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace([';', '|'], "\n");
    normalized
        .split('\n')
        .find_map(|seg| decide_segment(seg.trim_start_matches(char::is_whitespace)))
}

/// hook 本体(`main` が標準入力を読んだあとの処理)。`main` と、ルータ(`crates/hook-router`)が
/// 同じ関数を呼ぶ。入力が読めない・不正 JSON などは `None`(判定しない)。
pub fn hook(raw: &str) -> Option<hook_io::Outcome> {
    // 事前フィルタは `decide` がパース後の command に対して行う。生の JSON に対して見ると、
    // タブが `\t` にエスケープされて単語境界が崩れ、`git<TAB>stash` が素通りする(#637)。
    let input = hook_io::HookInput::parse(raw)?;
    let cmd = input.bash_command()?;
    // bash 版は `$(jq …)` で末尾改行が落ちる。
    let cmd = cmd.trim_end_matches('\n');
    if cmd.is_empty() {
        return None;
    }
    decide(cmd).map(|r| hook_io::Outcome::Deny(r.to_string()))
}
