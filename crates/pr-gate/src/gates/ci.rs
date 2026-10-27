//! G_CI: 期待集合の取得と判定、および待ち(`pr-checks-wait` と Stop の共通部)。
//!
//! 中心不変条件は「揃っていない集合を緑と読まないこと」。次の 3 経路で
//! `gh pr checks` は「チェックが無い/揃っていない」を「exit 0」で返す。どの経路でも
//! 判定をその exit code に置かない — 期待集合をサーバから取り、判定は
//! [`decide`] が `--json` の取り直しに対して行う:
//!   1. push 直後で check run がまだ API に現れていない (cli/cli#7401)
//!   2. ruleset の対象外(base が main 以外の stacked PR)で required が 0 件
//!   3. 6 件のうち一部だけが現れ、その部分集合が pass した時点で
//!      `--watch` が早期終了する (cli/cli#9973) — これが一番危ない。
//!      「1 件以上現れたら待機開始」では防げないので、`gh pr checks --watch` は
//!      呼ばず、取り直した `--json` を [`decide`] に通す。
//!
//! 判定は 2 つの集合を分ける。NG は**報告された全チェック**(required かどうかを
//! 問わず、赤 1 件で即時)、OK は**期待集合 E**(required の context)の全件が緑に
//! なった時点で即時(required でないチェックの pending は待たない)。
//! `gh pr checks --required --watch --fail-fast` では、集約ジョブ(`CI passed`)が
//! 全ジョブの完了後にしか結果を出さないため、先に落ちたジョブの NG が遅れる。

use crate::config::Config;
use crate::jqv::JqError;
use crate::{gh, jqv};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Empty,
    Missing,
    Failed,
    Pending,
    ApiFailure,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Empty => "EMPTY",
            Status::Missing => "MISSING",
            Status::Failed => "FAILED",
            Status::Pending => "PENDING",
            Status::ApiFailure => "API_FAILURE",
        }
    }
}

/// 判定結果。`detail` は MISSING なら未出現のチェック名、PASS なら quiesce の注記。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub status: Status,
    pub detail: String,
}

impl Outcome {
    fn of(status: Status) -> Self {
        Outcome {
            status,
            detail: String::new(),
        }
    }
}

/// サーバの ruleset から base ブランチの required_status_checks の context 名一覧を
/// 取る。`Ok(空)` は ruleset に required_status_checks が無い/base がスコープ外の
/// 場合(= quiesce モードで報告された全チェックを判定材料にする、区別不要)。
/// gh api 呼び出し自体の失敗、または応答が非空なのにパースに失敗した場合は
/// `Err` — 呼び出し側はこれを quiesce に縮退させず block する(#135: この区別が
/// 無いと、required が実在する状態で一時的な API 障害が起きたとき、揃って
/// いない集合を黙って緑と読んでしまう)。
pub fn expected_contexts(nwo: &str, base: &str) -> Result<Vec<Value>, JqError> {
    let raw = gh::branch_rules(nwo, base).ok_or(JqError)?;
    parse_expected(&raw)
}

/// `jq -c '[.[] | select(.type=="required_status_checks")
///   | .parameters.required_status_checks[].context]'`。
pub fn parse_expected(raw: &str) -> Result<Vec<Value>, JqError> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(raw).map_err(|_| JqError)?;
    let mut out = Vec::new();
    for rule in jqv::iter(&v)? {
        if jqv::field(rule, "type")? != Value::String("required_status_checks".into()) {
            continue;
        }
        let params = jqv::field(rule, "parameters")?;
        let checks = jqv::field(&params, "required_status_checks")?;
        for c in jqv::iter(&checks)? {
            out.push(jqv::field(c, "context")?);
        }
    }
    Ok(out)
}

static RUN_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/runs/([0-9]+)(?:/|$)").expect("RUN_ID_RE"));

