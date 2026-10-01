//! attribution-guard — AI エージェントが GitHub に書く外向きテキストに
//! attribution フッターが載っていることを保証する PreToolUse hook
//! (docs/claude/attribution-guard.md、#192、ADR-0024 Stage 4a #415)。
//!
//! bash 版 `config/claude/hooks/attribution-guard.sh` の移植。コマンド解析
//! (heredoc 分離・トークナイザ・コマンド位置・本文フラグ)は `guard-core` に
//! 切り出し、ここには attribution 固有の判定(フッター文言・対象コマンド・
//! `gh api` の対象エンドポイント・MCP の書き込み系 tool 名)だけを置く。
//!
//! 判定は 1 つだけ: 投稿本文に attribution フッター または
//! `No-Attribution: <理由>` が無ければ deny。
//!
//! - なぜ Stop hook でなく PreToolUse か: コメント投稿は通知が飛ぶ不可逆
//!   操作で、事後に止めても取り返せない。
//! - なぜ mention の有無で分岐しないか: mention が無くても watcher /
//!   assignee / subscriber には通知が飛ぶ。`@` をパースする分岐は偽陰性を
//!   生む(コードブロック内の @、メールアドレス)。AI 生成物である事実は
//!   読者が誰かに依存しない。
//! - 抜け道は本文マーカー `No-Attribution: <理由>`(pr-gate.sh の
//!   `No-Issue:` / `No-Visual:` と同型)。deny の理由文にこの抜け道を明示的に
//!   書く — bleep は漏洩防止のため bypass を理由文に書かない逆方針だが、
//!   No-Attribution: は正当な判断なので使えないと意味がない。
//!
//! 判定不能の倒し方(bash 版と同じ。判定できないことを deny に変えない):
//! - 本文フラグが無い(`gh pr edit --add-label`)→ 通す
//! - 本文がコマンド置換のみ(`--body "$(cat body.md)"`)→ 中身が不明なので
//!   通す。heredoc を使う範囲は本体が command 文字列内に実在するので本体を
//!   検査する
//! - トークン化できない(unmatched quote)→ 通す
//!
//! 既知の限界(意図的な選択、docs/claude/attribution-guard.md):
//! - `gh api` は外向き投稿の URL パス(issues/pulls の作成・編集・コメント、
//!   pulls のレビュー作成)のみ判定する(#195)。`pulls/N/comments` のような
//!   インラインレビューコメントは対象外のまま(#194)。GET/HEAD/DELETE は
//!   明示 `-X`/`--method` があるときだけ除外する。
//! - MCP tool 名の命名規則は Codex/Copilot とも未確認(#161)なので、MCP の
//!   判定は Claude だけで行う(bash 版の Codex/Copilot adapter が
//!   `decide_mcp` を呼ばなかったのと同じ)。

use guard_core::command::{first_deny, gh_command_at, is_gh};
use guard_core::gh::{api_method, api_path, assemble_body_text, has_heredoc, ApiBody, BodyFlags};
use guard_core::hook::ToolCall;
use guard_core::marker::{has_lead_then_name, has_reasoned_tag};
use guard_core::Range;
use hook_io::Agent;

/// フッターに載せるエージェント名とリンク先(#192)。bash 版では adapter が
/// `ATTRIBUTION_AGENT_NAME` / `ATTRIBUTION_AGENT_URL` を source 前に上書き
/// していた。ここでは `--agent` で選ぶ — 文言はエージェントごとに
/// この 1 箇所だけが持つ(D4)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attribution {
    pub name: &'static str,
    pub url: &'static str,
}

/// `Generated with` は harness 由来のフッター、`Filed from` は wrap-up inbox
/// の出自フッター(統合形「🤖 Filed from [Claude Code](…) wrap-up inbox」)が
/// 生成元表示を兼ねるケース(D1)。旧 2 行形式も前段でそのまま通る。
const LEADS: &[&str] = &["Generated with", "Filed from"];

const NO_ATTRIBUTION: &str = "No-Attribution:";

/// MCP GitHub の書き込み系 tool(現在未接続、命名は #161 と同じく未確認
/// なので保守的に名前で絞る)。read 系を deny すると壊れるので、書き込みを
/// 示す語を含むものだけを対象にする。
const MCP_WRITE_WORDS: &[&str] = &[
    "comment",
    "create_issue",
    "create_pull",
    "update_issue",
    "update_pull",
    "create_review",
    "submit_review",
    "add_issue_comment",
    "add_comment",
];

/// 対象コマンドの種別(bash の `TARGET_KIND`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// `gh (pr|issue) (create|edit|comment)` / `gh pr review`
    Cli,
    /// `gh api …`(対象パスかどうかは [`Attribution::decide_api_tokens`] が見る)
    Api,
}

