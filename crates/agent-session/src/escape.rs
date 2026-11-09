//! hook の逃げ道(guard・gate を一時的に止める手段)の、名前・真偽・スコープ・案内。
//!
//! 逃げ道は、スコープが 2 種類だけある。名前は、guard の実行名から導く(`STEM` = 実行名を大文字と `_` にしたもの)。
//!
//! | スコープ | 名前 | 読む場所 | 誰が使うか |
//! |---|---|---|---|
//! | 呼び出し単位 | `<STEM>_ALLOW=1` | Bash の 1 回の呼び出しのコマンドの文字列にある、引用外の語 | エージェントが、コマンドに足す(痕跡がコマンドに残る) |
//! | セッション単位 | `SKIP_<STEM>=1` | hook プロセスの環境変数。skip ファイルを持つ guard は、その存在も同じ意味 | 人が、起動前に設定する(エージェントは hook のプロセス環境を呼び出しの中から変えられない) |
//!
//! 真は **`"1"` だけ**。空・`0`・`true`・ほかの値は偽。名前・どの guard がどのスコープを持つか・案内の文は、
//! 下の [`ALL`] の表が唯一の正本で、guard 側は定数を参照するだけで、名前の文字列を手で書かない
//! (`repo-structure-check` の `escape` が、`SKIP_*` と `*_ALLOW` の文字列の直書きを落とす)。
//!
//! 持たない guard: `routines-write-guard` は、逃げ道を足さないと決めた(`home/modules/agents.nix`)。
//! 理由の要る本文タグ(`No-Attribution:` など)は別の機構である。bash の git hook の `GIT_ALLOW_MAIN_COMMIT` は、
//! この表を呼べない層なので範囲外。
//!
//! skip ファイルの置き場所は、各機能が所有する(`docs/agents/tooling-conventions.md`)。ここは、ディレクトリを
//! 受けて `skip` の存在を見るだけである。

use std::path::Path;

/// 逃げ道が効いているスコープ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// コマンドの中の `<STEM>_ALLOW=1`。
    Call,
    /// 環境変数 `SKIP_<STEM>=1` か、skip ファイル。
    Session,
}

/// 逃げ道 1 つの宣言。他の crate は、この構造体を作れない(表の外に逃げ道を作れない)。
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Hatch {
    /// 実行名を大文字と `_` にしたもの。
    pub stem: &'static str,
    /// 呼び出し単位(`<STEM>_ALLOW=1`)を持つか。
    pub call: bool,
    /// skip ファイルを持つか(セッション単位の別の印)。
    pub skip_file: bool,
    /// セッション単位(`SKIP_<STEM>=1`)を持つか。呼び出し単位だけの guard は持たない。
    pub session: bool,
    /// 拒否文・案内に、逃げ道の存在を書くか。外部への送信など、エージェントが自分で外せてはならない
    /// ものは、書かない。
    pub disclose: bool,
    /// 説明の文書(リポジトリの根からの相対パス)。
    pub doc: &'static str,
}

const fn hatch(
    stem: &'static str,
    call: bool,
    skip_file: bool,
    disclose: bool,
    doc: &'static str,
) -> Hatch {
    Hatch {
        stem,
        call,
        skip_file,
        session: true,
        disclose,
        doc,
    }
}

impl Hatch {
    /// セッション単位を持たない宣言にする(呼び出し単位だけの guard)。
    const fn call_only(mut self) -> Hatch {
        self.session = false;
        self
    }
}