/// チェックの `link` にある workflow run の ID。外部の status context など、無ければ `None`。
fn run_id(check: &Value) -> Option<u64> {
    let link = check.get("link")?.as_str()?;
    RUN_ID_RE.captures(link)?[1].parse().ok()
}

/// 同じ workflow に複数の run があれば、run ID が最大のもの(最新)のチェックだけを残す。
/// 同じ head に run が重なる(base ブランチの更新で `synchronize` が重ねて飛ぶ)と、先の run は
/// concurrency で cancel され、その集約ジョブ(`CI passed`)は fail で報告される。新しい run の
/// 同名のチェックがまだ作られていない間、その古い fail を NG と読むと誤報になる(#1157 で
/// 実際に起きた)。re-run は同じ run ID のままなので、これで落ちない。run ID の無いチェックは残す。
fn latest_runs_only(all: Vec<Value>) -> Vec<Value> {
    let mut newest: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for c in &all {
        if let Some(id) = run_id(c) {
            let wf = c.get("workflow").and_then(Value::as_str).unwrap_or("");
            let e = newest.entry(wf.to_string()).or_insert(id);
            *e = (*e).max(id);
        }
    }
    all.into_iter()
        .filter(|c| match run_id(c) {
            Some(id) => {
                let wf = c.get("workflow").and_then(Value::as_str).unwrap_or("");
                newest.get(wf).is_none_or(|n| id >= *n)
            }
            None => true,
        })
        .collect()
}

/// 報告集合(`gh pr checks --json`)。配列として読めなければ `None`。置き換えられた古い run の
/// チェックは除く([`latest_runs_only`])。
fn parse_reported(s: &str) -> Option<Vec<Value>> {
    match serde_json::from_str::<Value>(s).ok()? {
        Value::Array(a) => Some(latest_runs_only(a)),
        _ => None,
    }
}

fn names(reported: &[Value]) -> Result<Vec<Value>, JqError> {
    reported.iter().map(|r| jqv::field(r, "name")).collect()
}

fn bucket_is(r: &Value, buckets: &[&str]) -> Result<bool, JqError> {
    let b = jqv::field(r, "bucket")?;
    Ok(buckets.iter().any(|x| b == Value::String((*x).into())))
}

/// 取り直した 1 回分の報告に対する判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 報告のどれかが fail(即時 NG)、または cancel があって pending が 0(他が落ち着いても残る cancel)。
    Failed,
    /// E が非空で、E の全件が出現し、すべて pass / skipping(即時 OK)。
    Pass,
    /// まだ決まらない。`missing` は E のうち未出現の名前、`pending` は報告のうち
    /// pending の件数、`reported` は報告の件数。
    Wait {
        missing: Vec<Value>,
        pending: usize,
        reported: usize,
    },
}

