//! コマンド文字列全体から「対象コマンドごとの判定範囲」を切り出す
//! (attribution-guard.sh の `decide()` の前半。stack-base-guard.sh の
//! `decide_stack` / pr-title-guard.sh の `decide_pr_title` /
//! decision-colocation-guard.sh の `decide_colocation` / adr-number.sh の
//! `command_ran_pr_create` が同じ形を複製していた)。
//!
//! 範囲の切り方(bash 版の設計上の勘所):
//! - 対象コマンドは「コマンド位置」(先頭、または区切りトークンの直後)に
//!   あるものだけを拾う。クォートされた文字列は 1 トークンになるので
//!   コマンド位置には来ない。
//! - 1 範囲 = 対象コマンドの出現位置から次の対象コマンドの直前まで。区切りを
//!   シェル metachar にしないのは、`--body "$(cat <<'EOF' … EOF)"` の本文が
//!   metachar を含みうるため(metachar で切ると本文が後段に落ちる)。
//!   `gh pr create --body "…フッター…" && gh pr comment 1 --body "短い"` で
//!   comment 側が create 側のフッターで通ってしまうのを防ぐため、範囲ごとに
//!   独立に判定する。

use crate::shell::{is_sep, split_heredoc, tokenize};

/// heredoc 分離 + トークン化済みのコマンド。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    /// heredoc 本体を除いた文字列のトークン列(bash の `TOK`)。
    pub tokens: Vec<String>,
    /// heredoc 本体の連結(bash の `HD_BODIES`)。
    pub heredoc_bodies: String,
}

/// 1 つの対象コマンドの判定範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range<'a, K> {
    /// `is_target_at` が返した種別(bash の `TARGET_KIND`)。
    pub kind: K,
    /// [`ParsedCommand::tokens`] 内の開始位置。0 より大きければ対象コマンドの
    /// 前に別の文がある(decision-colocation-guard.sh の #668 の注意書きに使う)。
    pub start: usize,
    /// 範囲のトークン列(対象コマンドの先頭から次の対象コマンドの直前まで)。
    pub tokens: &'a [String],
    /// コマンド全体の heredoc 本体(範囲には分けない — 1 コマンド中に複数の
    /// heredoc がある場合の対応付けはしない、bash 版の既知の限界)。
    pub heredoc_bodies: &'a str,
}

/// `split_heredoc` → `tokenize`。トークン化できない(unmatched quote)か
/// トークンが 0 件なら `None`(呼び出し側は判定不能として通す)。
///
/// 注意: bash 版 attribution-guard.sh 冒頭のコメントは「トークナイザが
/// unmatched quote → heredoc と同じフォールバック(範囲文字列全体を検査)」
/// と書くが、実装は `tokenize` が何も出力せず `TOK` が空になり素通しする。
/// ここでは実装どおり `None` を返す。
pub fn parse(cmd: &str) -> Option<ParsedCommand> {
    let split = split_heredoc(cmd);
    let tokens = tokenize(&split.command)?;
    Some(ParsedCommand {
        tokens,
        heredoc_bodies: split.bodies,
    })
}

impl ParsedCommand {
    /// コマンド位置にある対象コマンドごとの判定範囲。
    ///
    /// `is_target_at(tokens, i)` は bash の `is_target_at`(各 guard が上書き
    /// していた関数)に当たる: `tokens[i]` が対象コマンドの先頭なら種別を返す。
    /// コマンド位置にあるトークンにだけ呼ばれる。
    pub fn ranges<K, F>(&self, mut is_target_at: F) -> Vec<Range<'_, K>>
    where
        F: FnMut(&[String], usize) -> Option<K>,
    {
        let n = self.tokens.len();
        let mut starts: Vec<(usize, K)> = Vec::new();
        let mut at_cmd_pos = true;
        for i in 0..n {
            if at_cmd_pos {
                if let Some(k) = is_target_at(&self.tokens, i) {
                    starts.push((i, k));
                }
            }
            at_cmd_pos = is_sep(&self.tokens[i]);
        }
        let ends: Vec<usize> = starts
            .iter()
            .skip(1)
            .map(|(s, _)| *s)
            .chain(std::iter::once(n))
            .collect();
        starts
            .into_iter()
            .zip(ends)
            .map(|((s, kind), e)| Range {
                kind,
                start: s,
                tokens: &self.tokens[s..e],
                heredoc_bodies: &self.heredoc_bodies,
            })
            .collect()
    }
}

