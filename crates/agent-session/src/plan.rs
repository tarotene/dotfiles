//! クラスタ C: ExitPlanMode の plan 本文の 3 段フォールバック。
//!
//! `tool_input.plan` → `tool_input.planFilePath` → 最新の `~/.claude/plans/*.md`
//!
//! 吸収元: `plan-scope-gate.sh:307-320` / `plan-precedent-gate.sh:247-260` /
//! `plan-fresh-gate.sh:211-225` / `copilot-plan-review.sh:1580-1595` /
//! `plan-view.sh:226`。

use crate::input::HookInput;
use std::fs;
use std::path::{Path, PathBuf};

/// plan 本文を解決する。どの段でも取れなければ `None`(呼び出し側は素通し)。
pub fn plan_text(input: &HookInput) -> Option<String> {
    let plans_dir = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/plans"));
    plan_text_in(input, plans_dir.as_deref())
}

/// [`plan_text`] の plans ディレクトリを注入可能にした版(テスト用)。
pub fn plan_text_in(input: &HookInput, plans_dir: Option<&Path>) -> Option<String> {
    match plan_source_in(input, plans_dir)? {
        PlanSource::Inline(t) => Some(t),
        PlanSource::File(p) => fs::read_to_string(p).ok(),
    }
}

/// plan 本文の出どころ。本文そのものではなく**ファイルのパス**を別プロセスに
/// 渡す hook(`copilot-plan-review` は critic にパスを読ませる)が使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSource {
    /// `tool_input.plan`(非空)。
    Inline(String),
    /// `tool_input.planFilePath`(実在するファイル)か、最新の `plans/*.md`。
    File(PathBuf),
}

/// [`plan_text`] と同じ 3 段フォールバックで、本文の出どころを返す。
pub fn plan_source(input: &HookInput) -> Option<PlanSource> {
    let plans_dir = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/plans"));
    plan_source_in(input, plans_dir.as_deref())
}

/// [`plan_source`] の plans ディレクトリを注入可能にした版(テスト用)。
pub fn plan_source_in(input: &HookInput, plans_dir: Option<&Path>) -> Option<PlanSource> {
    if let Some(t) = input.tool_input.get("plan").and_then(|v| v.as_str()) {
        // bash 版は `$(jq -r '.tool_input.plan // empty')` で取っていたため、
        // 末尾の改行は落ちる — 改行だけの本文は空として次の段へ落とす。
        if !t.trim_end_matches('\n').is_empty() {
            return Some(PlanSource::Inline(t.to_string()));
        }
    }
    if let Some(p) = input
        .tool_input
        .get("planFilePath")
        .and_then(|v| v.as_str())
    {
        let p = Path::new(p);
        if !p.as_os_str().is_empty() && p.is_file() && fs::read_to_string(p).is_ok() {
            return Some(PlanSource::File(p.to_path_buf()));
        }
    }
    latest_markdown(plans_dir?).map(PlanSource::File)
}

/// `ls -t dir/*.md | head -1` 相当: 更新時刻が最新の `.md`。
fn latest_markdown(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md") && p.is_file())
        .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{Duration, SystemTime};

    fn input(v: serde_json::Value) -> HookInput {
        HookInput {
            tool_input: v,
            ..Default::default()
        }
    }

    #[test]
    fn inline_plan_wins() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.md"), "latest").unwrap();
        let i = input(json!({"plan":"inline","planFilePath":"/nonexistent"}));
        assert_eq!(plan_text_in(&i, Some(d.path())).as_deref(), Some("inline"));
    }

    #[test]
    fn plan_file_path_second() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("p.md");
        fs::write(&f, "from-file").unwrap();
        let i = input(json!({"plan":"","planFilePath": f}));
        assert_eq!(plan_text_in(&i, None).as_deref(), Some("from-file"));
    }

    #[test]
    fn latest_plan_last() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("old.md");
        let new = d.path().join("new.md");
        fs::write(&old, "old").unwrap();
        fs::write(&new, "new").unwrap();
        fs::write(d.path().join("ignored.txt"), "txt").unwrap();
        let past = SystemTime::now() - Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let i = input(json!({"planFilePath":"/nonexistent"}));
        assert_eq!(plan_text_in(&i, Some(d.path())).as_deref(), Some("new"));
    }

    #[test]
    fn source_variants() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("p.md");
        fs::write(&f, "x").unwrap();
        assert_eq!(
            plan_source_in(&input(json!({"plan":"inline"})), None),
            Some(PlanSource::Inline("inline".into()))
        );
        assert_eq!(
            plan_source_in(&input(json!({"plan":"","planFilePath": f})), None),
            Some(PlanSource::File(f.clone()))
        );
        assert_eq!(
            plan_source_in(&input(json!({})), Some(d.path())),
            Some(PlanSource::File(f))
        );
        assert_eq!(plan_source_in(&input(json!({})), None), None);
    }

    #[test]
    fn newline_only_plan_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("p.md");
        fs::write(&f, "from-file").unwrap();
        let i = input(json!({"plan":"\n\n","planFilePath": f}));
        assert_eq!(plan_text_in(&i, None).as_deref(), Some("from-file"));
    }

    #[test]
    fn nothing_found() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(plan_text_in(&input(json!({})), Some(d.path())), None);
        assert_eq!(plan_text_in(&input(json!({})), None), None);
    }
}