/// 報告(`gh pr checks --json`)を期待集合 `expected` に照らして判定する純関数。
///
/// - 報告に fail が 1 件でもあれば `Failed`(E に入っていなくても)。cancel は、同じ head の
///   後続の run に置き換えられただけのことがある(concurrency の `cancel-in-progress`)ので
///   即時 NG にはしない: E が全緑なら `Pass`、pending が残る間は待ち、pending が 0 になって
///   なお cancel が残るときに `Failed`。
/// - `expected` が非空で、E ⊆ 報告 かつ E の全件の bucket が pass / skipping なら `Pass`。
///   E に無いチェックの pending はここでは見ない。
/// - それ以外は `Wait`。`expected` が空(base が ruleset の対象外)のときは `Pass` を
///   返さない — 「揃った」の近似(静止)は状態を持つ [`wait`] の仕事。
/// - 配列として読めない・要素がオブジェクトでない報告は、緑と読まず `Wait` に倒す
///   (旧 `judge` は全ゼロ = 素通りに倒していた)。
pub fn decide(expected: &[Value], reported: &str) -> Verdict {
    let unreadable = || Verdict::Wait {
        missing: expected.to_vec(),
        pending: 0,
        reported: 0,
    };
    let Some(r) = parse_reported(reported) else {
        return unreadable();
    };
    let mut pending = 0;
    let mut cancelled = 0;
    for x in &r {
        match bucket_is(x, &["fail"]) {
            Ok(true) => return Verdict::Failed,
            Ok(false) => {}
            Err(JqError) => return unreadable(),
        }
        match bucket_is(x, &["cancel"]) {
            Ok(true) => cancelled += 1,
            Ok(false) => {}
            Err(JqError) => return unreadable(),
        }
        match bucket_is(x, &["pending"]) {
            Ok(true) => pending += 1,
            Ok(false) => {}
            Err(JqError) => return unreadable(),
        }
    }
    let Ok(reported_names) = names(&r) else {
        return unreadable();
    };
    let missing = jqv::subtract(expected, &reported_names);
    if !expected.is_empty() && missing.is_empty() {
        let all_green = r.iter().all(|x| {
            let in_e = jqv::field(x, "name")
                .map(|n| expected.iter().any(|e| jqv::eq(e, &n)))
                .unwrap_or(false);
            !in_e || bucket_is(x, &["pass", "skipping"]).unwrap_or(false)
        });
        if all_green {
            return Verdict::Pass;
        }
    }
    if cancelled > 0 && pending == 0 {
        return Verdict::Failed;
    }
    Verdict::Wait {
        missing,
        pending,
        reported: r.len(),
    }
}

/// 待ちの結果の注記つき期待集合。`Ok((E, 注記))`。E が空なら quiesce で判定する。
///
/// base の ruleset が required を持たないとき(stacked PR)は、既定ブランチの ruleset の
/// required を E に使う: CI は base を問わず同名のチェックを報告する(docs/agents/pr-gate.md
/// 「ruleset のスコープと CI の報告は別軸」)ので、required の全緑で OK を判定できる。
/// 既定ブランチ側の取得に失敗したら、従来どおり quiesce に縮退する(base 側の API 失敗は
/// これまでどおり `Err` で、縮退させない — #135)。
pub fn resolve_expected(
    nwo: &str,
    base: &str,
    default_base: Option<&str>,
) -> Result<(Vec<Value>, String), JqError> {
    let e = expected_contexts(nwo, base)?;
    if !e.is_empty() {
        return Ok((e, String::new()));
    }
    if let Some(d) = default_base.filter(|d| !d.is_empty() && *d != base) {
        if let Ok(de) = expected_contexts(nwo, d) {
            if !de.is_empty() {
                return Ok((
                    de,
                    format!(
                        "base {base} は ruleset の対象外。既定ブランチ {d} の required で判定した"
                    ),
                ));
            }
        }
    }
    Ok((
        e,
        format!("base {base} は ruleset の対象外。報告された全チェックで判定した"),
    ))
}

/// 待ちの結果。`reported` は最後に取り直した報告、`expected` は判定に使った期待集合
/// (どちらも整形・内訳に使う。E が空なら quiesce で判定した)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waited {
    pub outcome: Outcome,
    pub reported: String,
    pub expected: Vec<Value>,
}

/// Stop の G_CI(`PR_GATE_CI_TIMEOUT` まで待つ)。
pub fn run(
    cfg: &Config,
    nwo: &str,
    pr_num: &str,
    base: &str,
    default_base: Option<&str>,
) -> Outcome {
    wait(cfg, nwo, pr_num, base, default_base, cfg.ci_timeout).outcome
}

