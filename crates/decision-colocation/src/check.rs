//! 決定成果物(ADR / 設計文書 / skill)の新規追加、または既存 ADR への
//! `## Amendment` 追加に、その決定を執行する実ファイルの同梱を要求する
//! 判定エンジン(ADR-396、docs/claude/decision-colocation.md)。
//!
//! bash 版 `scripts/decision-colocation-check` の移植。client guard
//! (`decision-colocation-guard`)と CI required check
//! (`decision-colocation-check`)の両方がこの [`run_check`] を呼ぶ —
//! 判定ロジックは 1 箇所にしか存在しない(ADR-396 の「単一ソース」)。
//!
//! 背景: この repo で追跡可能な ADR 27 本のうち 12 本が docs-only で着地し、
//! うち 2 本(ADR-0024/ADR-0013)は「後続 Issue に段階分割する」と明記した
//! まま追跡 Issue が一度も作られなかった。この検査は「決定成果物だけを
//! 残して実装を後続 Issue に分離する」経路を、機械的に成立不能にする。
//!
//! トリガ(base との diff が次のいずれかを含むとき発火):
//! 1. `docs/adr/**` / `docs/claude/**` / `config/claude/skills/**` への
//!    新規ファイル追加(`--diff-filter=A`)
//! 2. 既存 `docs/adr/*.md` への `^## Amendment` 見出しの追加
//!    (diff の追加行に現れる見出し行そのものを新規と見なす)
//!
//! 検査:
//! - トリガ1が `docs/adr/*.md` の新規追加のとき、そのファイルは
//!   `## 執行点` 節を持つこと。
//! - トリガ2のとき、その Amendment 見出しのブロックは `### 執行点` を持つこと。
//! - `## 執行点` / `### 執行点` に列挙された全パスが実在すること(壊れた参照の
//!   禁止)。
//! - そのうち少なくとも 1 つが、非 `.md` かつ `docs/` 配下でないパスであり、
//!   かつこの PR の diff(base..HEAD の変更ファイル)に含まれること。実在する
//!   が無変更のパスの併記だけでは合格しない(「既存機構を再利用する」という
//!   自己申告と、実際にそれを検証する変更を区別できないため — ADR-387 が実例)。
//! - トリガ1が `docs/adr/` 以外(`docs/claude/**` または
//!   `config/claude/skills/**` の新規)のときは、節を要求せず、PR の diff 全体に
//!   「非 `.md` かつ非 `docs/`」のパスが 1 つ以上あることだけを検査する。
//! - どのトリガにも触れない diff(索引修繕・typo・既存ファイルの編集のみ)は
//!   無条件で適合。既存 ADR への遡及適用はしない。
//!
//! `## 執行点` の書式: 見出し直下に 1 行 1 パスの箇条書き(バッククォート任意)。
//! パスの後にスペース区切りで説明文を続けてよい(最初の空白区切りトークンだけを
//! パスとして読む)。
//!
//! 判定不能の倒し方(bash 版と同じ): git の diff が失敗したら(`--base` が
//! 解決できない等)空の diff として扱い、適合にする(fail-open)。
//!
//! bash 版は awk / grep / `[[ =~ ]]` で書かれていた。ここでは同じ規則を
//! 手書きの行判定に置き換えている — 正規表現クレートは起動時間のために
//! 足さない(guard-core と同じ方針)。各関数の doc に元の正規表現を残す。

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// 違反メッセージの共通接頭辞(bash 版の `echo "decision-colocation-check: …"`)。
const PREFIX: &str = "decision-colocation-check: ";

/// `[[:space:]]`(C ロケールの isspace: 空白・\t \n \v \f \r)。
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

// ---- 経路の述語 -----------------------------------------------------------

/// 執行点として認めるパスか: 非 `.md` かつ非 `docs/` 配下(パス分類台帳を
/// 持たず、この 2 つの述語だけで判定する — docs/claude/decision-colocation.md
/// の D3)。bash の `case` は `*` が `/` にも一致するので、`docs/*` は
/// 前方一致、`*.md` は後方一致。
pub fn is_execution_point_path(p: &str) -> bool {
    !(p.ends_with(".md") || p.starts_with("docs/"))
}

/// `docs/adr/*.md`(新規 ADR のパス)。
pub fn is_new_adr_path(p: &str) -> bool {
    p.starts_with("docs/adr/") && p.ends_with(".md")
}