/// bash の `is_target_at`: `tokens[i]` が対象コマンドの先頭なら種別。
///
/// `gh api` はサブコマンドの有無だけで判定範囲を開始する(`-X` がパスより
/// 先に来る形もあるため、ここではパス位置を固定しない)。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<TargetKind> {
    if i + 1 >= tokens.len() || !is_gh(&tokens[i]) {
        return None;
    }
    if tokens[i + 1] == "api" {
        return Some(TargetKind::Api);
    }
    let ok = match tokens[i + 1].as_str() {
        "pr" => ["create", "edit", "comment", "review"]
            .iter()
            .any(|s| gh_command_at(tokens, i, &["pr", s])),
        "issue" => ["create", "edit", "comment"]
            .iter()
            .any(|s| gh_command_at(tokens, i, &["issue", s])),
        _ => false,
    };
    ok.then_some(TargetKind::Cli)
}

/// bash の `API_JUDGED_RE`:
/// `^/repos/[^/]+/[^/]+/(issues(/[0-9]+)?(/comments)?|issues/comments/[0-9]+|pulls(/[0-9]+)?(/reviews)?)$`。
///
/// pulls への `/comments` 系(インラインレビューコメント)は alternation に
/// 無い(pulls は `(/[0-9]+)?(/reviews)?` までしか許さない)ので自然に
/// 非対象になる — #194 の「インラインは対象外」を追加の除外規則無しで保つ。
pub fn is_judged_api_path(t: &str) -> bool {
    let Some(rest) = t.strip_prefix("/repos/") else {
        return false;
    };
    let Some((owner, rest)) = rest.split_once('/') else {
        return false;
    };
    let Some((repo, rest)) = rest.split_once('/') else {
        return false;
    };
    if owner.is_empty() || repo.is_empty() {
        return false;
    }
    let num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let seg: Vec<&str> = rest.split('/').collect();
    match seg.as_slice() {
        ["issues"] | ["issues", "comments"] | ["pulls"] | ["pulls", "reviews"] => true,
        ["issues", n] | ["pulls", n] => num(n),
        ["issues", n, "comments"] | ["pulls", n, "reviews"] => num(n),
        ["issues", "comments", n] => num(n),
        _ => false,
    }
}

/// MCP の tool 名が書き込み系か(bash の `MCP_WRITE_RE` の部分一致)。
pub fn is_mcp_write(tool: &str) -> bool {
    MCP_WRITE_WORDS.iter().any(|w| tool.contains(w))
}

impl Attribution {
    pub const CLAUDE: Attribution = Attribution {
        name: "Claude Code",
        url: "https://claude.com/claude-code",
    };
    pub const CODEX: Attribution = Attribution {
        name: "Codex CLI",
        url: "https://learn.chatgpt.com/docs/codex/cli",
    };
    pub const COPILOT: Attribution = Attribution {
        name: "GitHub Copilot CLI",
        url: "https://docs.github.com/en/copilot/how-tos/copilot-cli",
    };

    pub fn for_agent(agent: Agent) -> Self {
        match agent {
            Agent::Claude => Self::CLAUDE,
            Agent::Codex => Self::CODEX,
            Agent::Copilot => Self::COPILOT,
        }
    }

    /// 要求するフッター(bash の `ATTRIBUTION_FOOTER`)。
    pub fn footer(&self) -> String {
        format!("🤖 Generated with [{}]({})", self.name, self.url)
    }

    /// bash の `has_marker`: attribution フッターか `No-Attribution: <理由>`。
    pub fn has_marker(&self, text: &str) -> bool {
        has_lead_then_name(text, LEADS, self.name) || has_reasoned_tag(text, NO_ATTRIBUTION)
    }

    /// bash の `deny_reason`。抜け道を明示的に書く(モジュール doc 参照)。
    pub fn deny_reason(&self) -> String {
        format!(
            "GitHub に投稿する本文に attribution がありません(deny)。本文の末尾に「{}」を追記してください。本文が Claude 生成でない場合(ユーザーの逐語をそのまま代理投稿する等)は、本文に「No-Attribution: <理由>」と書いて明示的に抜けてください。",
            self.footer()
        )
    }

    fn judge_text(&self, text: Option<String>) -> Option<String> {
        let text = text?;
        (!self.has_marker(&text)).then(|| self.deny_reason())
    }

    /// bash の `decide_tokens`: `gh pr|issue …` 1 投稿ぶんの範囲。deny なら
    /// 理由文。
    pub fn decide_tokens(&self, tokens: &[String], heredoc_bodies: &str) -> Option<String> {
        let flags = BodyFlags::scan(tokens);
        if !flags.have_flag {
            return None; // 本文フラグ無し → 判定不能で通す
        }
        self.judge_text(assemble_body_text(
            &flags.texts,
            has_heredoc(tokens),
            heredoc_bodies,
        ))
    }