/// `limit` まで `--json` を取り直して判定する。赤 1 件で即 `Failed`、E が全緑で即 `Pass`。
/// E が空のときは、報告件数が `QUIESCE` 秒増えなくなる(または出現待ちの上限に達する)のを
/// 「揃った」とみなし、pending が 0 になったら `Pass`。
pub fn wait(
    cfg: &Config,
    nwo: &str,
    pr_num: &str,
    base: &str,
    default_base: Option<&str>,
    limit: Duration,
) -> Waited {
    let Ok((expected, note)) = resolve_expected(nwo, base, default_base) else {
        return Waited {
            outcome: Outcome::of(Status::ApiFailure),
            reported: String::new(),
            expected: Vec::new(),
        };
    };
    let done = |outcome: Outcome, reported: String| Waited {
        outcome,
        reported,
        expected: expected.clone(),
    };
    let required = !expected.is_empty();
    let start = Instant::now();
    let appear_deadline = start + Duration::from_secs(cfg.check_appear_timeout);
    let deadline = start + limit;
    let quiesce = Duration::from_secs(cfg.quiesce);
    let mut prev: Option<usize> = None;
    let mut stable_since: Option<Instant> = None;

    loop {
        let reported = gh::reported_checks(pr_num, nwo);
        let t = Instant::now();
        match decide(&expected, &reported) {
            Verdict::Failed => return done(Outcome::of(Status::Failed), reported),
            Verdict::Pass => {
                let o = Outcome {
                    status: Status::Pass,
                    detail: note,
                };
                return done(o, reported);
            }
            Verdict::Wait {
                missing,
                pending,
                reported: n,
            } => {
                let timed_out = t >= deadline;
                if required {
                    // required の未出現は、他のチェックが pending の間は出現待ちの上限で
                    // 打ち切らない: 集約ジョブ(`CI passed`)は `needs` が終わるまで
                    // チェックとして現れない。pending が 0 のまま現れないとき(workflow が
                    // 起動していない)だけ、出現待ちの上限で Missing にする。
                    let appear_expired = t >= appear_deadline && pending == 0;
                    if !missing.is_empty() && (appear_expired || timed_out) {
                        return done(
                            Outcome {
                                status: Status::Missing,
                                detail: jqv::join(&missing, ", "),
                            },
                            reported,
                        );
                    }
                } else {
                    if prev == Some(n) && n > 0 {
                        stable_since.get_or_insert(t);
                    } else {
                        stable_since = None;
                    }
                    prev = Some(n);
                    let quiesced = n > 0
                        && (t >= appear_deadline
                            || stable_since.is_some_and(|s| t.duration_since(s) >= quiesce));
                    if quiesced && pending == 0 {
                        return done(
                            Outcome {
                                status: Status::Pass,
                                detail: note,
                            },
                            reported,
                        );
                    }
                    if n == 0 && (t >= appear_deadline || timed_out) {
                        return done(Outcome::of(Status::Empty), reported);
                    }
                }
                if timed_out {
                    return done(Outcome::of(Status::Pending), reported);
                }
            }
        }
        std::thread::sleep(cfg.poll.min(deadline.saturating_duration_since(t)));
    }
}

/// 報告のうち pending のチェック名(整形用)。`only` が非空ならその名前のものだけ。
pub fn pending_names(reported: &str, only: &[Value]) -> Vec<String> {
    let Some(r) = parse_reported(reported) else {
        return Vec::new();
    };
    r.iter()
        .filter(|x| bucket_is(x, &["pending"]).unwrap_or(false))
        .filter_map(|x| jqv::field(x, "name").ok())
        .filter(|n| only.is_empty() || only.iter().any(|e| jqv::eq(e, n)))
        .map(|n| jqv::raw(&n))
        .collect()
}

/// 報告の件数(読めなければ 0)。
pub fn reported_count(reported: &str) -> usize {
    parse_reported(reported).map_or(0, |r| r.len())
}

static RUN_JOB_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^.*/runs/([0-9]+)/job/([0-9]+)").expect("RUN_JOB_RE"));

