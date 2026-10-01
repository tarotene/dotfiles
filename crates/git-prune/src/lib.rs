//! `git prune-worktrees` / `git prune-branches` の共有部(ADR-0024、
//! `scripts/git-prune-worktrees` / `scripts/git-prune-branches` の移植)。
//!
//! 2 本の bin は元々別スクリプトで、どちらも別プロセスの
//! `git-audit-worktrees` を呼ぶ(プロセス境界は維持する)。重複していた
//! 非自明な部分 — audit の呼び出し、TSV 行の分割/再構成、TOCTOU 再検証用の
//! 行照合、`log.tsv` への追記、確認プロンプト、git の実行 — をここに寄せる。
//!
//! bash 版が `set -euo pipefail` で暗黙に持っていた「失敗したら git/audit の
//! 終了コードで即終了」は、`Result<_, i32>`(`Err` が終了コード)で表す。

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// `Err(code)` はその終了コードでの即時終了(bash の `set -e` 相当)。
pub type Exit = Result<(), i32>;

pub fn out(s: &str) {
    let _ = writeln!(io::stdout(), "{s}");
}

pub fn err(s: &str) {
    let _ = writeln!(io::stderr(), "{s}");
}

/// bash の `${VAR:-}`: 未設定と空文字を同一視する。
pub fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `${<LOG_ENV>:-${XDG_STATE_HOME:-$HOME/.local/state}/git-auto-prune}`。
pub fn log_dir(log_env: &str) -> Result<PathBuf, i32> {
    if let Some(d) = env_nonempty(log_env) {
        return Ok(PathBuf::from(d));
    }
    let base = match env_nonempty("XDG_STATE_HOME") {
        Some(x) => PathBuf::from(x),
        None => match std::env::var_os("HOME") {
            Some(h) => PathBuf::from(h).join(".local/state"),
            None => {
                err("HOME: unbound variable");
                return Err(1);
            }
        },
    };
    Ok(base.join("git-auto-prune"))
}

/// `${<AUDIT_ENV>:-git-audit-worktrees}`。PATH 解決は `Command` に任せる。
pub fn audit_bin(audit_env: &str) -> String {
    env_nonempty(audit_env).unwrap_or_else(|| "git-audit-worktrees".to_string())
}

fn code_of(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// `"$AUDIT_BIN" <arg>` の stdout を返す。非 0 終了は `Err(その終了コード)`
/// (bash: `$(...)` 代入失敗 / pipefail による `set -e` 終了)。見つからな
/// ければ 127。`quiet_stderr` は porcelain 用の `2>/dev/null`。
pub fn run_audit(bin: &str, arg: &str, quiet_stderr: bool) -> Result<String, i32> {
    let mut cmd = Command::new(bin);
    cmd.arg(arg)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(if quiet_stderr {
            Stdio::null()
        } else {
            Stdio::inherit()
        });
    let o = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            if !quiet_stderr {
                err(&format!("{bin}: {e}"));
            }
            return Err(127);
        }
    };
    if !o.status.success() {
        return Err(code_of(o.status));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `$(...)` が末尾の改行を落とした後の行列(空なら 0 行)。
pub fn lines_of(s: &str) -> Vec<&str> {
    let s = s.trim_end_matches('\n');
    if s.is_empty() {
        Vec::new()
    } else {
        s.split('\n').collect()
    }
}

/// `awk -F'\t' '$1 == "<kind>"'`: 1 列目が `kind` の行だけ(行は無加工)。
pub fn rows_of_kind<'a>(out: &'a str, kind: &str) -> Vec<&'a str> {
    lines_of(out)
        .into_iter()
        .filter(|l| l.split('\t').next() == Some(kind))
        .collect()
}

/// `tr '\t' $'\x1f'` してから `IFS=$'\x1f' read -r a b c ...`(n 変数)する
/// のと同じ: 連続タブを畳まず、余りは最後の変数に残る。足りない列は空。
pub fn split_us(line: &str, n: usize) -> Vec<String> {
    let mut v: Vec<String> = line.splitn(n, '\t').map(|s| s.to_string()).collect();
    if let Some(last) = v.last_mut() {
        // bash では最後の変数に残る区切りは \x1f のまま。
        *last = last.replace('\t', "\x1f");
    }
    v.resize(n, String::new());
    v
}

/// `IFS=$'\t' read -r a b c ...`(n 変数)の再現。タブは IFS 空白扱いなので
/// 先頭タブは落ち、連続タブは 1 つに畳まれ、最後の変数の末尾タブも落ちる。
/// git-prune-worktrees の orphaned 削除ループが実際にこの読み方をして
/// いる(render だけが \x1f 方式)ため、挙動を揃える。
pub fn read_tab_ws(line: &str, n: usize) -> Vec<String> {
    let mut rest = line.trim_start_matches('\t');
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i == n - 1 {
            out.push(rest.trim_end_matches('\t').to_string());
            rest = "";
        } else if let Some(p) = rest.find('\t') {
            out.push(rest[..p].to_string());
            rest = rest[p..].trim_start_matches('\t');
        } else {
            out.push(rest.to_string());
            rest = "";
        }
    }
    out
}