/// bash の `decide()` 全体: 範囲ごとに `judge` を呼び、最初の deny 理由を
/// 返す(1 件でも deny なら deny)。パースできない・対象が無い・全範囲が
/// 通れば `None`。
pub fn first_deny<K, F, J>(cmd: &str, is_target_at: F, judge: J) -> Option<String>
where
    F: FnMut(&[String], usize) -> Option<K>,
    J: FnMut(&Range<'_, K>) -> Option<String>,
{
    let parsed = parse(cmd)?;
    let ranges = parsed.ranges(is_target_at);
    ranges.iter().find_map(judge)
}

/// トークンが `gh`(素の `gh` でもフルパスでもよい)なら真。bash の
/// `base="${TOK[i]##*/}"; [[ $base == gh ]]`。
pub fn is_gh(tok: &str) -> bool {
    tok.rsplit('/').next() == Some("gh")
}

/// `tokens[i..]` が `gh <words…>` で始まれば真(各 guard の `is_target_at` の
/// 定型部分。例: `gh_command_at(t, i, &["pr", "create"])`)。bash の
/// `((i + k < n)) || return 1` と同じく、語が足りなければ偽。
pub fn gh_command_at(tokens: &[String], i: usize, words: &[&str]) -> bool {
    if i + words.len() >= tokens.len() {
        return false;
    }
    is_gh(&tokens[i])
        && words
            .iter()
            .enumerate()
            .all(|(k, w)| tokens[i + 1 + k] == *w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr_create(t: &[String], i: usize) -> Option<()> {
        gh_command_at(t, i, &["pr", "create"]).then_some(())
    }

    #[test]
    fn ranges_only_at_command_position() {
        let p = parse("echo gh pr create; /usr/bin/gh pr create -t x && gh pr create").unwrap();
        let r = p.ranges(pr_create);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start, 5);
        assert_eq!(r[0].tokens.first().map(String::as_str), Some("/usr/bin/gh"));
        // 範囲は次の対象の直前まで(区切りも含む)
        assert_eq!(r[0].tokens.last().map(String::as_str), Some("&"));
        // split_heredoc が各行に足す `\n` も区切りトークンとして残る(bash と同じ)
        assert_eq!(r[1].tokens, ["gh", "pr", "create", "\n"]);
    }

    #[test]
    fn quoted_spelling_is_not_command_position() {
        let p = parse("echo 'gh pr create'").unwrap();
        assert!(p.ranges(pr_create).is_empty());
    }

    #[test]
    fn heredoc_body_is_not_command_position() {
        let p = parse("git commit -F - <<'M'\ngh pr create\nM").unwrap();
        assert!(p.ranges(pr_create).is_empty());
        assert_eq!(p.heredoc_bodies, "gh pr create\n");
    }

    #[test]
    fn unparsable_is_none() {
        assert!(parse("gh pr create --body 'x").is_none());
        assert!(first_deny("gh pr create --body 'x", pr_create, |_| Some("d".into())).is_none());
    }

    #[test]
    fn gh_command_needs_enough_words() {
        let t: Vec<String> = ["gh", "pr"].iter().map(|s| s.to_string()).collect();
        assert!(!gh_command_at(&t, 0, &["pr", "create"]));
        // `gh api` 型(語 1 つ)は i + 1 < n で足りる
        assert!(gh_command_at(&t, 0, &["pr"]));
        assert!(!gh_command_at(&t, 1, &["pr"]));
    }
}
