//! G_CI: 期待集合の取得と判定。
//!
//! 中心不変条件は「揃っていない集合を緑と読まないこと」。次の 3 経路で
//! `gh pr checks` は「チェックが無い/揃っていない」を「exit 0」で返す。どの経路でも
//! 判定をその exit code に置かない — 期待集合をサーバから取り、判定は
//! [`judge`] が `--json` の取り直しに対して行う:
//!   1. push 直後で check run がまだ API に現れていない (cli/cli#7401)
//!   2. ruleset の対象外(base が main 以外の stacked PR)で required が 0 件
//!   3. 6 件のうち一部だけが現れ、その部分集合が pass した時点で
//!      `--watch` が早期終了する (cli/cli#9973) — これが一番危ない。
//!      「1 件以上現れたら待機開始」では防げないので、`--watch` は待つための
//!      道具に格下げし、その exit code を acceptance criterion にしない。

use crate::config::Config;
use crate::jqv::JqError;
use crate::{gh, jqv};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// 報告集合(`gh pr checks --json`)。配列として読めなければ `None`。
fn parse_reported(s: &str) -> Option<Vec<Value>> {
    match serde_json::from_str::<Value>(s).ok()? {
        Value::Array(a) => Some(a),
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

/// 判定材料の集計(bash の `judged` の jq 式)。
#[derive(Debug, Default, PartialEq)]
pub struct Judged {
    pub n: usize,
    pub missing: Vec<Value>,
    pub failed: usize,
    pub pending: usize,
}

/// 期待集合が非空なら報告のうち期待に含まれるものだけ、空なら報告すべてを対象に、
/// 未出現・失敗(fail/cancel)・pending を数える。jq がエラーになる形
/// (配列でない・要素がオブジェクトでない)は全ゼロに倒す(bash 版と同じ)。
pub fn judge(expected: &[Value], reported: &str) -> Judged {
    let inner = || -> Result<Judged, JqError> {
        let r = parse_reported(reported).ok_or(JqError)?;
        let rel: Vec<Value> = if expected.is_empty() {
            r
        } else {
            let mut rel = Vec::new();
            for x in r {
                let n = jqv::field(&x, "name")?;
                if expected.iter().any(|e| jqv::eq(e, &n)) {
                    rel.push(x);
                }
            }
            rel
        };
        let mut failed = 0;
        let mut pending = 0;
        for x in &rel {
            if bucket_is(x, &["fail", "cancel"])? {
                failed += 1;
            }
            if bucket_is(x, &["pending"])? {
                pending += 1;
            }
        }
        Ok(Judged {
            n: rel.len(),
            missing: jqv::subtract(expected, &names(&rel)?),
            failed,
            pending,
        })
    };
    inner().unwrap_or_default()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sleep5() {
    std::thread::sleep(Duration::from_secs(5));
}

/// G_CI の判定本体(bash の `run_g_ci`)。
pub fn run(cfg: &Config, nwo: &str, pr_num: &str, base: &str) -> Outcome {
    let Ok(expected) = expected_contexts(nwo, base) else {
        return Outcome::of(Status::ApiFailure);
    };
    let required = !expected.is_empty();
    let note = if required {
        String::new()
    } else {
        format!("base {base} は ruleset の対象外。報告された全チェックで判定した")
    };
    let deadline = now() + cfg.check_appear_timeout;

    if required {
        // 出現待ち: 期待集合 E が報告集合 R に完全に含まれるまで待つ(cli#7401 / #9973 対策)。
        loop {
            let reported = gh::reported_checks(pr_num, nwo);
            let names_json = parse_reported(&reported)
                .and_then(|r| names(&r).ok())
                .unwrap_or_default();
            let missing = jqv::subtract(&expected, &names_json);
            if missing.is_empty() {
                break;
            }
            if now() >= deadline {
                return Outcome {
                    status: Status::Missing,
                    detail: jqv::join(&missing, ", "),
                };
            }
            sleep5();
        }
    } else {
        // quiescence: E が定義できない(stacked PR 等)ので、報告件数が
        // QUIESCE 秒増えなくなるまで待って「揃った」とみなす。
        let mut prev: i64 = -1;
        let mut stable_since: Option<u64> = None;
        loop {
            let reported = gh::reported_checks(pr_num, nwo);
            let cur = serde_json::from_str::<Value>(&reported)
                .ok()
                .and_then(|v| jqv::length(&v).ok())
                .unwrap_or(0) as i64;
            let t = now();
            if cur > 0 && cur == prev {
                let since = *stable_since.get_or_insert(t);
                if t - since >= cfg.quiesce {
                    break;
                }
            } else {
                stable_since = None;
            }
            prev = cur;
            if t >= deadline {
                if cur == 0 {
                    return Outcome::of(Status::Empty);
                }
                break; // 部分的でも上限に達したら今の集合で判定に進む(下の判定が最終防御)
            }
            sleep5();
        }
    }

    // terminal state まで待つだけ。exit code は見ない(cli/cli#9973)。
    gh::watch_checks(pr_num, nwo, required, cfg.ci_timeout);

    // 判定は --json を取り直して行う。--watch の exit code は使わない。
    let j = judge(&expected, &gh::reported_checks(pr_num, nwo));
    if !required && j.n == 0 {
        return Outcome::of(Status::Empty);
    }
    if !j.missing.is_empty() {
        return Outcome {
            status: Status::Missing,
            detail: jqv::join(&j.missing, ", "),
        };
    }
    if j.failed > 0 {
        return Outcome::of(Status::Failed);
    }
    if j.pending > 0 {
        return Outcome::of(Status::Pending);
    }
    Outcome {
        status: Status::Pass,
        detail: note,
    }
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

    #[test]
    fn judge_counts() {
        let e = vec![json!("job-a"), json!("job-b")];
        let r = r#"[{"name":"job-a","bucket":"pass"},{"name":"job-b","bucket":"pending"},{"name":"x","bucket":"fail"}]"#;
        let j = judge(&e, r);
        assert_eq!((j.n, j.failed, j.pending), (2, 0, 1));
        assert!(j.missing.is_empty());
        assert_eq!(judge(&[], "not json"), Judged::default());
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
