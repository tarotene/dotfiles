//! git-stash-guard — 素の `git stash` を弾く PreToolUse hook の判定エンジン。
//!
//! `config/claude/hooks/git-stash-guard.sh` と Codex adapter
//! `config/codex/hooks/git-stash-guard.sh` の移植(ADR-0024)。両 host は
//! 判定も出力形(`hookSpecificOutput`)も同じなので 1 つの bin が
//! `--host claude|codex` を受ける。設計と根拠: docs/claude/git-stash-guard.md。
//!
//! 許可: `git stash list|show` / `push -u -m <tag>` / `apply <SHA>` /
//! `drop <SHA>`。それ以外の stash 呼び出しは deny。トークン分割は素朴な
//! 空白分割(bash 版と同じ割り切り)で、`git stash …` と
//! `git -C <dir> stash …` の 2 形だけを扱う。

/// grep の `-w` 相当: `word` が、前後を単語構成文字(英数字・`_`)で挟まれず
/// に現れるか。
pub fn contains_word(hay: &str, word: &str) -> bool {
    hay.match_indices(word).any(|(i, m)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + m.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// bash の `read -r -a` 既定 IFS(空白・タブ・改行)による分割。
fn tokens(s: &str) -> Vec<&str> {
    s.split([' ', '\t', '\n'])
        .filter(|t| !t.is_empty())
        .collect()
}

fn has_flag(long: &str, short: &str, args: &[&str]) -> bool {
    let long_eq = format!("{long}=");
    args.iter()
        .any(|a| *a == long || a.starts_with(&long_eq) || *a == short)
}

fn has_sha(args: &[&str]) -> bool {
    args.iter()
        .any(|a| a.len() == 40 && a.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// 複合コマンドを含まない単一の呼び出し 1 つ。deny なら理由文。
pub fn decide_single(seg: &str) -> Option<String> {
    let tok = tokens(seg);
    if tok.first() != Some(&"git") {
        return None;
    }
    let mut idx = 1;
    if tok.get(1) == Some(&"-C") {
        idx = 3;
    }
    if idx >= tok.len() || tok[idx] != "stash" {
        return None;
    }
    let args = &tok[idx + 1..];
    let sub = args.first().copied().unwrap_or("push");
    let rest = args.get(1..).unwrap_or(&[]);

    match sub {
        "list" | "show" => None,
        "push" => {
            if has_flag("--include-untracked", "-u", rest) && has_flag("--message", "-m", rest) {
                return None;
            }
            Some("git stash push は -u と -m <tag> を両方付けてください(deny)。この worktree の退避は git shelve \"<メモ>\" で積めます(推奨)。手動なら -u と -m <tag> を両方付けてください。".to_string())
        }
        "apply" => {
            if has_sha(rest) {
                return None;
            }
            Some("git stash apply は SHA を明示してください(deny)。この worktree の退避を戻すなら git unshelve を使ってください。手動なら git stash list --format=\"%H %gs\" で確認してから git stash apply <SHA> としてください。".to_string())
        }
        "drop" => {
            if has_sha(rest) {
                // パス境界としては通す(git 自身がこの形を "not a stash
                // reference" として拒否する — bash 版のコメント参照)。
                return None;
            }
            Some("git stash drop は SHA を明示してください(deny)。この worktree の退避を消すだけなら git unshelve が apply と drop をまとめて安全に行います。".to_string())
        }
        "pop" => Some("git stash pop は他の worktree の WIP を巻き込みます(deny)。この worktree の退避は git unshelve で戻せます。".to_string()),
        "clear" => Some("git stash clear は他の worktree の WIP を含めて全消去します(deny)。個別に戻すなら git unshelve、内容を確認したいだけなら git stash list / show を使ってください。".to_string()),
        _ => Some(format!("未知の git stash 呼び出しです(deny): {seg}")),
    }
}

/// `(^|[^[:alnum:]_])git[[:space:]]+(-C[[:space:]]+[^[:space:]]+[[:space:]]+)?stash([^[:alnum:]_]|$)`
/// に一致する箇所があるか(regex クレートを入れずに手で書く)。
fn has_git_stash_invocation(seg: &str) -> bool {
    fn stash_follow(x: &str) -> bool {
        x.strip_prefix("stash")
            .is_some_and(|r| !r.chars().next().is_some_and(is_word_char))
    }
    fn skip_ws(x: &str) -> Option<&str> {
        let t = x.trim_start_matches(char::is_whitespace);
        (t.len() < x.len()).then_some(t)
    }
    seg.match_indices("git").any(|(i, m)| {
        if seg[..i].chars().next_back().is_some_and(is_word_char) {
            return false;
        }
        let Some(t) = skip_ws(&seg[i + m.len()..]) else {
            return false;
        };
        let with_group = t.strip_prefix("-C").and_then(|r| {
            let r = skip_ws(r)?;
            let r2 = r.trim_start_matches(|c: char| !c.is_whitespace());
            if r2.len() == r.len() {
                return None;
            }
            skip_ws(r2)
        });
        with_group.is_some_and(stash_follow) || stash_follow(t)
    })
}

/// Bash ツールのコマンド文字列全体。deny なら理由文。
pub fn decide(cmd: &str) -> Option<String> {
    if !contains_word(cmd, "stash") {
        return None;
    }
    let normalized = cmd
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace([';', '|'], "\n");
    for seg in normalized.split('\n') {
        if seg.is_empty() {
            continue;
        }
        if seg.contains("$(") || seg.contains('`') || seg.contains('>') || seg.contains('<') {
            if has_git_stash_invocation(seg) {
                return Some(format!(
                    "複合コマンドの中に git stash が含まれています(deny): {seg}"
                ));
            }
            continue;
        }
        if let Some(reason) = decide_single(seg) {
            return Some(reason);
        }
    }
    None
}
