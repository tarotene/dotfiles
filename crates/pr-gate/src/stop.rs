//! Stop hook: PR completion barrier。block は `StopOutcome::Block`(終了コード 0 の標準出力の JSON。
//! 描画は `agent_session::Reply::render`)、素通りは `None`(advisory があれば stderr に出す)。

use crate::body::{self, Link, Visual};
use crate::config::Config;
use crate::gates::ci::{self, Status};
use crate::gates::handoff::{self, Handoff};
use crate::gates::{prior, stack};
use crate::state::{self, State};
use crate::{gh, join_lines, jqv, repo, session_start};
use agent_session::HookInput;
use agent_session::{Reentry, StopOutcome};
use serde_json::Value;
use std::path::Path;

/// `session_id // "unknown"` をファイル名に使える形にしたもの。
fn session_id(input: &str) -> String {
    let sid = HookInput::parse(input)
        .map(|i| i.session_id)
        .unwrap_or_else(|| "unknown".to_string());
    agent_session::ledger::sanitize_session_id(&sid)
}

/// 再入(`stop_hook_active`)は見ない。block の直後の再呼び出しでも判定する。無限に block しないための
/// 上限は、`State` の session_id ごとのカウンタが持つ(上限到達で escalate して以後は素通り)。
pub const REENTRY: Reentry = Reentry::Recheck;

pub fn run(input: &str) -> Option<StopOutcome> {
    if !process_control::command_exists("gh") || !process_control::command_exists("git") {
        return None;
    }
    let cfg = Config::from_env();
    if cfg.skipped() {
        return None;
    }
    let sid = session_id(input);
    let project = session_start::project_dir(input)?;
    let nwo = session_start::target(&cfg, &project)?;
    // 同じ linked worktree を別のセッションが今使っているときは、そちらが動かしたブランチ・
    // 作った commit・PR をこのセッションのものとして block しない(2026-10-27 に別セッションの
    // PR #1173 が帰属された)。判定は worktree-lease の印(docs/agents/worktree-lease.md)。
    if HookInput::parse(input)
        .is_some_and(|i| worktree_lease::held_by_other(&project, &i.session_id))
    {
        return None;
    }

    state::ensure_dirs(&cfg);
    let st = State::new(&cfg);
    if st.is_escalated(&sid) || st.at_limit(&sid) {
        return None;
    }

    let branch = repo::git_ok(&project, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return None;
    }

    let pr_json = gh::pr_for_branch(&nwo, &branch, "number,baseRefName,headRefOid,body,isDraft");
    // bash 版は gh の応答が JSON として読めないと jq の失敗(終了コード 2 = block 扱い
    // になりうる)で異常終了していた。判定不能なので素通す。
    let Ok(prs) = serde_json::from_str::<Value>(&pr_json) else {
        return None;
    };
    match jqv::first_field(&prs, "number") {
        None => no_pr(&st, &sid, &project, &nwo, &branch),
        Some(pr_num) => {
            let transcript = transcript_path(input);
            with_pr(
                &cfg,
                &st,
                &sid,
                &project,
                &nwo,
                &branch,
                &prs,
                &pr_num,
                &transcript,
            )
        }
    }
}