/// `docs/claude/*` / `config/claude/skills/*`(ADR 以外の決定成果物)。
pub fn is_non_adr_artifact_path(p: &str) -> bool {
    p.starts_with("docs/claude/") || p.starts_with("config/claude/skills/")
}

// ---- git ------------------------------------------------------------------

/// `git -C <repo> <args…>` の標準出力。失敗(`--base` が解決できない等)は
/// 空文字 — bash の `2> /dev/null || true`。
fn git_out(repo: &Path, args: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// base..HEAD で追加されたファイル(`--diff-filter=A`)。
fn list_added_files(repo: &Path, base: &str) -> String {
    git_out(
        repo,
        &[
            "diff",
            "--name-only",
            "--diff-filter=A",
            base,
            "HEAD",
            "--",
            ".",
        ],
    )
}

/// base..HEAD の変更ファイルすべて(追加・変更・削除・改名)。
fn list_changed_files(repo: &Path, base: &str) -> String {
    git_out(repo, &["diff", "--name-only", base, "HEAD", "--", "."])
}

/// base..HEAD で内容が変更された(追加でも削除でもない)`docs/adr/` 配下。
fn list_modified_adrs(repo: &Path, base: &str) -> String {
    git_out(
        repo,
        &[
            "diff",
            "--name-only",
            "--diff-filter=M",
            base,
            "HEAD",
            "--",
            "docs/adr/",
        ],
    )
}

/// base に無く HEAD の diff 追加行にのみ現れる `## Amendment` 見出し行
/// (先頭の `+` を除いた原文)。bash の
/// `grep -E '^\+## Amendment' | sed -E 's/^\+//'`。
fn list_new_amendment_headings(repo: &Path, base: &str, file: &str) -> Vec<String> {
    git_out(repo, &["diff", base, "HEAD", "--", file])
        .split('\n')
        .filter(|l| l.starts_with("+## Amendment"))
        .map(|l| l[1..].to_string())
        .collect()
}

// ---- 節の切り出し -----------------------------------------------------------

/// `$(…)` と同じく末尾の改行だけを落とす。
fn strip_trailing_newlines(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// `^## 執行点[[:space:]]*$`(新規 ADR の節見出し)。
fn is_adr_exec_heading(line: &str) -> bool {
    line.strip_prefix("## 執行点")
        .is_some_and(|rest| rest.chars().all(is_space))
}

/// `^### 執行点[[:space:]]*$`(Amendment 内の節見出し)。
fn is_amendment_exec_heading(line: &str) -> bool {
    line.strip_prefix("### 執行点")
        .is_some_and(|rest| rest.chars().all(is_space))
}

/// `^#{1,6}[[:space:]]`(節の終わりになる見出し行。`#` が 1〜6 個の直後に
/// 空白 1 文字)。
fn is_any_heading(line: &str) -> bool {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    (1..=6).contains(&hashes) && line[hashes..].chars().next().is_some_and(is_space)
}

/// bash の `extract_section`(awk): 開始見出しの次の行から、終了見出し
/// (exclusive)までの本文。開始行そのものは含めない。
///
/// awk の規則の順序をそのまま写す: 開始見出しに一致する行は(節の途中でも)
/// 読み飛ばし、そうでなく節の中で終了見出しに当たれば打ち切る。
fn extract_section(text: &str, is_open: fn(&str) -> bool) -> String {
    let mut out = String::new();
    let mut flag = false;
    for line in text.split_terminator('\n') {
        if is_open(line) {
            flag = true;
            continue;
        }
        if flag && is_any_heading(line) {
            break;
        }
        if flag {
            out.push_str(line);
            out.push('\n');
        }
    }
    strip_trailing_newlines(&out).to_string()
}

/// bash の `extract_block_by_heading`(awk): 見出し行(原文の完全一致)の次の
/// 行から、次の `^## `(末尾に空白が要る)見出しの手前までの本文。
fn extract_block_by_heading(text: &str, want: &str) -> String {
    let mut out = String::new();
    let mut flag = false;
    for line in text.split_terminator('\n') {
        if line == want {
            flag = true;
            continue;
        }
        if flag && line.starts_with("## ") {
            break;
        }
        if flag {
            out.push_str(line);
            out.push('\n');
        }
    }
    strip_trailing_newlines(&out).to_string()
}

// ---- 箇条書きからのパス抽出 ---------------------------------------------------

/// `^[[:space:]]*[-*][[:space:]]+(.*)$` の `(.*)`(箇条書き行の本文)。
/// 箇条書きでなければ `None`。
fn bullet_text(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(is_space);
    let rest = rest.strip_prefix(['-', '*'])?;
    let after = rest.trim_start_matches(is_space);
    // `[[:space:]]+` は 1 文字以上の空白を要求する。
    (after.len() < rest.len()).then_some(after)
}

/// バッククォートを除き、最初のスペース(タブは区切りでない)までを取る
/// — bash の `token="${text//\`/}"; token="${token%% *}"`。
fn first_token(text: &str) -> String {
    let t: String = text.chars().filter(|&c| c != '`').collect();
    match t.find(' ') {
        Some(i) => t[..i].to_string(),
        None => t,
    }
}

/// bash の `extract_paths`: 箇条書き行(`-` / `*`)からパスのトークンを
/// 1 行 1 個で抽出する。バッククォートは除去し、パスの後の説明文(空白区切り)
/// は捨てる。トークンに残ったタブは取り除く。
pub fn extract_paths(body: &str) -> Vec<String> {
    body.split('\n')
        .filter_map(bullet_text)
        .map(|text| first_token(text).replace('\t', ""))
        .filter(|t| !t.is_empty())
        .collect()
}

/// `` ^`[^`]+`[[:space:]]+`[^`]+` ``(バッククォートで囲んだパスが、説明語を
/// 挟まず 2 つ続く)。
fn two_backticked_paths(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('`') else {
        return false;
    };
    let Some(close) = rest.find('`') else {
        return false;
    };
    if close == 0 {
        return false; // `[^`]+` は 1 文字以上
    }
    let after = rest[close + 1..].trim_start_matches(is_space);
    if after.len() == rest.len() - close - 1 {
        return false; // `[[:space:]]+`
    }
    let Some(rest2) = after.strip_prefix('`') else {
        return false;
    };
    matches!(rest2.find('`'), Some(c) if c > 0)
}

/// bash の `multi_path_lines`: 1 行に複数のパスを書いた箇条書き行を、原文の
/// まま列挙する(#668)。
///
/// `extract_paths` は行の最初のトークンしか読まないので、`` `a`, `b` `` の
/// ような行は、カンマ付きの `a,` が「実在しないパス」になってしまう。原因を
/// 「実在しない」ではなく「1 行 1 パス違反」と名指しするための検出:
/// 1. 最初のトークンが `,` `、` `,` で終わる
/// 2. バッククォートで囲んだパスが、説明語を挟まず 2 つ続く
pub fn multi_path_lines(body: &str) -> Vec<String> {
    body.split('\n')
        .filter(|line| {
            let Some(text) = bullet_text(line) else {
                return false;
            };
            first_token(text).ends_with([',', '、', '，']) || two_backticked_paths(text)
        })
        .map(str::to_string)
        .collect()
}

// ---- 判定 -----------------------------------------------------------------

/// 節本文 `body` の全パス実在 + 「非 .md 非 docs かつ diff 内」のパスが 1 つ以上。
/// 違反メッセージは `out` に積む。適合なら `true`。
fn judge_execution_points(
    label: &str,
    repo_root: &Path,
    changed_files: &str,
    body: &str,
    out: &mut Vec<String>,
) -> bool {
    let body = strip_trailing_newlines(body);
    if body.is_empty() {
        out.push(format!("{PREFIX}{label} が空です"));
        return false;
    }
    let multi = multi_path_lines(body);
    if !multi.is_empty() {
        out.push(format!(
            "{PREFIX}{label} の箇条書きは 1 行 1 パスで書いてください。1 行に複数のパスがある行があります(パスが実在しないのではありません):"
        ));
        for line in multi {
            out.push(format!("  {line}"));
        }
        return false;
    }
    let paths = extract_paths(body);
    if paths.is_empty() {
        out.push(format!("{PREFIX}{label} にパスが列挙されていません"));
        return false;
    }
    let mut ok_exec = false;
    let mut fails = false;
    for p in &paths {
        if !repo_root.join(p).exists() {
            out.push(format!("{PREFIX}{label} の執行点 '{p}' が実在しません"));
            fails = true;
            continue;
        }
        if is_execution_point_path(p) && changed_files.split('\n').any(|l| l == p) {
            ok_exec = true;
        }
    }
    if fails {
        return false;
    }
    if !ok_exec {
        out.push(format!(
            "{PREFIX}{label} の執行点のうち、非 .md かつ非 docs/ 配下で、かつこの PR の diff に含まれるものが1つもありません(既存機構の無変更併記だけでは合格しません)"
        ));
        return false;
    }
    true
}

fn read_text(path: &Path) -> String {
    fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn check_new_adr(repo_root: &Path, file: &str, changed_files: &str, out: &mut Vec<String>) -> bool {
    let section = extract_section(&read_text(&repo_root.join(file)), is_adr_exec_heading);
    if section.is_empty() {
        out.push(format!(
            "{PREFIX}{file} は新規 ADR ですが '## 執行点' 節がありません"
        ));
        return false;
    }
    judge_execution_points(
        &format!("{file} の '## 執行点'"),
        repo_root,
        changed_files,
        &section,
        out,
    )
}

fn check_one_amendment(
    repo_root: &Path,
    file: &str,
    heading: &str,
    changed_files: &str,
    out: &mut Vec<String>,
) -> bool {
    let block = extract_block_by_heading(&read_text(&repo_root.join(file)), heading);
    let section = extract_section(&block, is_amendment_exec_heading);
    if section.is_empty() {
        out.push(format!(
            "{PREFIX}{file} の '{heading}' に '### 執行点' がありません"
        ));
        return false;
    }
    judge_execution_points(
        &format!("{file} の '{heading}' の '### 執行点'"),
        repo_root,
        changed_files,
        &section,
        out,
    )
}

fn check_amended_adr(
    repo_root: &Path,
    base: &str,
    file: &str,
    changed_files: &str,
    out: &mut Vec<String>,
) -> bool {
    let mut ok = true;
    // 新規 Amendment 見出しなし(typo 修正等)は非該当。
    for heading in list_new_amendment_headings(repo_root, base, file) {
        if heading.is_empty() {
            continue;
        }
        // 全見出しを評価する(最初の違反で打ち切らない)。
        ok &= check_one_amendment(repo_root, file, &heading, changed_files, out);
    }
    ok
}

fn diff_has_execution_point(changed_files: &str) -> bool {
    changed_files
        .split('\n')
        .any(|p| !p.is_empty() && is_execution_point_path(p))
}

/// 判定本体。違反メッセージを出力順に返す(空 = 適合)。
///
/// bash 版は違反を stderr に出して rc=1、適合なら stdout に
/// `decision-colocation-check: OK` を出して rc=0。
pub fn run_check(repo_root: &Path, base: &str) -> Vec<String> {
    let added_files = list_added_files(repo_root, base);
    let changed_files = list_changed_files(repo_root, base);
    let mut out = Vec::new();

    // トリガ1a: 新規 ADR
    for f in added_files.split('\n').filter(|f| !f.is_empty()) {
        if is_new_adr_path(f) {
            check_new_adr(repo_root, f, &changed_files, &mut out);
        }
    }

    // トリガ1b: ADR 以外の新規決定成果物(docs/claude/**, config/claude/skills/**)
    let non_adr_new = added_files
        .split('\n')
        .any(|f| !f.is_empty() && is_non_adr_artifact_path(f));
    if non_adr_new && !diff_has_execution_point(&changed_files) {
        out.push(format!(
            "{PREFIX}docs/claude/ または config/claude/skills/ への新規追加がありますが、この PR の diff に非 .md かつ非 docs/ 配下のパスが含まれていません"
        ));
    }

    // トリガ2: 既存 ADR への `## Amendment` 追加
    for f in list_modified_adrs(repo_root, base)
        .split('\n')
        .filter(|f| !f.is_empty())
    {
        check_amended_adr(repo_root, base, f, &changed_files, &mut out);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_predicates_follow_bash_case_globs() {
        assert!(is_execution_point_path("config/claude/hooks/x.sh"));
        assert!(is_execution_point_path("config/shell/profile"));
        assert!(!is_execution_point_path("docs/claude/foo.md"));
        assert!(!is_execution_point_path("README.md"));
        // docs/ 配下は拡張子に関係なく執行点でない
        assert!(!is_execution_point_path("docs/tool/run.sh"));
        assert!(is_new_adr_path("docs/adr/0100-x.md"));
        // `case` の `*` は `/` にも一致する
        assert!(is_new_adr_path("docs/adr/sub/x.md"));
        assert!(!is_new_adr_path("docs/adr/x.txt"));
        assert!(is_non_adr_artifact_path("config/claude/skills/a/SKILL.md"));
        assert!(!is_non_adr_artifact_path("docs/adr/x.md"));
    }

    #[test]
    fn extract_paths_reads_first_token_only() {
        let body = "- config/claude/hooks/foo-gate.sh\n\
                    - `home/modules/claude.nix` — hook 登録\n\
                    * scripts/x\n\
                    not a bullet\n\
                    -\n\
                    -x";
        assert_eq!(
            extract_paths(body),
            [
                "config/claude/hooks/foo-gate.sh",
                "home/modules/claude.nix",
                "scripts/x"
            ]
        );
        // タブは区切りでなく、トークンから取り除かれる。
        assert_eq!(extract_paths("- a\tb"), ["ab"]);
        // 空トークン(バッククォートだけ)は捨てる。
        assert!(extract_paths("- ``").is_empty());
    }

    #[test]
    fn multi_path_lines_names_both_shapes() {
        // (1) 最初のトークンが区切り文字で終わる
        assert_eq!(multi_path_lines("- a/b.sh, c/d.sh"), ["- a/b.sh, c/d.sh"]);
        assert_eq!(multi_path_lines("- a/b.sh、 c/d.sh"), ["- a/b.sh、 c/d.sh"]);
        assert_eq!(multi_path_lines("- a/b.sh， c/d.sh"), ["- a/b.sh， c/d.sh"]);
        // 区切りの直後に空白が無い行は、最初のトークンが 1 語になり検出されない
        // (bash 版も同じ。パス不在として落ちる)。
        assert!(multi_path_lines("- a/b.sh、c/d.sh").is_empty());
        // (2) バッククォートで囲んだパスが 2 つ続く
        assert_eq!(
            multi_path_lines("- `a/b.sh` `c/d.sh`"),
            ["- `a/b.sh` `c/d.sh`"]
        );
        assert_eq!(
            multi_path_lines("- `a/b.sh`, `c/d.sh`"),
            ["- `a/b.sh`, `c/d.sh`"]
        );
        // 説明語を挟めば 1 パス
        assert!(multi_path_lines("- `a/b.sh` — `c` を呼ぶ").is_empty());
        assert!(multi_path_lines("- `a/b.sh` hook 登録 `c`").is_empty());
        assert!(multi_path_lines("- a/b.sh").is_empty());
        assert!(multi_path_lines("- ``  `x`").is_empty());
    }

    #[test]
    fn section_extraction_matches_awk() {
        let text = "# t\n\n## 執行点\n- a\n- b\n\n## 他\n- c\n";
        assert_eq!(extract_section(text, is_adr_exec_heading), "- a\n- b");
        // 開始見出しが無ければ空
        assert_eq!(extract_section("# t\n## x\n", is_adr_exec_heading), "");
        // `### 執行点` は `## 執行点` に一致しない
        assert_eq!(
            extract_section("### 執行点\n- a\n", is_adr_exec_heading),
            ""
        );
        // `#` が 7 個以上の行は終了見出しにならない
        assert_eq!(
            extract_section("## 執行点\n- a\n####### x\n- b\n", is_adr_exec_heading),
            "- a\n####### x\n- b"
        );
        // 見出し末尾の空白は許す
        assert_eq!(
            extract_section("## 執行点  \n- a\n", is_adr_exec_heading),
            "- a"
        );
    }

    #[test]
    fn block_extraction_stops_at_next_h2() {
        let text = "## Amendment (a)\n### 執行点\n- x\n## Amendment (b)\n### 執行点\n- y\n";
        assert_eq!(
            extract_block_by_heading(text, "## Amendment (a)"),
            "### 執行点\n- x"
        );
        assert_eq!(
            extract_block_by_heading(text, "## Amendment (b)"),
            "### 執行点\n- y"
        );
        assert_eq!(extract_block_by_heading(text, "## Amendment (c)"), "");
    }
}
