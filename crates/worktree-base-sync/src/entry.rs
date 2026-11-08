// worktree-base-sync — pristine な worktree を SessionStart で
// `origin/<base>` へ黙って fast-forward する(`config/claude/hooks/
// worktree-fresh-base.sh` の移植、ADR-0024)。設計と根拠:
// docs/agents/worktree-base-sync.md。
//
// 動かしてよいのは「何も積んでいない worktree」だけ: 作業ツリーがクリーン
// かつ ahead==0 かつ behind>0。`git merge --ff-only` で原子的に前提を再強制
// する。全失敗経路は fail-open(無出力 exit 0)。動かした場合だけ
// `additionalContext` を stdout に出す。
//
// 環境変数: `CLAUDE_PROJECT_DIR`(無ければ stdin の `.cwd`)、
// `WORKTREE_FRESH_BASE_FETCH_TTL`(秒、既定 600: このフック自身が base を fetch
// した時刻の印([`STAMP`])がこれより新しければ fetch を省く)。
//
// fetch の省略は FETCH_HEAD の更新時刻では決めない: FETCH_HEAD は他の fetch
// (別のブランチの `git fetch origin <branch>` など)でも更新されるので、base の
// 追跡 ref が古いまま fetch を省き、古い土台から切った worktree に気づけなかった
// 見込み(#1344。原因は未確認の仮説)。fetch できなかったときは、手元の追跡 ref
// より遅れていれば、そのことを知らせる。

use agent_session::entry::{enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, Reply};
use git_repository::out as git;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SPEC: Spec = Spec {
    name: "worktree-base-sync",
    agents: &[Agent::Claude, Agent::Codex],
    event: Some(HookEvent::SessionStart),
};

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// このフックが base の fetch に成功した時刻の印(共通の git ディレクトリの下のファイル。更新時刻だけを使う)。
const STAMP: &str = "worktree-base-sync.fetched";

fn run() -> Option<()> {
    let raw = read_stdin();
    let input = agent_session::HookInput::parse(&raw).unwrap_or_default();
    let project = input.project_dir()?;
    let project = project.as_path();

    git(project, &["rev-parse", "--is-inside-work-tree"])?;

    let branch = git(project, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return None; // detached HEAD / rebase 中は触らない
    }
    let base = git_repository::default_branch(project)?;
    if branch == base {
        return None; // 自分自身が base なら FF する意味がない
    }
    // status が失敗しても出力は空として続行する(bash 版と同じ)。
    if !git(project, &["status", "--porcelain"])
        .unwrap_or_default()
        .is_empty()
    {
        return None;
    }

    let common_dir = git_repository::git_common_dir(project)?;
    let ttl: u64 = std::env::var("WORKTREE_FRESH_BASE_FETCH_TTL")
        .ok()
        .filter(|v| !v.is_empty())
        .map_or(600, |v| v.parse().unwrap_or(0));
    let stamp = common_dir.join(STAMP);
    let mut do_fetch = true;
    if let Ok(meta) = std::fs::metadata(&stamp) {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if now.saturating_sub(mtime) < ttl {
            do_fetch = false;
        }
    }
    let origin_base = format!("origin/{base}");
    if do_fetch {
        if git_repository::fetch_quiet(project, &base, FETCH_TIMEOUT) {
            let _ = std::fs::File::create(&stamp); // 成功した時刻を残す(更新時刻だけを使う)
        } else {
            // fetch できなかった(ネットワーク・認証・時間切れ)。最新かは分からないが、手元の追跡 ref
            // より遅れているなら、それだけは確かなので知らせる。黙って終わると、古い土台のまま作業を
            // 始めても気づけない。
            notify_if_behind(project, &base, &origin_base);
            return None;
        }
    }
    git(project, &["rev-parse", "--verify", "-q", &origin_base])?;

    let ab = git(
        project,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{origin_base}"),
        ],
    )?;
    let ahead = ab.split('\t').next().unwrap_or("");
    let behind = ab.rsplit('\t').next().unwrap_or("");
    if ahead != "0" {
        return None;
    }
    let behind_n: u64 = behind.parse().ok()?;
    if behind_n == 0 {
        return None;
    }

    let before = git(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    git(project, &["merge", "--ff-only", "--quiet", &origin_base])?;
    let after = git(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    if before.is_empty() || after.is_empty() || before == after {
        return None;
    }

    let ctx = format!("[worktree-base-sync] pristine worktree を origin/{base} へ {behind} コミット fast-forward しました({before} -> {after})。他 hook の SessionStart advisory(pr-gate の base 追従など)は並列実行の都合でこの更新前の状態を見ている場合があります。");
    print!(
        "{}",
        Reply::context(ctx).render(Agent::Claude, HookEvent::SessionStart)
    );
    Some(())
}

/// fetch できなかったときに、手元の `origin/<base>` より HEAD が遅れていれば、`additionalContext` で知らせる。
fn notify_if_behind(project: &std::path::Path, base: &str, origin_base: &str) -> Option<()> {
    git(project, &["rev-parse", "--verify", "-q", origin_base])?;
    let behind: u64 = git(
        project,
        &["rev-list", "--count", &format!("HEAD..{origin_base}")],
    )?
    .parse()
    .ok()?;
    if behind == 0 {
        return None;
    }
    let ctx = format!("[worktree-base-sync] origin/{base} を fetch できませんでした(ネットワーク・認証を確かめてください)。手元の origin/{base} に対して、この worktree は {behind} コミット遅れています(origin/{base} 自体が古い可能性もあります)。作業の前に `git fetch origin {base}` してから、`git rebase origin/{base}` で土台を追従させてください。");
    print!(
        "{}",
        Reply::context(ctx).render(Agent::Claude, HookEvent::SessionStart)
    );
    Some(())
}

fn main() -> ExitCode {
    if let Err(code) = enter(&SPEC) {
        return code;
    }
    let _ = run();
    ExitCode::SUCCESS
}