    /// bash の `decide_api_tokens`(#195): `gh api …` 1 投稿ぶんの範囲。
    ///
    /// 1. `-X`/`--method` が GET/HEAD/DELETE なら通す(既定メソッドの推測は
    ///    しない — 明示が無ければ本文フラグの有無で自然に判定不能に落ちる)
    /// 2. 対象エンドポイントのパスが無ければ通す — `gh api -X POST
    ///    repos/…/rulesets --input -`(scripts/github-rulesets-apply)を
    ///    誤判定しない回帰要件
    /// 3. 本文は `body=<値>` のフィールドか `--input <file>` の `.body`
    pub fn decide_api_tokens(&self, tokens: &[String], heredoc_bodies: &str) -> Option<String> {
        if let Some(m) = api_method(tokens) {
            if matches!(m.to_ascii_uppercase().as_str(), "GET" | "HEAD" | "DELETE") {
                return None;
            }
        }
        api_path(tokens, is_judged_api_path)?;
        let body = ApiBody::scan(tokens);
        if !body.have_flag {
            return None;
        }
        self.judge_text(assemble_body_text(
            &body.texts,
            has_heredoc(tokens),
            heredoc_bodies,
        ))
    }

    fn decide_range(&self, r: &Range<'_, TargetKind>) -> Option<String> {
        match r.kind {
            TargetKind::Api => self.decide_api_tokens(r.tokens, r.heredoc_bodies),
            TargetKind::Cli => self.decide_tokens(r.tokens, r.heredoc_bodies),
        }
    }

    /// bash の `decide`: Bash ツールのコマンド文字列全体。対象コマンドごとの
    /// 範囲を独立に判定し、1 件でも deny なら deny の理由文。
    pub fn decide(&self, cmd: &str) -> Option<String> {
        first_deny(cmd, is_target_at, |r| self.decide_range(r))
    }

    /// bash の `decide_mcp`: MCP GitHub の書き込み系 tool の
    /// `.tool_input.body // .tool_input.comment`。
    ///
    /// matcher を Bash 単体にすると MCP を接続した瞬間に無検査になるので、
    /// 登録は `Bash|mcp__.*` にして名前で書き込み系に絞ってから本文を見る。
    pub fn decide_mcp(&self, call: &ToolCall) -> Option<String> {
        if !is_mcp_write(&call.tool) {
            return None;
        }
        let body = call.tool_input_alt(&["body", "comment"])?;
        if body.is_empty() {
            return None;
        }
        self.judge_text(Some(body))
    }
}

/// hook 本体(bash の `main` / adapter の `main_codex` / `main_copilot`)。
/// deny なら理由文。
///
/// - Claude: `Bash` のコマンドと `mcp__github*` の MCP tool
/// - Codex / Copilot: シェル実行 tool のコマンドだけ(MCP は #161 で未確認)
pub fn check(call: &ToolCall) -> Option<String> {
    let attr = Attribution::for_agent(call.agent);
    if call.is_bash() {
        return attr.decide(&call.bash_command()?);
    }
    if call.agent == Agent::Claude && call.tool.starts_with("mcp__github") {
        return attr.decide_mcp(call);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judged_paths() {
        for p in [
            "/repos/o/r/issues",
            "/repos/o/r/issues/1",
            "/repos/o/r/issues/comments",
            "/repos/o/r/issues/1/comments",
            "/repos/o/r/issues/comments/9",
            "/repos/o/r/pulls",
            "/repos/o/r/pulls/1",
            "/repos/o/r/pulls/reviews",
            "/repos/o/r/pulls/1/reviews",
        ] {
            assert!(is_judged_api_path(p), "{p}");
        }
        for p in [
            "/repos/o/r/pulls/1/comments",
            "/repos/o/r/pulls/comments/9",
            "/repos/o/r/rulesets",
            "/repos/o/r/issues/x",
            "/repos/o//issues",
            "/repos/o/r/issues/1/comments/2",
            "/user/repos",
        ] {
            assert!(!is_judged_api_path(p), "{p}");
        }
    }

    #[test]
    fn target_kinds() {
        let t = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(is_target_at(&t("gh api x"), 0), Some(TargetKind::Api));
        assert_eq!(is_target_at(&t("gh api"), 0), Some(TargetKind::Api));
        assert_eq!(
            is_target_at(&t("/usr/bin/gh pr review 1"), 0),
            Some(TargetKind::Cli)
        );
        assert_eq!(is_target_at(&t("gh issue review 1"), 0), None);
        assert_eq!(is_target_at(&t("gh pr"), 0), None);
        assert_eq!(is_target_at(&t("gh"), 0), None);
    }

    #[test]
    fn footer_per_agent() {
        assert_eq!(
            Attribution::for_agent(Agent::Codex).footer(),
            "🤖 Generated with [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)"
        );
        // 他エージェントの名前のフッターでは通らない
        let c = Attribution::COPILOT;
        assert!(c
            .decide("gh pr comment 1 --body 'x 🤖 Generated with [Claude Code](u)'")
            .is_some());
    }
}