// 呼び出し単位とセッション単位の両方(コマンドを見る guard。案内する)。
pub const CI_WAIT_GUARD: Hatch = hatch(
    "CI_WAIT_GUARD",
    true,
    false,
    true,
    "docs/agents/ci-wait-guard.md",
);
pub const PKILL_GUARD: Hatch = hatch(
    "PKILL_GUARD",
    true,
    false,
    true,
    "docs/agents/pkill-guard.md",
);
pub const TEST_OUTPUT_GUARD: Hatch = hatch(
    "TEST_OUTPUT_GUARD",
    true,
    false,
    true,
    "docs/agents/test-output-guard.md",
);
pub const SANDBOX_COMPOUND_GUARD: Hatch = hatch(
    "SANDBOX_COMPOUND_GUARD",
    true,
    false,
    true,
    "docs/agents/sandbox-compound-guard.md",
);
// 呼び出し単位だけ(案内しない)。
pub const RULESETS_WRITE_GUARD: Hatch = hatch(
    "RULESETS_WRITE_GUARD",
    true,
    false,
    false,
    "docs/agents/rulesets-write-guard.md",
)
.call_only();
// セッション単位だけ(案内する)。
pub const PR_TITLE_GUARD: Hatch = hatch(
    "PR_TITLE_GUARD",
    false,
    false,
    true,
    "docs/agents/pr-title-contract.md",
);
pub const PR_FOLLOWUP_GUARD: Hatch = hatch(
    "PR_FOLLOWUP_GUARD",
    false,
    false,
    true,
    "docs/agents/pr-followup-guard.md",
);
pub const REPO_CREATE_GUARD: Hatch = hatch(
    "REPO_CREATE_GUARD",
    false,
    false,
    true,
    "docs/agents/repo-create-guard.md",
);
pub const DECISION_COLOCATION_GUARD: Hatch = hatch(
    "DECISION_COLOCATION_GUARD",
    false,
    false,
    true,
    "docs/agents/decision-colocation.md",
);
// セッション単位だけ(案内しない)。
pub const EXTERNAL_SEND_GUARD: Hatch = hatch(
    "EXTERNAL_SEND_GUARD",
    false,
    false,
    false,
    "docs/agents/external-send-guard.md",
);
pub const JA_POLISH_GATE: Hatch = hatch(
    "JA_POLISH_GATE",
    false,
    false,
    false,
    "docs/agents/ja-polish.md",
);
// セッション単位 + skip ファイル(案内しない)。
pub const PLAN_SCOPE_GATE: Hatch = hatch(
    "PLAN_SCOPE_GATE",
    false,
    true,
    false,
    "docs/agents/requirements-inventory.md",
);
pub const PLAN_PRECEDENT_GATE: Hatch = hatch(
    "PLAN_PRECEDENT_GATE",
    false,
    true,
    false,
    "docs/agents/precedent-grounding.md",
);
pub const PLAN_FRESH_GATE: Hatch = hatch(
    "PLAN_FRESH_GATE",
    false,
    true,
    false,
    "docs/agents/plan-fresh-gate.md",
);
pub const CODEX_PLAN_GATE: Hatch = hatch(
    "CODEX_PLAN_GATE",
    false,
    true,
    false,
    "docs/claude/codex-plan-gate.md",
);
pub const PR_GATE: Hatch = hatch("PR_GATE", false, true, false, "docs/agents/pr-gate.md");
pub const STACK_BASE_GUARD: Hatch = hatch(
    "STACK_BASE_GUARD",
    false,
    true,
    false,
    "docs/agents/stack-base-guard.md",
);
pub const NEW_TOOL_GUARD: Hatch = hatch(
    "NEW_TOOL_GUARD",
    false,
    true,
    false,
    "docs/agents/new-tool-guard.md",
);
pub const GH_EDIT_ALLOW: Hatch = hatch(
    "GH_EDIT_ALLOW",
    false,
    true,
    false,
    "docs/agents/gh-edit-allow.md",
);
pub const COPILOT_PLAN_REVIEW: Hatch = hatch(
    "COPILOT_PLAN_REVIEW",
    false,
    true,
    false,
    "docs/agents/copilot-plan-review.md",
);
// セッション単位 + skip ファイル(案内する: 状態の表示に出る)。
pub const PLAN_VIEW: Hatch = hatch("PLAN_VIEW", false, true, true, "docs/agents/plan-view.md");

/// 逃げ道の全部。`stem` は一意。
pub const ALL: &[&Hatch] = &[
    &CI_WAIT_GUARD,
    &PKILL_GUARD,
    &TEST_OUTPUT_GUARD,
    &SANDBOX_COMPOUND_GUARD,
    &RULESETS_WRITE_GUARD,
    &PR_TITLE_GUARD,
    &PR_FOLLOWUP_GUARD,
    &REPO_CREATE_GUARD,
    &DECISION_COLOCATION_GUARD,
    &EXTERNAL_SEND_GUARD,
    &JA_POLISH_GATE,
    &PLAN_SCOPE_GATE,
    &PLAN_PRECEDENT_GATE,
    &PLAN_FRESH_GATE,
    &CODEX_PLAN_GATE,
    &PR_GATE,
    &STACK_BASE_GUARD,
    &NEW_TOOL_GUARD,
    &GH_EDIT_ALLOW,
    &COPILOT_PLAN_REVIEW,
    &PLAN_VIEW,
];

