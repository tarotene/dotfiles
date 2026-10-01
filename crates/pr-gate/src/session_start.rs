//! SessionStart hook: 現在状態の一覧(PR・CI・Issue リンク・視覚証跡・base 追従・
//! 未 push)と hygiene advisory を additionalContext で運ぶ。縮退はすべて exit 0
//! (SessionStart には block/pass の概念が無い)。

use crate::body::{self, Link, Visual};
use crate::config::Config;
use crate::jqv::JqError;
use crate::{gh, join_lines, jqv, repo};
use hook_io::jqfmt::J;
use hook_io::HookInput;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// `${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty')}` が実在するディレクトリか。
/// stdin が JSON として読めなくても `CLAUDE_PROJECT_DIR` があれば進む(bash 版と同じ)。
pub fn project_dir(input: &str) -> Option<PathBuf> {
    HookInput::parse(input).unwrap_or_default().project_dir()
}

/// allowlist 内の GitHub リポジトリの worktree なら (project, nwo)。
pub fn target(cfg: &Config, project: &Path) -> Option<String> {
    if !repo::git_succeeds(project, &["rev-parse", "--is-inside-work-tree"]) {
        return None;
    }
    let nwo = repo::owner_repo(project)?;
    repo::allowlisted(&cfg.allowlist, &nwo).then_some(nwo)
}

fn emit(ctx: &str) {
    let out = J::obj(vec![(
        "hookSpecificOutput",
        J::obj(vec![
            ("hookEventName", J::str("SessionStart")),
            ("additionalContext", J::str(ctx)),
        ]),
    )]);
    println!("{}", out.pretty());
}

/// FETCH_HEAD が `ttl` 秒より新しければ fetch を省く。fetch に失敗したら注記を返す。
fn maybe_fetch(cfg: &Config, project: &Path) -> Option<String> {
    let common = hook_io::git::git_common_dir(project)?;
    let fetch_head = common.join("FETCH_HEAD");
    let mut do_fetch = true;
    if fetch_head.is_file() {
        let mtime = std::fs::metadata(&fetch_head)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let age = SystemTime::now()
            .duration_since(mtime)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        if age < cfg.fetch_ttl {
            do_fetch = false;
        }
    }
    let mut note = String::new();
    if do_fetch {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C")
            .arg(project)
            .args(["fetch", "--quiet", "--prune", "origin"]);
        let ok = hook_io::proc::output_with_timeout(&mut cmd, Duration::from_secs(15), None)
            .is_some_and(|(st, _)| st.success());
        if !ok {
            note = " (リモート未確認: fetch 失敗)".to_string();
        }
    }
    Some(note)
}

/// `[.[].bucket] | group_by(.) | map("\(.[0]): \(length)") | join(", ")`。
fn bucket_summary(checks_json: &str) -> String {
    let unknown = "不明".to_string();
    let Ok(v) = serde_json::from_str::<Value>(checks_json) else {
        return unknown;
    };
    // `if length == 0 then "不明"`(length がエラーになる形も jq の失敗 → "不明")。
    if !matches!(jqv::length(&v), Ok(n) if n > 0) {
        return unknown;
    }
    let Ok(items) = jqv::iter(&v) else {
        return unknown;
    };
    let mut buckets = Vec::new();
    for i in items {
        match jqv::field(i, "bucket") {
            Ok(b) => buckets.push(b),
            Err(JqError) => return unknown,
        }
    }
    buckets.sort_by(jqv::cmp);
    let mut groups: Vec<(Value, usize)> = Vec::new();
    for b in buckets {
        match groups.last_mut() {
            Some((g, n)) if jqv::cmp(g, &b).is_eq() => *n += 1,
            _ => groups.push((b, 1)),
        }
    }
    groups
        .iter()
        .map(|(g, n)| format!("{}: {n}", jqv::raw(g)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn run(input: &str) -> i32 {
    if !hook_io::proc::command_exists("gh") || !hook_io::proc::command_exists("git") {
        return 0;
    }
    let cfg = Config::from_env();
    let Some(project) = project_dir(input) else {
        return 0;
    };
    let Some(nwo) = target(&cfg, &project) else {
        return 0;
    };
    if cfg.skipped() {
        return 0;
    }
    let Some(fetch_note) = maybe_fetch(&cfg, &project) else {
        return 0;
    };
    let branch = repo::git_ok(&project, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return 0;
    }

    // hygiene advisory(事故①④): PR の有無を問わず計算できる。API は叩かない。
    let gone_line = repo::gone_branches_line(&project);
    let stale_wt_line = repo::stale_worktrees_line(&project);

    let pr_json = gh::pr_for_branch(&nwo, &branch, "number,baseRefName,headRefOid,body");
    // bash 版は gh の応答が JSON として読めないと jq の失敗で異常終了していた。
    // SessionStart はそもそも何も止められないので、ここでは黙って exit 0 にする。
    let Ok(prs) = serde_json::from_str::<Value>(&pr_json) else {
        return 0;
    };
    let Some(pr_num) = jqv::first_field(&prs, "number") else {
        // 以前はここで無条件 exit 0 していたため、PR を作る前のセッションでは
        // base がどれだけ遅れていても [gone] が何本あっても一切表示されなかった
        // (この worktree 自身が origin/main から 4 コミット遅れていて実際に無音
        // だったケース)。PR 有無自体は issue-index が示すので、ここでは
        // hygiene の材料が無ければやはり無音にする。
        let default_br = hook_io::git::default_branch(&project).unwrap_or_default();
        let stale_line = repo::stale_base_line(&project, &default_br);
        let hygiene = join_lines(&[stale_line, gone_line, stale_wt_line]);
        if !hygiene.is_empty() {
            emit(&format!("[pr-gate] {hygiene}"));
        }
        return 0;
    };

    let first = &prs[0];
    let base = jqv::raw(first.get("baseRefName").unwrap_or(&Value::Null));
    let head_oid = jqv::raw(first.get("headRefOid").unwrap_or(&Value::Null));

    let (ahead, behind) = repo::ahead_behind(&project, &base);
    let unpushed = repo::unpushed_since(&project, &head_oid);
    let summary = bucket_summary(&gh::reported_checks(&pr_num, &nwo));
    let pr_body = jqv::first_field(&prs, "body").unwrap_or_default();

    let link_state = match body::judge_link(&pr_body) {
        Link::Linked => "closing keyword あり",
        Link::NoIssue => "No-Issue: 宣言あり",
        Link::Missing => "なし — Closes #N か No-Issue: が要ります",
    };
    let visual_state = match body::judge_visual(&pr_body) {
        Visual::Image => "画像あり",
        Visual::CodeBlock => "Before/After コードブロックあり",
        Visual::NoVisual => "No-Visual: 宣言あり",
        Visual::Missing => "なし — Before/After 画像/コード対比か No-Visual: が要ります",
    };

    let mut ctx = format!(
        "[pr-gate] PR #{pr_num} (base: {base}) — CI: {summary}{fetch_note}\nIssue リンク: {link_state}\n視覚証跡: {visual_state}\nbase 追従: ahead {ahead} / behind {behind}\n未 push: {unpushed} 件"
    );
    // PR がある場合の base 追従は上の ahead/behind 行がすでに単独で運んでいるので
    // stale_base_line は重ねない。gone/worktree 残骸は PR の有無と無関係な情報
    // なので、こちらには追加する。
    let extra = join_lines(&[gone_line, stale_wt_line]);
    if !extra.is_empty() {
        ctx.push('\n');
        ctx.push_str(&extra);
    }
    emit(&ctx);
    0
}