/// `grep -qxF "$line" <<<"$fresh"`: fresh のどれかの行と完全一致。
pub fn has_exact_line(fresh: &[&str], line: &str) -> bool {
    fresh.contains(&line)
}

/// `awk ... | LC_ALL=C sort -u` で一意化した共通 dir(空は除く)。
pub fn sorted_unique(items: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    items.into_iter().filter(|s| !s.is_empty()).collect()
}

/// git を実行し stdout+stderr を合流した出力(末尾改行除去)と成否を返す。
/// bash の `err="$(git ... 2>&1)"` 相当。順序まで揃えるため sh 経由で
/// `2>&1` する。
pub fn git_merged(args: &[&str]) -> (bool, String) {
    let o = Command::new("sh")
        .args(["-c", "exec git \"$@\" 2>&1", "sh"])
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output();
    match o {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string(),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// git を出力そのまま(継承)で実行。失敗は `Err(終了コード)`(`set -e`)。
pub fn git_inherit(args: &[&str]) -> Exit {
    match Command::new("git").args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(code_of(s)),
        Err(e) => {
            err(&format!("git: {e}"));
            Err(127)
        }
    }
}

/// git の stdout を返す(stderr は継承)。失敗は `Err(終了コード)`。
pub fn git_stdout(args: &[&str]) -> Result<String, i32> {
    let o = Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output();
    match o {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(code_of(o.status)),
        Err(e) => {
            err(&format!("git: {e}"));
            Err(127)
        }
    }
}

/// `GIT_PRUNE_*_TEST_PRE_ACT_HOOK`(テスト専用シーム、通常運用では未設定)。
/// bash 版の `eval "$HOOK"` 相当(`bash -c`)。非 0 は `set -e` で終了。
pub fn run_pre_act_hook(hook_env: &str) -> Exit {
    if let Some(h) = env_nonempty(hook_env) {
        match Command::new("bash").arg("-c").arg(h).status() {
            Ok(s) if s.success() => {}
            Ok(s) => return Err(code_of(s)),
            Err(e) => {
                err(&format!("bash: {e}"));
                return Err(127);
            }
        }
    }
    Ok(())
}

/// `date +%Y-%m-%dT%H:%M:%S%z`(ローカル時刻・数値オフセット)。bash 版と
/// 同じ出力にするため `date` に任せる(std にローカル時刻 API は無い)。
fn timestamp() -> String {
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%z")
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default()
}

/// 共有 auto-prune ログへ 1 行追記する。列: timestamp, kind, 以降 `cols`。
/// `mkdir -p "$LOG_DIR"` → `>>log.tsv`。失敗は `Err(1)`。
pub fn log_auto_prune(log_dir: &std::path::Path, kind: &str, cols: &[&str]) -> Exit {
    if let Err(e) = fs::create_dir_all(log_dir) {
        err(&format!("mkdir: {}: {e}", log_dir.display()));
        return Err(1);
    }
    let path = log_dir.join("log.tsv");
    let mut line = format!("{}\t{kind}", timestamp());
    for c in cols {
        line.push('\t');
        line.push_str(c);
    }
    line.push('\n');
    let r = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = r {
        err(&format!("{}: {e}", path.display()));
        return Err(1);
    }
    Ok(())
}

/// `read -r [-p PROMPT] ans` の再現。stdin が端末のときだけ PROMPT を
/// stderr に出す(bash の `read -p` と同じ)。`(前後の IFS 空白を落とした
/// 入力, 改行で終端されていたか)` を返す。bash の read は改行なしの EOF で
/// 非 0 を返しつつ変数には部分入力を入れる — 呼び出し側が使い分ける。
pub fn read_answer(prompt: &str) -> (String, bool) {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        let _ = write!(io::stderr(), "{prompt}");
        let _ = io::stderr().flush();
    }
    let mut buf = String::new();
    let _ = stdin.lock().read_line(&mut buf);
    let terminated = buf.ends_with('\n');
    let ans = buf
        .trim_matches(|c| c == ' ' || c == '\t' || c == '\n')
        .to_string();
    (ans, terminated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_us_keeps_empty_middle_field() {
        assert_eq!(
            split_us("worktree\tc\tr\tp\t\tsha\tC3:#7", 7),
            vec!["worktree", "c", "r", "p", "", "sha", "C3:#7"]
        );
    }

    #[test]
    fn split_us_pads_missing_columns() {
        assert_eq!(split_us("a\tb", 4), vec!["a", "b", "", ""]);
    }

    #[test]
    fn read_tab_ws_collapses_consecutive_tabs() {
        // 空の branch 列が畳まれて reason が 3 番目に滑る(bash 版と同じ)。
        assert_eq!(
            read_tab_ws("repo\tpath\t\treason", 4),
            vec!["repo", "path", "reason", ""]
        );
    }

    #[test]
    fn rows_of_kind_filters_on_first_column() {
        let s = "worktree\ta\nbranch\tb\n\nworktree\tc\n";
        assert_eq!(
            rows_of_kind(s, "worktree"),
            vec!["worktree\ta", "worktree\tc"]
        );
    }
}