/// 失敗/取消チェックをジョブ名 + link + gh run view コマンドで整形する
/// (末尾の改行は落とす — bash 版の `$(render_failed_checks …)`)。
pub fn render_failed(reported: &str) -> String {
    let mut out = String::new();
    let Some(r) = parse_reported(reported) else {
        return out;
    };
    for x in &r {
        match bucket_is(x, &["fail", "cancel"]) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(JqError) => return String::new(),
        }
        let name = jqv::field(x, "name")
            .map(|v| jqv::raw(&v))
            .unwrap_or_default();
        let link = jqv::field(x, "link")
            .map(|v| jqv::raw(&v))
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push_str(&format!("失敗: {name}\n  {link}\n"));
        if let Some(c) = RUN_JOB_RE.captures(&link) {
            out.push_str(&format!(
                "\nログ:\n  gh run view {} --log-failed --job {}\n",
                &c[1], &c[2]
            ));
        }
    }
    crate::trim_nl(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expected_forms() {
        let raw = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"job-a"},{"context":"job-b"}]}},{"type":"other"}]"#;
        assert_eq!(
            parse_expected(raw).unwrap(),
            vec![json!("job-a"), json!("job-b")]
        );
        assert_eq!(parse_expected("[]").unwrap(), Vec::<Value>::new());
        assert_eq!(parse_expected("").unwrap(), Vec::<Value>::new());
        assert!(parse_expected("{").is_err());
    }

    fn e2() -> Vec<Value> {
        vec![json!("job-a"), json!("job-b")]
    }

    fn wait_of(v: Verdict) -> (Vec<Value>, usize, usize) {
        match v {
            Verdict::Wait {
                missing,
                pending,
                reported,
            } => (missing, pending, reported),
            other => panic!("Wait を期待したが {other:?}"),
        }
    }

    #[test]
    fn any_red_is_failed_even_if_not_required() {
        // NG は報告された全チェックで見る(E に入っていない x でも即時)。
        let r = r#"[{"name":"job-a","bucket":"pending"},{"name":"x","bucket":"fail"}]"#;
        assert_eq!(decide(&e2(), r), Verdict::Failed);
        // cancel だけが残って他が落ち着いたなら NG。
        let r = r#"[{"name":"x","bucket":"cancel"}]"#;
        assert_eq!(decide(&[], r), Verdict::Failed);
    }

    #[test]
    fn stale_run_results_are_ignored() {
        // 古い run(100)の CI passed は fail、新しい run(200)にはまだ CI passed が無い。
        let e = vec![json!("job-a"), json!("CI passed")];
        let r = r#"[
          {"name":"CI passed","bucket":"fail","link":"https://x/runs/100/job/1","workflow":"CI"},
          {"name":"Build ${{ matrix.host }}","bucket":"cancel","link":"https://x/runs/100/job/2","workflow":"CI"},
          {"name":"job-a","bucket":"pending","link":"https://x/runs/200/job/3","workflow":"CI"}]"#;
        let (missing, pending, n) = wait_of(decide(&e, r));
        assert_eq!((missing, pending, n), (vec![json!("CI passed")], 1, 1));
        // 新しい run の CI passed が出れば、それで判定する。
        let r = r#"[
          {"name":"CI passed","bucket":"fail","link":"https://x/runs/100/job/1","workflow":"CI"},
          {"name":"CI passed","bucket":"pass","link":"https://x/runs/200/job/4","workflow":"CI"},
          {"name":"job-a","bucket":"pass","link":"https://x/runs/200/job/3","workflow":"CI"}]"#;
        assert_eq!(decide(&e, r), Verdict::Pass);
        // 別の workflow の古い run は、その workflow の最新があるときだけ除く(workflow ごと)。
        let r = r#"[
          {"name":"x","bucket":"fail","link":"https://x/runs/5/job/1","workflow":"Darwin"},
          {"name":"job-a","bucket":"pass","link":"https://x/runs/200/job/3","workflow":"CI"}]"#;
        assert_eq!(decide(&[json!("job-a")], r), Verdict::Failed);
        // run ID の無いチェック(外部の status)は残す。
        let r = r#"[{"name":"ext","bucket":"fail","link":"https://ci.example/b/1","workflow":""}]"#;
        assert_eq!(decide(&[], r), Verdict::Failed);
    }

    #[test]
    fn cancel_is_not_an_immediate_ng() {
        // 後続の run に置き換えられた cancel(#1157 で実際に起きた)で、即 NG にしない。
        let r = r#"[{"name":"job-a","bucket":"cancel"},{"name":"job-b","bucket":"pending"}]"#;
        let (_, pending, _) = wait_of(decide(&e2(), r));
        assert_eq!(pending, 1);
        // E が全緑なら、E 外の cancel があっても OK。
        let r = r#"[{"name":"job-a","bucket":"pass"},{"name":"job-b","bucket":"pass"},{"name":"x","bucket":"cancel"}]"#;
        assert_eq!(decide(&e2(), r), Verdict::Pass);
        // E の cancel が残り、pending が 0 なら NG。
        let r = r#"[{"name":"job-a","bucket":"pass"},{"name":"job-b","bucket":"cancel"}]"#;
        assert_eq!(decide(&e2(), r), Verdict::Failed);
    }

    #[test]
    fn required_all_green_passes_without_waiting_for_others() {
        // OK は E の全件が緑になった時点。E 外の pending(Build altair 等)は待たない。
        let r = r#"[{"name":"job-a","bucket":"pass"},{"name":"job-b","bucket":"skipping"},{"name":"altair","bucket":"pending"}]"#;
        assert_eq!(decide(&e2(), r), Verdict::Pass);
    }

    #[test]
    fn partial_set_is_never_pass() {
        // cli/cli#9973: 出現した部分集合が全緑でも、E ⊆ 報告 でなければ Pass にしない。
        let r = r#"[{"name":"job-a","bucket":"pass"}]"#;
        let (missing, pending, n) = wait_of(decide(&e2(), r));
        assert_eq!((missing, pending, n), (vec![json!("job-b")], 0, 1));
        // 全件出現していても required が pending なら続行。
        let r = r#"[{"name":"job-a","bucket":"pass"},{"name":"job-b","bucket":"pending"}]"#;
        let (missing, pending, _) = wait_of(decide(&e2(), r));
        assert_eq!((missing.len(), pending), (0, 1));
    }

    #[test]
    fn empty_expected_never_passes_here() {
        // E が空のときの「揃った」は静止で近似する(状態を持つ wait の仕事)。
        let r = r#"[{"name":"x","bucket":"pass"},{"name":"y","bucket":"pending"}]"#;
        let (missing, pending, n) = wait_of(decide(&[], r));
        assert_eq!((missing.len(), pending, n), (0, 1, 2));
        assert_eq!(wait_of(decide(&[], "[]")).2, 0);
    }

    #[test]
    fn unreadable_report_is_not_green() {
        for bad in ["not json", "{}", r#"[1]"#, r#"[{"name":"job-a"}]"#] {
            assert!(matches!(decide(&e2(), bad), Verdict::Wait { .. }), "{bad}");
        }
    }

    #[test]
    fn pending_names_and_count() {
        let r = r#"[{"name":"a","bucket":"pass"},{"name":"b","bucket":"pending"}]"#;
        assert_eq!(pending_names(r, &[]), vec!["b".to_string()]);
        assert!(pending_names(r, &[json!("a")]).is_empty());
        assert_eq!(reported_count(r), 2);
        assert_eq!(reported_count("x"), 0);
    }

    #[test]
    fn render_failed_lines() {
        let r = r#"[{"name":"job-b","bucket":"fail","link":"https://x/runs/1/job/12"}]"#;
        assert_eq!(
            render_failed(r),
            "失敗: job-b\n  https://x/runs/1/job/12\n\nログ:\n  gh run view 1 --log-failed --job 12"
        );
    }
}