/// hook 入力の `transcript_path`(無い・文字列でないときは空)。
fn transcript_path(input: &str) -> String {
    serde_json::from_str::<Value>(input)
        .ok()
        .and_then(|v| v.get("transcript_path")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// `[[ "$x" =~ ^[0-9]+$ ]] && [[ "$x" -gt 0 ]]`。
fn positive(x: &str) -> bool {
    !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()) && x.parse::<u64>().is_ok_and(|n| n > 0)
}

/// PR がまだ無いブランチ: G_unpushed → G_pr。
///
/// G_push は open PR の headRefOid が無いと判定できない。以前はここで
/// 無条件に完全沈黙していたため、コミットを積んで push せずに終わる
/// セッションを何も止めなかった。PR を作るべきかには踏み込まず、
/// 「積んだコミットが 1 個もリモートに無い」事実だけを見る(G_unpushed)。
fn no_pr(st: &State, sid: &str, project: &Path, nwo: &str, branch: &str) -> Option<StopOutcome> {
    let unpushed = repo::unpushed_count(project, branch);
    let default_br = git_repository::default_branch(project).unwrap_or_default();
    if positive(&unpushed) {
        let hygiene = join_lines(&[
            repo::stale_base_line(project, &default_br),
            format!("未コミット: {} ファイル", repo::uncommitted_count(project)),
        ]);
        return Some(st.block_or_escalate(
            sid,
            &format!(
                "未 push の commit が {unpushed} 件あります。PR はまだありません。

push し、そのまま `gh pr create` まで実行してから終了してください
(push.autoSetupRemote が upstream を自動で設定するので --set-upstream の
指定は要りません)。本文は pr-description スキルの 5 節スケルトンに従い、
1 行目に `Closes #<番号>` か `No-Issue: <理由>`、`## Before / After` に
証跡(または `No-Visual: <理由>`)を入れてください。

{hygiene}"
            ),
        ));
    }

    // G_pr — push 済みなのに PR が無いまま終わろうとする事故を塞ぐ。
    // G_unpushed は「push したか」しか見ないため、push 済みブランチが
    // PR を作らずに終わるケースはここまで無条件で完全沈黙していた
    // (詳細: docs/agents/pr-gate.md の G_pr 節)。判定できる場合だけ block し、
    // 判定できないときは断定に変えず素通す(以下はすべて exit 0 の skip 条件):
    //   - unpushed が "?"(upstream 不明。判定不能)
    //   - default_br が空(origin/HEAD 未設定。判定不能)
    //   - branch が default branch そのもの(PR フロー外の直接作業)
    if unpushed != "0" || default_br.is_empty() || branch == default_br {
        return None;
    }
    let (ahead, _) = repo::ahead_behind(project, &default_br);
    // ahead が非数値("?": origin/<default_br> が手元に無い)か 0
    // (PR に値する commit が無い)なら判定できない/対象外として素通す。
    if !positive(&ahead) {
        return None;
    }

    // merged/closed の PR が既にある場合は block しない — squash merge 後の
    // 残骸ブランチ(ahead>0 のまま残る)や、意図的に閉じた PR まで block
    // すると「PR を作れ」という案内が誤りになる。gh 失敗時は fail-open
    // (空応答と API 失敗を区別せず、どちらも「分からないので止めない」に倒す)。
    let all_json = gh::pr_for_branch_any_state(nwo, branch)?;
    let Ok(all) = serde_json::from_str::<Value>(&all_json) else {
        return None;
    };
    if jqv::first_field(&all, "state").is_some_and(|s| !s.is_empty()) {
        return None;
    }

    let hygiene = join_lines(&[format!(
        "未コミット: {} ファイル",
        repo::uncommitted_count(project)
    )]);
    Some(st.block_or_escalate(
        sid,
        &format!(
            "ブランチ {branch} は push 済みですが PR がありません
(origin/{default_br} に対し ahead {ahead})。

`gh pr create` まで実行してから終了してください。本文は pr-description
スキルの 5 節スケルトンに従い、1 行目に `Closes #<番号>` か
`No-Issue: <理由>`、`## Before / After` に証跡(または
`No-Visual: <理由>`)を入れてください(G_link / G_visual が後で検査します)。

{hygiene}"
        ),
    ))
}

/// 本文由来の block メッセージ群(G_link / G_stack / G_visual / G_prior)。
/// 判定だけ先に済ませ、block は最後に回す。G_push / G_CI が止める場面では、その
/// block メッセージに G_stack 以外を相乗りさせる(本文の修正は CI を待たずに済む
/// ので、単独で 1 往復を消費させる理由がない。G_stack は CI が通った後の単独
/// block だけ)。
#[derive(Default)]
struct BodyFindings {
    link: Option<String>,
    stack: Option<String>,
    visual: Option<String>,
    prior: Option<String>,
}

impl BodyFindings {
    /// G_push / G_CI の block に相乗りさせる文面(link → visual → prior → advisory の
    /// 順)。stack は載せない: `gh stack link` は CI が緑になってから 1 回だけ行う
    /// 手順(stacked-pr スキル、github/gh-stack#319)なので、CI 待ちの block で案内
    /// すると手順書と食い違う。stack の案内は `standalone` の単独 block だけが担う。
    fn rider(&self, advisory: &str) -> String {
        let mut parts: Vec<&str> = [&self.link, &self.visual, &self.prior]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        parts.push(advisory);
        parts.join("\n\n")
    }

    /// 単独 block の本文(link → visual → stack → prior、空なら `None`)。`with_stack` が
    /// false のときは stack を載せない(CI がまだ緑でないとき)。
    fn standalone(&self, with_stack: bool) -> Option<String> {
        let stack = if with_stack { &self.stack } else { &None };
        let parts: Vec<&str> = [&self.link, &self.visual, stack, &self.prior]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }
}

#[allow(clippy::too_many_arguments)]
fn with_pr(
    cfg: &Config,
    st: &State,
    sid: &str,
    project: &Path,
    nwo: &str,
    branch: &str,
    prs: &Value,
    pr_num: &str,
    transcript: &str,
) -> Option<StopOutcome> {
    let first = &prs[0];
    let base = jqv::raw(first.get("baseRefName").unwrap_or(&Value::Null));
    let head_oid = jqv::raw(first.get("headRefOid").unwrap_or(&Value::Null));

    // advisory: base 追従 / 未コミット(block 時だけ相乗り、単独では終了を止めない)
    let (ahead, behind) = repo::ahead_behind(project, &base);
    let wt_count = repo::uncommitted_count(project);
    let mut advisory =
        format!("base 追従: ahead {ahead} / behind {behind}\n未コミット: {wt_count} ファイル");

    // 中断ハンドオフ(Handoff: #N) — G_link / G_CI の緩和条件。判定不能なら
    // 緩めない(fail-closed)。docs/agents/pr-gate.md「中断ハンドオフ」節。
    let pr_body = jqv::first_field(prs, "body").unwrap_or_default();
    let is_draft = jqv::first_field(prs, "isDraft").unwrap_or_else(|| "false".into()) == "true";
    let handoff_issue = match is_draft.then(|| handoff::judge(&pr_body, nwo)) {
        Some(Handoff::Ok(n)) => Some(n),
        _ => None,
    };

    let mut f = BodyFindings::default();

    // G_link
    let default_br = git_repository::default_branch(project).unwrap_or_default();
    match (body::judge_link(&pr_body), &handoff_issue) {
        (Link::Missing, Some(h)) => {
            advisory.push_str(&format!(
                "\nHandoff: #{h}(open)を検出したため、closing keyword 省略を中断
ハンドオフとして許容します。再開時は完了する子 Issue の `Closes #…` に
書き換えてください(親 #{h} への Closes は、その merge で親の
全 sub-issue が closed になり親の完了定義も満たすときだけ)。"
            ));
        }
        (Link::Missing, None) => {
            f.link = Some(format!(
                "PR #{pr_num} の本文が Issue を閉じません(closing keyword なし)。

このままマージしても Issue は open のまま残り、後日の再トリアージまで
誰も気づきません。次のどちらかを本文に入れてください:

  Closes #<番号>          — 対応する Issue がある場合(複数なら各行に)
  No-Issue: <理由>        — 対応する Issue が本当に無い場合
  Handoff: #<番号>        — 中断ハンドオフの Draft PR で、対応する Issue が open な場合

  gh pr edit {pr_num} --body-file <file>

open な Issue の一覧は SessionStart の issue-index が注入しています。"
            ));
        }
        (Link::Linked, _) if !default_br.is_empty() && base != default_br => {
            // 公式仕様: closing keyword は default branch を狙う PR でのみ解釈される。
            // stacked PR 自体は正当なので断定せず advisory に留める。
            advisory.push_str(&format!(
                "\n注意: base が {base} で default branch({default_br})ではないため、本文の
      closing keyword はマージしても発火しません。"
            ));
        }
        _ => {}
    }

    // G_link(1 keyword 1 Issue、#722): `Closes #1 #2` は #1 しか閉じない。
    let listed = body::unclosed_listed_refs(&pr_body);
    if !listed.is_empty() {
        let msg = format!(
            "PR #{pr_num} の本文に、1 つの closing keyword の後ろに番号を並べた書き方があります。
GitHub は keyword 直後の 1 件しか閉じないため、次は閉じられずに open のまま残ります:

  {}

keyword は Issue ごとに書いてください(各行に `Closes #<番号>`)。

  gh pr edit {pr_num} --body-file <file>",
            listed.join(" ")
        );
        f.link = Some(match f.link.take() {
            Some(prev) => format!("{prev}\n\n{msg}"),
            None => msg,
        });
    }

    // G_visual
    if body::judge_visual(&pr_body) == Visual::Missing {
        f.visual = Some(format!(
            "PR #{pr_num} の本文に Before/After の視覚証跡がありません。
次のいずれかを本文の `## Before / After` 節に入れてください:

  画像:       gh pr edit {pr_num} --attach './before.png#Before' --attach './after.png#After'
              (gh 2.99.0 以降が必要)
  コード対比: 見出し配下に fenced code block でテキストの Before/After を書く
              (見た目の差分がテキストで十分伝わる場合)
  No-Visual: <理由>  — 外観に影響しない変更の場合(1 行、理由必須)

撮り方・Before の取り方は pr-description スキルを参照。"
        ));
    }

    // G_stack(ADR-0027)。head branch は PR 一覧の応答に含めていない(headRefName
    // を追加すると G_push 等の縮退判定と無関係に応答形を太らせるだけ)ので、
    // --head フィルタで絞り込みに使った現在のチェックアウトブランチをそのまま
    // 使う — 両者は定義上一致する。
    let chain = gh::open_prs(nwo)
        .filter(|s| !s.is_empty())
        .and_then(|s| stack::parse_rows(&s))
        .and_then(|rows| stack::chain(&rows, pr_num, branch, &base));
    if let Some(chain) = chain.filter(|c| c.len() >= 2) {
        if !gh::stack_extension_available() {
            advisory.push_str(
                "\nstack: gh-stack 拡張が無いため `gh stack link` できません(base チェーン
       自体は stack-base-guard が別途強制しています)。導入するには
       `gh extension install github/gh-stack`。",
            );
        } else {
            match gh::stacks(nwo).filter(|s| !s.is_empty()) {
                Some(stacks_json) => {
                    // `gh stack link` は、全段の CI が緑になってから 1 回だけ行う手順(stacked-pr
                    // スキル。link 後は push の CI が走らない)。現在の PR の CI が通っていても、
                    // チェーンの別の段の CI が pending のうちは案内しない(最上段の CI が走っている
                    // 間に Stop して link を求められた、wrap-up inbox の気づき)。
                    let waiting_stages: Vec<&str> = chain
                        .iter()
                        .map(String::as_str)
                        .filter(|n| *n != pr_num)
                        .filter(|n| {
                            !ci::pending_names(&gh::reported_checks(n, nwo), &[]).is_empty()
                        })
                        .collect();
                    if !waiting_stages.is_empty() {
                        advisory.push_str(&format!(
                            "\nstack: 別の段(#{})の CI が pending のため、stack のリンクはまだ案内しません。\
                             全段の CI が緑になってから 1 回だけリンクしてください(stacked-pr スキル)。",
                            waiting_stages.join(", #")
                        ));
                    } else if !stack::is_linked(&stacks_json, &chain) {
                        f.stack = Some(format!(
                            "stacked PR チェーン(#{})が
GitHub 上の stack にリンクされていません。最下段から順に PR **番号**を
指定して(ブランチ名ではない — link はブランチ引数だと未作成の PR を
自動生成する副作用があります)リンクしてください:

  gh stack link {}",
                            chain.join(" → #"),
                            chain.join(" ")
                        ));
                    }
                }
                None => advisory.push_str(
                    "\nstack: stacks API の取得に失敗しました(拡張は導入済み)。ネットワーク・
       認証・機能撤収のいずれかの可能性があります。base チェーン自体は
       stack-base-guard が別途強制しています。",
                ),
            }
        }
    }

    // G_prior(ADR-543)。base ref が手元に無い等で判定不能なら何も起きない。
    let added = repo::added_files(project, &base);
    if !added.is_empty() {
        let missing = prior::judge(project, &pr_body, &added);
        if !missing.is_empty() {
            // bash 版は `printf '  - %s\n' "$missing_kizon"` に改行区切りの 1 引数を
            // 渡していたため、2 件目以降に "  - " が付かない。移植では挙動を保つ。
            // 台帳(new-tool-guard)に登録済みの行があれば、そのまま本文に追記できる形で載せる。
            let registered = prior::registered_lines(project, &missing);
            let registered_block = if registered.is_empty() {
                String::new()
            } else {
                format!(
                    "\n登録済みの行(new-tool-guard の台帳から。そのまま `## 解決策` 節に追記できます):\n\n  {}\n",
                    registered.join("\n  ")
                )
            };
            f.prior = Some(format!(
                "PR #{pr_num} で新しい道具・単位を追加していますが、本文に
`既存手段:` の記載がありません(ADR-543「既存手段の前倒し接地と、決定論
への昇格導線」):

  - {}
{registered_block}
既存の枯れた技術で足りないか検討し、次のいずれかの形式で本文の
`## 解決策` 節に1行ずつ追記してから再試行してください(pr-description
スキル):

  既存手段: <path> — 採用: <ツール名/URL>
  既存手段: <path> — 拡張: <既存パス>
  既存手段: <path> — 自前 — 却下: <候補> (<理由>)

  gh pr edit {pr_num} --body-file <file>",
                missing.join("\n")
            ));
        }
    }

    let rider = f.rider(&advisory);

    // G_push
    let local_head = repo::git_ok(project, &["rev-parse", "HEAD"]).unwrap_or_default();
    if !local_head.is_empty() && local_head != head_oid {
        let unpushed = repo::unpushed_since(project, &head_oid);
        let short: String = head_oid.chars().take(7).collect();
        return Some(st.block_or_escalate(
            sid,
            &format!(
                "未 push の commit が {unpushed} 件あります。
CI の緑は古い head ({short}) の結果です。

push してから終了してください。
注: herdr worktree からの push は pre-push の例外です(#39)。手動で切った
    worktree の場合のみ阻まれるので、その場合は親ブランチへ戻してから push
    するか、ユーザーに確認してください(--no-verify など pre-push を無効に
    する形は bleep が deny します。docs/git-sync.md)。

{rider}"
            ),
        ));
    }

    // G_CI — Handoff 成立時は block ではなく advisory の rider に回す(中断中の
    // WIP に CI green を要求しないため)。判定不能(API_FAILURE)も同様に緩める
    // — Handoff の成立自体は既に fail-closed で判定済みなので、ここでの緩和は
    // 「揃っていない集合を緑と読む」ことにはならない(CI を評価しないだけ)。
    let default_base = git_repository::default_branch(project);
    let outcome = ci::run(cfg, nwo, pr_num, &base, default_base.as_deref());
    // 背景の待機に任せて CI を block しなかったか(単独 block に stack の案内を載せない。
    // `gh stack link` は CI が緑になってからの手順)。
    let mut ci_deferred = false;
    if outcome.status != Status::Pass {
        if let Some(h) = &handoff_issue {
            advisory.push_str(&format!(
                "\nG_CI: {}(Handoff: #{h} により advisory — 再開して
      `gh pr ready` するまで CI green は要求しません)",
                outcome.status.as_str()
            ));
        } else if matches!(
            outcome.status,
            Status::Empty | Status::Missing | Status::Pending
        ) && agent_session::transcript::waiting_on_background(transcript)
        {
            // CI が出揃うのを背景の待機(pr-checks-wait 等)が待っている。完了すれば通知が届き、
            // 次のターンで結果を読める。ここで block すると、待機を起動した直後のターンが
            // 差し戻される(wrap-up inbox の気づき)。
            ci_deferred = true;
            advisory.push_str(&format!(
                "\nG_CI: {}(未完了の背景タスクを検出したため advisory — 完了通知を待ってください。
      CI が出揃わない・赤のまま終わるときは、次の Stop で判定し直します)",
                outcome.status.as_str()
            ));
        } else {
            let short: String = head_oid.chars().take(7).collect();
            let msg = match outcome.status {
                Status::Empty => format!(
                    "CI のチェックがまだ 1 件も報告されていません。
pr-checks-wait {pr_num} で確認してから終わってください。

{rider}"
                ),
                Status::Missing => format!(
                    "CI のチェックが揃っていません。未出現: {}
`pr-checks-wait {pr_num}` を run_in_background で待ってから終わってください
(赤 1 件で即 NG、required 全緑で即 OK を返します)。

{rider}",
                    outcome.detail
                ),
                Status::ApiFailure => format!(
                    "required チェック集合の取得に失敗しました(gh api の呼び出しエラー、
または応答のパースに失敗)。ネットワーク・認証・API レート制限等の一時的な
障害の可能性があります。required が実在しないと確定できないまま quiesce
判定に倒すと、揃っていないチェック集合を緑と読みかねません。

  gh api repos/{nwo}/rules/branches/{base}

で手動確認するか、しばらく待って再実行してください。

{rider}"
                ),
                Status::Failed => format!(
                    "CI が赤です。PR #{pr_num} (head {short})

{}
修正して push してから終わってください。

{rider}",
                    ci::render_failed(&gh::reported_checks(pr_num, nwo))
                ),
                _ => format!(
                    "CI がまだ pending です。`pr-checks-wait {pr_num}` を run_in_background で待ってから終わってください
(赤 1 件で即 NG、required 全緑で即 OK を返します)。

{rider}"
                ),
            };
            return Some(st.block_or_escalate(sid, &msg));
        }
    }

    if outcome.status == Status::Pass && !outcome.detail.is_empty() {
        advisory.push('\n');
        advisory.push_str(&outcome.detail);
    }

    // G_link / G_visual / G_stack / G_prior を単独で block するのはここ — push
    // も CI も通っている、つまり「あとは終わるだけ」の一点。まさに本文の不備・
    // stack リンク忘れ・既存手段: 記載漏れが見落とされる瞬間なので、この位置で
    // 止める。欠けているものが複数あれば 1 つの block メッセージに合流させ、
    // まとめて 1 往復で直せるようにする(いずれも CI を再走させない修正のため)。
    if let Some(body_msg) = f.standalone(!ci_deferred) {
        return Some(st.block_or_escalate(sid, &format!("{body_msg}\n\n{advisory}")));
    }

    eprintln!("{advisory}");
    None
}
