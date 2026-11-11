//! 確認の台帳(ADR-1529 の置き場 6)。機体ごとに常設の Issue `verify-ledger: <host>` を 1 件持ち、マージ後の
//! 実機確認を本文の 1 行ずつで持つ。SessionStart で、その機体の台帳から「いま着手できる項目」を選んで注入し、
//! そのセッションのエージェントが `machine` の項目を自分で実行する(docs/agents/issue-index.md「確認の台帳」)。
//!
//! 項目の形(閉じた 1 行):
//!
//! ```text
//! - [ ] after:#<PR> <machine|human> — <何を確かめるか> — check: `<コマンド>`
//! ```
//!
//! 着手できるかは、`after:#<PR>` の PR がマージ済みで、その merge commit がこの機体に適用した rev
//! (`~/.local/state/hms/last-apply.json` の `rev`)の祖先であることで決める。判定は純粋関数で、`gh`・`git` の
//! 呼び出しは `main.rs` が持つ。

/// 台帳の Issue の題の接頭辞。`promotion-detect` の `issue_flow::LEDGER_TITLE_PREFIX` と同じ値。
pub const LEDGER_TITLE_PREFIX: &str = "verify-ledger:";

/// 機体 `host` の台帳の題。
pub fn ledger_title(host: &str) -> String {
    format!("{LEDGER_TITLE_PREFIX} {host}")
}

/// 項目の種類。`machine` はエージェントがコマンドで確かめ、`human` は本人に頼む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Machine,
    Human,
}

/// 未了の項目 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `after:#<PR>` の PR 番号。
    pub pr: u64,
    pub kind: Kind,
    /// 種類のあとの残り(何を確かめるか・check のコマンド)。
    pub rest: String,
}

impl Item {
    /// 注入する 1 行(`after:#N machine — …`)。
    pub fn line(&self) -> String {
        let kind = match self.kind {
            Kind::Machine => "machine",
            Kind::Human => "human",
        };
        format!("after:#{} {kind} — {}", self.pr, self.rest)
    }
}

/// 台帳の本文を読んだもの。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub open: Vec<Item>,
    /// `- [ ] ` で始まるが、項目の形に合わない行(形を直す対象)。
    pub malformed: Vec<String>,
    /// 済んだ項目(`- [x] `)の数。
    pub done: usize,
}

/// 本文を 1 行ずつ読む。未了の項目は `- [ ] `、済んだ項目は `- [x] ` で始まる行だけを数え、ほかの行は無視する。
pub fn parse(body: &str) -> Parsed {
    let mut p = Parsed::default();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("- [x] ") || t.starts_with("- [X] ") {
            p.done += 1;
        } else if let Some(rest) = t.strip_prefix("- [ ] ") {
            match parse_item(rest) {
                Some(item) => p.open.push(item),
                None => p.malformed.push(rest.trim().to_string()),
            }
        }
    }
    p
}

fn parse_item(rest: &str) -> Option<Item> {
    let rest = rest.trim();
    let (after, rest) = rest.split_once(char::is_whitespace)?;
    let pr: u64 = after.strip_prefix("after:#")?.parse().ok()?;
    let rest = rest.trim_start();
    let (kind, rest) = match rest.split_once(char::is_whitespace) {
        Some((k, r)) => (k, r),
        None => (rest, ""),
    };
    let kind = match kind {
        "machine" => Kind::Machine,
        "human" => Kind::Human,
        _ => return None,
    };
    let rest = rest.trim_start().trim_start_matches(['—', '-']).trim();
    if rest.is_empty() {
        return None;
    }
    Some(Item {
        pr,
        kind,
        rest: rest.to_string(),
    })
}

/// `after:#<PR>` の PR の状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    /// マージ済み。値は merge commit の oid。
    Merged(String),
    /// 未マージ(open か、マージせずに閉じた)。
    NotMerged,
}