/// skip ファイルの名前。置き場所(ディレクトリ)は、各機能が所有する。
pub const SKIP_FILE: &str = "skip";

/// 真は `"1"` だけ。
pub fn truthy(v: Option<&str>) -> bool {
    v == Some("1")
}

impl Hatch {
    /// セッション単位の環境変数の名前(`SKIP_<STEM>`)。
    pub fn session_name(&self) -> String {
        format!("SKIP_{}", self.stem)
    }

    /// 呼び出し単位の語の名前(`<STEM>_ALLOW`)。持たないときは `None`。
    pub fn call_name(&self) -> Option<String> {
        self.call.then(|| format!("{}_ALLOW", self.stem))
    }

    /// 拒否文に添える、逃げ道の案内。`disclose` が偽なら `None`(書かない)。
    pub fn how_to(&self) -> Option<String> {
        if !self.disclose {
            return None;
        }
        let session = self.session_name();
        Some(match (self.call_name(), self.session) {
            (Some(call), true) => format!(
                "やむを得ないときだけ、コマンドに `{call}=1` を足す(人が起動前に `{session}=1` を設定してもよい)。"
            ),
            (Some(call), false) => format!("やむを得ないときだけ、コマンドに `{call}=1` を足す。"),
            (None, _) => format!(
                "やむを得ないときだけ、人が `{session}=1` を設定して起動し直す(エージェントは設定できない)。"
            ),
        })
    }

    /// 判定(純粋)。セッション単位(環境変数か skip ファイル)が先で、次に呼び出し単位。
    ///
    /// - `session_env`: `SKIP_<STEM>` の値。
    /// - `skip_exists`: skip ファイルが在るか(`skip_file` を持たない guard では無視する)。
    /// - `tokens`: コマンドの語(引用外)。コマンドを持たない gate は `None`。
    pub fn judge(
        &self,
        session_env: Option<&str>,
        skip_exists: bool,
        tokens: Option<&[String]>,
    ) -> Option<Scope> {
        if self.session && (truthy(session_env) || (self.skip_file && skip_exists)) {
            return Some(Scope::Session);
        }
        let call = self.call_name()?;
        let word = format!("{call}=1");
        tokens
            .is_some_and(|t| t.contains(&word))
            .then_some(Scope::Call)
    }