/// 項目の着手の可否。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// PR がマージ済みで、この機体に適用済み。
    Ready,
    /// PR はマージ済みだが、この機体に適用したかを確かめられない(適用の記録が無い・この repo の commit でない)。
    ReadyUnverified,
    /// PR はマージ済みだが、この機体にはまだ適用していない(`hms` で当てると着手できる)。
    NotApplied,
    /// PR が未マージ。
    PrNotMerged,
    /// PR の状態を読めなかった。
    Unknown,
}

/// 1 項目の可否。`applied(oid)` は「merge commit がこの機体の適用 rev の祖先か」で、確かめられなければ `None`。
pub fn readiness(pr: Option<&PrState>, applied: impl Fn(&str) -> Option<bool>) -> Readiness {
    match pr {
        None => Readiness::Unknown,
        Some(PrState::NotMerged) => Readiness::PrNotMerged,
        Some(PrState::Merged(oid)) => match applied(oid) {
            Some(true) => Readiness::Ready,
            Some(false) => Readiness::NotApplied,
            None => Readiness::ReadyUnverified,
        },
    }
}

/// SessionStart に足す節。未了の項目が 1 件も無ければ `None`。
///
/// `status` は `parsed.open` と同じ順の可否。
pub fn section(number: u64, host: &str, parsed: &Parsed, status: &[Readiness]) -> Option<String> {
    if parsed.open.is_empty() && parsed.malformed.is_empty() {
        return None;
    }
    let pick = |want: Readiness| -> Vec<&Item> {
        parsed
            .open
            .iter()
            .zip(status)
            .filter(|(_, s)| **s == want)
            .map(|(i, _)| i)
            .collect()
    };
    let bullets = |items: &[&Item]| -> String {
        items
            .iter()
            .map(|i| format!("- {}", sanitize_line(&i.line())))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let numbers = |items: &[&Item]| -> String {
        let mut prs: Vec<u64> = items.iter().map(|i| i.pr).collect();
        prs.sort_unstable();
        prs.dedup();
        prs.iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let ready = pick(Readiness::Ready);
    let unverified = pick(Readiness::ReadyUnverified);
    let not_applied = pick(Readiness::NotApplied);
    let not_merged = pick(Readiness::PrNotMerged);
    let unknown = pick(Readiness::Unknown);

    let mut s = format!(
        "確認の台帳 #{number}({}。この機体のマージ後の実機確認): 未了 {} 項目、済み {} 項目。",
        ledger_title(host),
        parsed.open.len(),
        parsed.done
    );
    if !ready.is_empty() {
        s.push_str("\n着手できる(PR がマージ済みで、この機体に適用済み):\n");
        s.push_str(&bullets(&ready));
    }
    if !unverified.is_empty() {
        s.push_str(
            "\n着手できる見込み(PR はマージ済み。この機体の適用の記録からは確かめられない):\n",
        );
        s.push_str(&bullets(&unverified));
    }
    let mut waits = Vec::new();
    if !not_applied.is_empty() {
        waits.push(format!(
            "この機体に未適用 {} 項目({}。`hms` で当てると着手できる)",
            not_applied.len(),
            numbers(&not_applied)
        ));
    }
    if !not_merged.is_empty() {
        waits.push(format!(
            "PR が未マージ {} 項目({})",
            not_merged.len(),
            numbers(&not_merged)
        ));
    }
    if !unknown.is_empty() {
        waits.push(format!(
            "PR の状態を読めない {} 項目({})",
            unknown.len(),
            numbers(&unknown)
        ));
    }
    if !waits.is_empty() {
        s.push_str("\n待ち: ");
        s.push_str(&waits.join("、"));
        s.push('。');
    }
    if !parsed.malformed.is_empty() {
        s.push_str(&format!(
            "\n形式外の行 {} 件(`- [ ] after:#<PR> <machine|human> — <何を確かめるか> — check: `<コマンド>`` の形に直す)。",
            parsed.malformed.len()
        ));
    }
    if !ready.is_empty() || !unverified.is_empty() {
        s.push_str(
            "\n着手できる `machine` の項目は、このセッションでエージェントが check のコマンドを実行し、終了コードと出力の要点を\
             その行の末尾に書いて `- [x]` にする(`gh issue edit --body-file`。コメントでなく本文を直す)。`human` の項目は、\
             本人に 1 つずつ頼む(共有 AGENTS.md「人に依頼する手順は…」)。",
        );
    }
    Some(s)
}

/// 項目の行の上限(コードポイント)。check のコマンドを含むので、索引の題名(120)より長く取る。
pub const LINE_MAX: usize = 300;

/// 制御文字を除き、[`LINE_MAX`] で切る(本文は第三者が書き得るので、注入の前に無害化する)。
fn sanitize_line(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(LINE_MAX)
        .collect()
}

/// `last-apply.json`(hms の適用の記録)から `rev` を読む。読めない・null なら `None`。
pub fn applied_rev(last_apply_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(last_apply_json).ok()?;
    v.get("rev")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// GraphQL の応答(`p<N>: pullRequest(number: N) { merged mergeCommit { oid } }` の並び)から、PR ごとの状態を読む。
pub fn pr_states(graphql: &str) -> std::collections::HashMap<u64, PrState> {
    let mut out = std::collections::HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(graphql) else {
        return out;
    };
    let Some(repo) = v
        .get("data")
        .and_then(|d| d.get("repository"))
        .and_then(serde_json::Value::as_object)
    else {
        return out;
    };
    for (key, pr) in repo {
        let Some(n) = key.strip_prefix('p').and_then(|n| n.parse::<u64>().ok()) else {
            continue;
        };
        let merged = pr.get("merged").and_then(serde_json::Value::as_bool);
        let oid = pr
            .get("mergeCommit")
            .and_then(|m| m.get("oid"))
            .and_then(serde_json::Value::as_str);
        match (merged, oid) {
            (Some(true), Some(oid)) => {
                out.insert(n, PrState::Merged(oid.to_string()));
            }
            (Some(false), _) => {
                out.insert(n, PrState::NotMerged);
            }
            _ => {}
        }
    }
    out
}

/// PR の状態を 1 回の GraphQL で読むための問い合わせ。
pub fn pr_states_query(owner: &str, name: &str, prs: &[u64]) -> String {
    let fields: Vec<String> = prs
        .iter()
        .map(|n| format!("p{n}: pullRequest(number: {n}) {{ merged mergeCommit {{ oid }} }}"))
        .collect();
    format!(
        "query {{ repository(owner: \"{owner}\", name: \"{name}\") {{ {} }} }}",
        fields.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "\
## 台帳

- [ ] after:#1404 machine — pkill -f の deny を読む — check: `pkill-guard --check 'pkill -f x'`
- [x] after:#1316 machine — external-send-guard の reply を読む — 結果: exit 1(2026-11-01、vega)
- [ ] after:#1469 human — Stop の block の表示を見る
- [ ] 形の違う行
- [ ] after:#12 robot — 種類が違う
- [ ] after:#13 machine
本文の地の文は数えない
";

    #[test]
    fn parse_reads_open_done_and_malformed_lines() {
        let p = parse(BODY);
        assert_eq!(p.done, 1);
        assert_eq!(
            p.open,
            vec![
                Item {
                    pr: 1404,
                    kind: Kind::Machine,
                    rest: "pkill -f の deny を読む — check: `pkill-guard --check 'pkill -f x'`"
                        .to_string()
                },
                Item {
                    pr: 1469,
                    kind: Kind::Human,
                    rest: "Stop の block の表示を見る".to_string()
                },
            ]
        );
        assert_eq!(
            p.malformed,
            vec![
                "形の違う行".to_string(),
                "after:#12 robot — 種類が違う".to_string(),
                "after:#13 machine".to_string()
            ]
        );
    }

    #[test]
    fn readiness_follows_merge_then_application() {
        let merged = PrState::Merged("abc".to_string());
        assert_eq!(readiness(None, |_| Some(true)), Readiness::Unknown);
        assert_eq!(
            readiness(Some(&PrState::NotMerged), |_| Some(true)),
            Readiness::PrNotMerged
        );
        assert_eq!(readiness(Some(&merged), |_| Some(true)), Readiness::Ready);
        assert_eq!(
            readiness(Some(&merged), |_| Some(false)),
            Readiness::NotApplied
        );
        assert_eq!(
            readiness(Some(&merged), |_| None),
            Readiness::ReadyUnverified
        );
        // 適用の判定には merge commit の oid を渡す。
        assert_eq!(
            readiness(Some(&merged), |oid| Some(oid == "abc")),
            Readiness::Ready
        );
    }

    #[test]
    fn section_groups_items_and_explains_what_to_do() {
        let p = parse(BODY);
        let s = section(
            1600,
            "vega",
            &p,
            &[Readiness::Ready, Readiness::PrNotMerged],
        )
        .unwrap();
        assert!(
            s.starts_with("確認の台帳 #1600(verify-ledger: vega。"),
            "{s}"
        );
        assert!(s.contains("未了 2 項目、済み 1 項目"), "{s}");
        assert!(s.contains("着手できる(PR がマージ済みで、この機体に適用済み):\n- after:#1404 machine — pkill -f"), "{s}");
        assert!(s.contains("PR が未マージ 1 項目(#1469)"), "{s}");
        assert!(s.contains("形式外の行 3 件"), "{s}");
        assert!(s.contains("`- [x]` にする"), "{s}");
    }

    #[test]
    fn section_without_ready_items_omits_the_instruction() {
        let p = parse("- [ ] after:#1 machine — 読む\n");
        let s = section(1, "altair", &p, &[Readiness::NotApplied]).unwrap();
        assert!(
            s.contains("この機体に未適用 1 項目(#1。`hms` で当てると着手できる)"),
            "{s}"
        );
        assert!(!s.contains("`- [x]` にする"), "{s}");
    }

    #[test]
    fn section_is_none_when_nothing_is_open() {
        let p = parse("- [x] after:#1 machine — 済み\n");
        assert_eq!(section(1, "vega", &p, &[]), None);
    }

    #[test]
    fn applied_rev_reads_the_hms_record() {
        let rec = "{\n  \"mode\": \"default\",\n  \"rev\": \"deadbeef\",\n  \"generation\": \"/nix/store/x\",\n  \"applied_at\": 1\n}\n";
        assert_eq!(applied_rev(rec).as_deref(), Some("deadbeef"));
        assert_eq!(applied_rev("{\"rev\": null}"), None);
        assert_eq!(applied_rev("壊れた"), None);
    }

    #[test]
    fn pr_states_reads_aliased_pull_requests() {
        let raw = r#"{"data":{"repository":{
            "p1404":{"merged":true,"mergeCommit":{"oid":"aaa"}},
            "p1469":{"merged":false,"mergeCommit":null},
            "p9":null,
            "other":{"merged":true,"mergeCommit":{"oid":"bbb"}}
        }}}"#;
        let m = pr_states(raw);
        assert_eq!(m.get(&1404), Some(&PrState::Merged("aaa".to_string())));
        assert_eq!(m.get(&1469), Some(&PrState::NotMerged));
        assert_eq!(m.get(&9), None);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn pr_states_query_aliases_each_number() {
        let q = pr_states_query("o", "r", &[1, 22]);
        assert!(q.contains("repository(owner: \"o\", name: \"r\")"), "{q}");
        assert!(
            q.contains("p1: pullRequest(number: 1) { merged mergeCommit { oid } }"),
            "{q}"
        );
        assert!(q.contains("p22: pullRequest(number: 22)"), "{q}");
    }

    #[test]
    fn ledger_title_matches_the_prefix() {
        assert_eq!(ledger_title("altair"), "verify-ledger: altair");
        assert!(ledger_title("vega").starts_with(LEDGER_TITLE_PREFIX));
    }
}