    /// 環境変数と skip ファイルを読んで [`Hatch::judge`] を呼ぶ唯一の入口。`skip_dir` は、skip ファイルを持つ
    /// guard の状態ディレクトリ。
    pub fn active(&self, skip_dir: Option<&Path>, tokens: Option<&[String]>) -> Option<Scope> {
        let env = std::env::var(self.session_name()).ok();
        let skip_exists = skip_dir.is_some_and(|d| d.join(SKIP_FILE).exists());
        self.judge(env.as_deref(), skip_exists, tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &[&str]) -> Vec<String> {
        s.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn only_the_exact_string_one_is_true() {
        assert!(truthy(Some("1")));
        for v in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some("yes"),
            Some("11"),
            Some(" 1"),
        ] {
            assert!(!truthy(v), "{v:?}");
        }
    }

    #[test]
    fn names_follow_from_the_stem() {
        assert_eq!(CI_WAIT_GUARD.session_name(), "SKIP_CI_WAIT_GUARD");
        assert_eq!(
            CI_WAIT_GUARD.call_name().as_deref(),
            Some("CI_WAIT_GUARD_ALLOW")
        );
        assert_eq!(PR_TITLE_GUARD.call_name(), None);
        assert_eq!(GH_EDIT_ALLOW.session_name(), "SKIP_GH_EDIT_ALLOW");
    }

    #[test]
    fn stems_are_unique_upper_snake_and_documented() {
        let mut seen = std::collections::BTreeSet::new();
        for h in ALL {
            assert!(seen.insert(h.stem), "重複: {}", h.stem);
            assert!(
                h.stem
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "{}",
                h.stem
            );
            assert!(h.doc.ends_with(".md"), "{}", h.doc);
        }
        assert_eq!(ALL.len(), 21);
    }

    #[test]
    fn the_session_scope_wins_over_the_call_scope() {
        let t = words(&["gh", "run", "watch", "CI_WAIT_GUARD_ALLOW=1"]);
        assert_eq!(
            CI_WAIT_GUARD.judge(Some("1"), false, Some(&t)),
            Some(Scope::Session)
        );
        assert_eq!(
            CI_WAIT_GUARD.judge(None, false, Some(&t)),
            Some(Scope::Call)
        );
        assert_eq!(
            CI_WAIT_GUARD.judge(None, false, Some(&words(&["gh"]))),
            None
        );
    }

    #[test]
    fn a_call_word_counts_anywhere_among_the_unquoted_words_but_only_as_exact_one() {
        for (w, expect) in [
            ("CI_WAIT_GUARD_ALLOW=1", Some(Scope::Call)),
            ("CI_WAIT_GUARD_ALLOW=0", None),
            ("CI_WAIT_GUARD_ALLOW=", None),
            ("CI_WAIT_GUARD_ALLOW=true", None),
            ("CI_WAIT_GUARD_BYPASS=1", None),
        ] {
            let t = words(&["echo", w, ";", "gh"]);
            assert_eq!(CI_WAIT_GUARD.judge(None, false, Some(&t)), expect, "{w}");
        }
    }

    #[test]
    fn a_hatch_without_a_call_scope_never_reads_the_command() {
        let call_word = format!("{}_ALLOW=1", PR_TITLE_GUARD.stem);
        let t = words(&[call_word.as_str(), "SKIP_PR_TITLE_GUARD=1"]);
        assert_eq!(PR_TITLE_GUARD.judge(None, false, Some(&t)), None);
        assert_eq!(
            PR_TITLE_GUARD.judge(Some("1"), false, Some(&t)),
            Some(Scope::Session)
        );
    }

    #[test]
    fn a_call_only_hatch_ignores_the_session_scope() {
        let t = words(&["RULESETS_WRITE_GUARD_ALLOW=1"]);
        assert_eq!(RULESETS_WRITE_GUARD.judge(Some("1"), true, None), None);
        assert_eq!(
            RULESETS_WRITE_GUARD.judge(None, false, Some(&t)),
            Some(Scope::Call)
        );
        // 案内は出さない設計の guard。出すとしても、人の SKIP は書かない。
        assert_eq!(RULESETS_WRITE_GUARD.how_to(), None);
    }

    #[test]
    fn the_skip_file_counts_only_for_hatches_that_have_one() {
        assert_eq!(
            PLAN_SCOPE_GATE.judge(None, true, None),
            Some(Scope::Session)
        );
        assert_eq!(PR_TITLE_GUARD.judge(None, true, None), None);
    }

    #[test]
    fn a_gate_without_a_command_has_only_the_session_scope() {
        assert_eq!(PLAN_SCOPE_GATE.judge(None, false, None), None);
        assert_eq!(
            PLAN_SCOPE_GATE.judge(Some("1"), false, None),
            Some(Scope::Session)
        );
    }

    #[test]
    fn how_to_has_three_shapes() {
        let dual = CI_WAIT_GUARD.how_to().unwrap();
        assert!(dual.contains("CI_WAIT_GUARD_ALLOW=1") && dual.contains("SKIP_CI_WAIT_GUARD=1"));
        let session = PR_TITLE_GUARD.how_to().unwrap();
        assert!(session.contains("SKIP_PR_TITLE_GUARD=1") && !session.contains("_ALLOW"));
        assert_eq!(EXTERNAL_SEND_GUARD.how_to(), None);
        assert_eq!(JA_POLISH_GATE.how_to(), None);
    }

    #[test]
    fn active_reads_the_environment_and_the_skip_file() {
        let dir = std::env::temp_dir().join(format!("escape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let h = PLAN_VIEW;
        std::env::remove_var(h.session_name());
        assert_eq!(h.active(Some(&dir), None), None);
        std::fs::write(dir.join(SKIP_FILE), "").unwrap();
        assert_eq!(h.active(Some(&dir), None), Some(Scope::Session));
        std::fs::remove_file(dir.join(SKIP_FILE)).unwrap();
        std::env::set_var(h.session_name(), "1");
        assert_eq!(h.active(Some(&dir), None), Some(Scope::Session));
        std::env::set_var(h.session_name(), "0");
        assert_eq!(h.active(Some(&dir), None), None);
        std::env::remove_var(h.session_name());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
