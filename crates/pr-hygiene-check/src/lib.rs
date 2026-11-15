//! pr-hygiene-check — 自分の open PR の「汚れ」を、決定論で見つける(設計は docs/agents/pr-hygiene-check.md)。
//!
//! PR の履歴や mergeable の構造は、時間とともに汚れる。main が進んで衝突する、積み上げた PR の下の段が squash で
//! マージされて、上の段に済んだコミットが残る(ADR-0027 の「汚染 diff」)、base のブランチが消える、merge commit が
//! 混ざる、必須の最新化に遅れる。直す手順は `stacked-pr` スキル §4 の積み直しで、この道具は**見つけるだけ**
//! (履歴の改変は、人が起動した入口が行う。docs/agents/pr-gate.md「G_base / G_worktree を block にしない理由」)。
//!
//! 母集団は、この機体の側と同じ側の持ち主だけで作る(`repo_scope::Policy::same_side_owners`)。別の側の持ち主には、
//! 拒否に頼らず、最初から問い合わせない(docs/agents/repo-scope-guard.md)。検索の結果も、持ち主で絞り直す。
//!
//! 分類(閉じた語彙。1 つの PR が複数を持ちうる):
//!
//! | 分類 | 意味 | 見つけ方 |
//! |---|---|---|
//! | `conflict` | base と衝突している(CI も回らない) | `mergeable = CONFLICTING` |
//! | `merged-ancestor` | マージ済みの別の PR に属するコミットを含む(下の段が squash でマージされた後など) | コミットの `associatedPullRequests` に、別の `merged` な PR |
//! | `base-gone` | base のブランチが無い | `baseRef = null` |
//! | `merge-commit` | 親が 2 つ以上のコミットがある(rebase で一直線にする規則) | `parents.totalCount > 1` |
//! | `behind` | base に遅れていて、最新化が要る | `mergeStateStatus = BEHIND` |
//! | `unknown` | 判定できなかった(GitHub が mergeable を計算中のまま、コミットが多すぎる、問い合わせの失敗) | — |
//!
//! 積み上げた PR(同じリポジトリで、ある PR の base が別の PR の head)は、下の段が汚れていれば、上の段に
//! `below` を付ける(根の原因の PR の番号)。出力は、下の段から直す順(リポジトリ、段の深さ、番号)。
//!
//! 環境に触れる部分(`gh`・待ち時間)は注入できる形で持つ。

use repo_scope::split_nwo;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

/// 汚れの分類。閉じた語彙。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Conflict,
    MergedAncestor,
    BaseGone,
    MergeCommit,
    Behind,
    Unknown,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Conflict => "conflict",
            Class::MergedAncestor => "merged-ancestor",
            Class::BaseGone => "base-gone",
            Class::MergeCommit => "merge-commit",
            Class::Behind => "behind",
            Class::Unknown => "unknown",
        }
    }
}

/// 検索で見つかった PR。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub updated_at: String,
    pub is_draft: bool,
}

/// GraphQL で読んだ、構造の詳細。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub base: String,
    pub head: String,
    pub head_oid: String,
    pub base_exists: bool,
    pub mergeable: String,
    pub merge_state: String,
    pub commits_total: u64,
    pub commits_seen: u64,
    pub merge_commits: u64,
    /// マージ済みの別の PR の番号(昇順、重複なし)。
    pub merged_ancestors: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pr {
    pub found: Found,
    /// 詳細を読めなかった(問い合わせの失敗)ときは `None`。
    pub details: Option<Details>,
}

impl Pr {
    fn nwo(&self) -> String {
        format!("{}/{}", self.found.owner, self.found.repo)
    }
}

/// 1 つの PR の分類(空なら、構造は clean)。
pub fn classify(pr: &Pr) -> Vec<Class> {
    let Some(d) = &pr.details else {
        return vec![Class::Unknown];
    };
    let mut v = Vec::new();
    if d.mergeable == "CONFLICTING" {
        v.push(Class::Conflict);
    }
    if !d.merged_ancestors.is_empty() {
        v.push(Class::MergedAncestor);
    }
    if !d.base_exists {
        v.push(Class::BaseGone);
    }
    if d.merge_commits > 0 {
        v.push(Class::MergeCommit);
    }
    if d.merge_state == "BEHIND" {
        v.push(Class::Behind);
    }
    if d.mergeable == "UNKNOWN" || d.commits_total > d.commits_seen {
        v.push(Class::Unknown);
    }
    v
}

/// 分類つきの PR。積み上げの位置を持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub pr: Pr,
    pub classes: Vec<Class>,
    /// 積み上げで、下の段にある、汚れた根の PR の番号(無ければ空)。
    pub below: Vec<u64>,
    /// 積み上げの深さ(0 = 最下段、または積まれていない)。
    pub depth: usize,
}

impl Entry {
    /// 直す必要がある(自分が汚れているか、下の段が汚れている)。
    pub fn is_dirty(&self) -> bool {
        !self.classes.is_empty() || !self.below.is_empty()
    }

    /// 確かな汚れ(`unknown` だけのものを除く)。
    fn is_definitely_dirty(&self) -> bool {
        !self.below.is_empty() || self.classes.iter().any(|c| *c != Class::Unknown)
    }
}

/// 分類し、積み上げの位置を付けて、下の段から直す順(リポジトリ、深さ、番号)に並べる。
pub fn analyze(prs: Vec<Pr>) -> Vec<Entry> {
    let classes: Vec<Vec<Class>> = prs.iter().map(classify).collect();

    // 同じリポジトリの、head → PR の添字。base が別の PR の head なら、その PR が親。
    let mut by_head: HashMap<(String, String), usize> = HashMap::new();
    for (i, pr) in prs.iter().enumerate() {
        if let Some(d) = &pr.details {
            by_head.insert((pr.nwo(), d.head.clone()), i);
        }
    }
    let parent = |i: usize| -> Option<usize> {
        let d = prs[i].details.as_ref()?;
        let p = *by_head.get(&(prs[i].nwo(), d.base.clone()))?;
        (p != i).then_some(p)
    };

    let mut entries: Vec<Entry> = Vec::with_capacity(prs.len());
    for (i, pr) in prs.iter().enumerate() {
        let mut depth = 0;
        let mut below = Vec::new();
        let mut seen = BTreeSet::from([i]);
        let mut cur = i;
        while let Some(p) = parent(cur) {
            if !seen.insert(p) {
                break; // 輪になっている(壊れた入力)
            }
            depth += 1;
            if below.is_empty() && !classes[p].is_empty() {
                below.push(prs[p].found.number);
            }
            cur = p;
        }
        entries.push(Entry {
            pr: pr.clone(),
            classes: classes[i].clone(),
            below,
            depth,
        });
    }
    entries.sort_by(|a, b| {
        (a.pr.nwo(), a.depth, a.pr.found.number).cmp(&(b.pr.nwo(), b.depth, b.pr.found.number))
    });
    entries
}

// ---- 検索と GraphQL の解析 ----

/// `gh search prs --json repository,number,title,url,updatedAt,isDraft` の出力を読む。
pub fn parse_search(json: &str) -> Result<Vec<Found>, String> {
    let v: Value =
        serde_json::from_str(json).map_err(|e| format!("検索の出力が JSON でない: {e}"))?;
    let arr = v.as_array().ok_or("検索の出力が配列でない")?;
    let mut out = Vec::new();
    for item in arr {
        let nwo = item
            .pointer("/repository/nameWithOwner")
            .and_then(Value::as_str)
            .ok_or("検索の結果に repository.nameWithOwner が無い")?;
        let (owner, repo) =
            split_nwo(nwo).ok_or_else(|| format!("リポジトリ名を読めない: {nwo}"))?;
        let s = |k: &str| {
            item.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        out.push(Found {
            owner,
            repo,
            number: item
                .get("number")
                .and_then(Value::as_u64)
                .ok_or("検索の結果に number が無い")?,
            title: s("title"),
            url: s("url"),
            updated_at: s("updatedAt"),
            is_draft: item
                .get("isDraft")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Ok(out)
}

const FRAGMENT: &str = "fragment F on PullRequest { number baseRefName headRefName headRefOid \
baseRef { name } mergeable mergeStateStatus \
commits(first: 100) { totalCount nodes { commit { oid parents(first: 2) { totalCount } \
associatedPullRequests(first: 5) { nodes { number merged } } } } } }";

/// 1 つのリポジトリの PR(番号の並び)を、別名つきで 1 回に引く GraphQL。持ち主・リポジトリ名は
/// `split_nwo` の文字種に限られた値だけを埋める。
pub fn build_query(owner: &str, repo: &str, numbers: &[u64]) -> String {
    let aliases: String = numbers
        .iter()
        .map(|n| format!("p{n}: pullRequest(number: {n}) {{ ...F }} "))
        .collect();
    format!(
        "query {{ repository(owner: \"{owner}\", name: \"{repo}\") {{ {aliases}}} }} {FRAGMENT}"
    )
}

/// [`build_query`] の応答から、番号ごとの詳細を読む。null の PR(閉じた・見えない)は含めない。
pub fn parse_details(json: &str) -> Result<BTreeMap<u64, Details>, String> {
    let v: Value =
        serde_json::from_str(json).map_err(|e| format!("GraphQL の応答が JSON でない: {e}"))?;
    let repo = v
        .pointer("/data/repository")
        .and_then(Value::as_object)
        .ok_or("GraphQL の応答に data.repository が無い")?;
    let mut out = BTreeMap::new();
    for node in repo.values() {
        if node.is_null() {
            continue;
        }
        let number = node
            .get("number")
            .and_then(Value::as_u64)
            .ok_or("PR に number が無い")?;
        let s = |k: &str| {
            node.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let commits = node.pointer("/commits/nodes").and_then(Value::as_array);
        let (mut merge_commits, mut ancestors, mut seen) = (0u64, BTreeSet::new(), 0u64);
        for c in commits.into_iter().flatten() {
            seen += 1;
            if c.pointer("/commit/parents/totalCount")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                > 1
            {
                merge_commits += 1;
            }
            for a in c
                .pointer("/commit/associatedPullRequests/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (n, merged) = (
                    a.get("number").and_then(Value::as_u64),
                    a.get("merged").and_then(Value::as_bool).unwrap_or(false),
                );
                if let (Some(n), true) = (n, merged) {
                    if n != number {
                        ancestors.insert(n);
                    }
                }
            }
        }
        out.insert(
            number,
            Details {
                base: s("baseRefName"),
                head: s("headRefName"),
                head_oid: s("headRefOid"),
                base_exists: node.get("baseRef").is_some_and(|b| !b.is_null()),
                mergeable: s("mergeable"),
                merge_state: s("mergeStateStatus"),
                commits_total: node
                    .pointer("/commits/totalCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(seen),
                commits_seen: seen,
                merge_commits,
                merged_ancestors: ancestors.into_iter().collect(),
            },
        );
    }
    Ok(out)
}

// ---- 実行 ----

/// `gh` の呼び出し(試験で差し替える)。失敗・起動不能は `None`。
pub trait Gh {
    /// `gh search prs --author @me --state open …`。`owners` は、持ち主の絞り(`--owner`)。
    fn search_open_prs(&self, owners: &[String]) -> Option<String>;
    /// `gh api graphql -f query=<query>`。
    fn graphql(&self, query: &str) -> Option<String>;
}

/// 実際の `gh`。
pub struct RealGh;

impl Gh for RealGh {
    fn search_open_prs(&self, owners: &[String]) -> Option<String> {
        let mut args: Vec<&str> = vec![
            "search",
            "prs",
            "--author",
            "@me",
            "--state",
            "open",
            "--archived=false",
            "--limit",
            "1000",
            "--json",
            "repository,number,title,url,updatedAt,isDraft",
        ];
        for o in owners {
            args.push("--owner");
            args.push(o);
        }
        github_command::exec::gh_ok(&args)
    }

    fn graphql(&self, query: &str) -> Option<String> {
        github_command::exec::gh_ok(&["api", "graphql", "-f", &format!("query={query}")])
    }
}

/// 1 回の問い合わせに入れる PR の数の上限(ノード数の上限に収める)。
const CHUNK: usize = 40;

/// GitHub が mergeable を計算中(`UNKNOWN`)のときの読み直し。
#[derive(Debug, Clone, Copy)]
pub struct Retry {
    pub times: u32,
    pub delay: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub owners: Vec<String>,
    pub entries: Vec<Entry>,
}

impl Report {
    /// 0 = すべて clean、1 = 汚れあり、2 = 判定できないものだけが残った。
    pub fn exit_code(&self) -> u8 {
        if self.entries.iter().any(Entry::is_definitely_dirty) {
            1
        } else if self.entries.iter().any(Entry::is_dirty) {
            2
        } else {
            0
        }
    }

    pub fn dirty(&self) -> usize {
        self.entries.iter().filter(|e| e.is_dirty()).count()
    }
}

fn query_repo(
    gh: &dyn Gh,
    owner: &str,
    repo: &str,
    numbers: &[u64],
) -> Option<BTreeMap<u64, Details>> {
    let mut out = BTreeMap::new();
    for chunk in numbers.chunks(CHUNK) {
        let json = gh.graphql(&build_query(owner, repo, chunk))?;
        out.extend(parse_details(&json).ok()?);
    }
    Some(out)
}

/// 母集団(`owners`)の open PR を集めて、分類する。`owners` の外の持ち主の PR は、検索が返しても捨てる。
pub fn run(
    gh: &dyn Gh,
    owners: &[String],
    retry: Retry,
    sleep: &dyn Fn(Duration),
) -> Result<Report, String> {
    let json = gh
        .search_open_prs(owners)
        .ok_or("`gh search prs` に失敗した(認証・ネットワーク)")?;
    let allowed: BTreeSet<String> = owners.iter().map(|o| o.to_ascii_lowercase()).collect();
    let found: Vec<Found> = parse_search(&json)?
        .into_iter()
        .filter(|f| allowed.contains(&f.owner.to_ascii_lowercase()))
        .collect();

    let mut by_repo: BTreeMap<(String, String), Vec<u64>> = BTreeMap::new();
    for f in &found {
        by_repo
            .entry((f.owner.clone(), f.repo.clone()))
            .or_default()
            .push(f.number);
    }
    let mut details: HashMap<(String, String, u64), Details> = HashMap::new();
    for ((owner, repo), numbers) in &by_repo {
        if let Some(map) = query_repo(gh, owner, repo, numbers) {
            for (n, d) in map {
                details.insert((owner.clone(), repo.clone(), n), d);
            }
        }
    }

    // mergeable は、GitHub が計算するまで UNKNOWN で返る。計算中のものだけを、待って引き直す。
    for _ in 0..retry.times {
        let mut pending: BTreeMap<(String, String), Vec<u64>> = BTreeMap::new();
        for (k, d) in &details {
            if d.mergeable == "UNKNOWN" {
                pending
                    .entry((k.0.clone(), k.1.clone()))
                    .or_default()
                    .push(k.2);
            }
        }
        if pending.is_empty() {
            break;
        }
        sleep(retry.delay);
        for ((owner, repo), mut numbers) in pending {
            numbers.sort_unstable();
            if let Some(map) = query_repo(gh, &owner, &repo, &numbers) {
                for (n, d) in map {
                    details.insert((owner.clone(), repo.clone(), n), d);
                }
            }
        }
    }

    let prs = found
        .into_iter()
        .map(|f| {
            let d = details
                .get(&(f.owner.clone(), f.repo.clone(), f.number))
                .cloned();
            Pr {
                found: f,
                details: d,
            }
        })
        .collect();
    Ok(Report {
        owners: owners.to_vec(),
        entries: analyze(prs),
    })
}

// ---- 出力 ----

pub fn render_json(r: &Report) -> String {
    let prs: Vec<Value> = r
        .entries
        .iter()
        .map(|e| {
            let f = &e.pr.found;
            let d = e.pr.details.as_ref();
            json!({
                "repo": format!("{}/{}", f.owner, f.repo),
                "number": f.number,
                "title": f.title,
                "url": f.url,
                "draft": f.is_draft,
                "updatedAt": f.updated_at,
                "base": d.map(|d| d.base.as_str()),
                "head": d.map(|d| d.head.as_str()),
                "headOid": d.map(|d| d.head_oid.as_str()),
                "mergeable": d.map(|d| d.mergeable.as_str()),
                "mergeStateStatus": d.map(|d| d.merge_state.as_str()),
                "classes": e.classes.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
                "mergedAncestors": d.map(|d| d.merged_ancestors.clone()).unwrap_or_default(),
                "below": e.below,
                "depth": e.depth,
                "dirty": e.is_dirty(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({
        "owners": r.owners,
        "checked": r.entries.len(),
        "dirty": r.dirty(),
        "exitCode": r.exit_code(),
        "prs": prs,
    }))
    .unwrap_or_default()
}

pub fn render_table(r: &Report) -> String {
    let owners = r.owners.join(", ");
    let mut out = format!(
        "確認した open PR: {} 本(持ち主: {owners})、汚れ: {} 本\n",
        r.entries.len(),
        r.dirty()
    );
    for e in r.entries.iter().filter(|e| e.is_dirty()) {
        let f = &e.pr.found;
        let mut tags: Vec<String> = e.classes.iter().map(|c| c.as_str().to_string()).collect();
        tags.extend(e.below.iter().map(|n| format!("below:#{n}")));
        let (base, head) =
            e.pr.details
                .as_ref()
                .map(|d| (d.base.as_str(), d.head.as_str()))
                .unwrap_or(("?", "?"));
        let draft = if f.is_draft { "[draft] " } else { "" };
        out.push_str(&format!(
            "- {}/{}#{} {}  {base} ← {head}  {}  {draft}{}\n",
            f.owner,
            f.repo,
            f.number,
            tags.join(","),
            f.updated_at.get(..10).unwrap_or(&f.updated_at),
            f.title
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    // 試験の名前は、すべて架空(ADR-0034)。

    fn found(owner: &str, repo: &str, number: u64) -> Found {
        Found {
            owner: owner.into(),
            repo: repo.into(),
            number,
            title: format!("title {number}"),
            url: format!("https://github.com/{owner}/{repo}/pull/{number}"),
            updated_at: "2026-11-01T00:00:00Z".into(),
            is_draft: false,
        }
    }

    fn clean(base: &str, head: &str) -> Details {
        Details {
            base: base.into(),
            head: head.into(),
            head_oid: "abc".into(),
            base_exists: true,
            mergeable: "MERGEABLE".into(),
            merge_state: "CLEAN".into(),
            commits_total: 1,
            commits_seen: 1,
            merge_commits: 0,
            merged_ancestors: vec![],
        }
    }

    fn pr(number: u64, d: Option<Details>) -> Pr {
        Pr {
            found: found("me", "tools", number),
            details: d,
        }
    }

    #[test]
    fn classifies_each_dirty_kind_and_a_clean_pr_as_empty() {
        assert_eq!(classify(&pr(1, Some(clean("main", "a")))), vec![]);

        let mut d = clean("main", "a");
        d.mergeable = "CONFLICTING".into();
        d.merge_state = "DIRTY".into();
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::Conflict]);

        let mut d = clean("main", "a");
        d.merged_ancestors = vec![7];
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::MergedAncestor]);

        let mut d = clean("gone", "a");
        d.base_exists = false;
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::BaseGone]);

        let mut d = clean("main", "a");
        d.merge_commits = 2;
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::MergeCommit]);

        let mut d = clean("main", "a");
        d.merge_state = "BEHIND".into();
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::Behind]);

        // 計算中のまま・コミットが多すぎる・問い合わせの失敗は、判定できない
        let mut d = clean("main", "a");
        d.mergeable = "UNKNOWN".into();
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::Unknown]);
        let mut d = clean("main", "a");
        d.commits_total = 150;
        d.commits_seen = 100;
        assert_eq!(classify(&pr(1, Some(d))), vec![Class::Unknown]);
        assert_eq!(classify(&pr(1, None)), vec![Class::Unknown]);

        // 複数が同時にある
        let mut d = clean("main", "a");
        d.mergeable = "CONFLICTING".into();
        d.merged_ancestors = vec![3];
        d.merge_commits = 1;
        assert_eq!(
            classify(&pr(1, Some(d))),
            vec![Class::Conflict, Class::MergedAncestor, Class::MergeCommit]
        );
    }

    #[test]
    fn a_dirty_lower_stage_marks_every_stage_above_it_and_orders_bottom_up() {
        // main ← #1(a)← #2(b)← #3(c)。#1 が衝突している。#9 は積まれていない clean な PR。
        let mut conflict = clean("main", "a");
        conflict.mergeable = "CONFLICTING".into();
        let entries = analyze(vec![
            pr(3, Some(clean("b", "c"))),
            pr(9, Some(clean("main", "z"))),
            pr(2, Some(clean("a", "b"))),
            pr(1, Some(conflict)),
        ]);
        let order: Vec<u64> = entries.iter().map(|e| e.pr.found.number).collect();
        assert_eq!(
            order,
            vec![1, 9, 2, 3],
            "リポジトリ、深さ、番号の順。深さ 0 の #1 と #9 が先で、そのあとに #1 の上の段が下から続く"
        );
        let by = |n: u64| entries.iter().find(|e| e.pr.found.number == n).unwrap();
        assert_eq!(by(1).classes, vec![Class::Conflict]);
        assert_eq!((by(1).depth, by(1).below.clone()), (0, vec![]));
        assert_eq!((by(2).depth, by(2).below.clone()), (1, vec![1]));
        assert_eq!(
            (by(3).depth, by(3).below.clone()),
            (2, vec![1]),
            "根の原因は #1"
        );
        assert!(by(2).is_dirty() && by(3).is_dirty());
        assert!(!by(9).is_dirty());
    }

    #[test]
    fn a_clean_stack_is_clean_and_stacks_are_per_repository() {
        let a = Pr {
            found: found("me", "one", 1),
            details: Some(clean("main", "x")),
        };
        // 別のリポジトリの同じ名前のブランチは、親にならない
        let mut b = Pr {
            found: found("me", "two", 2),
            details: Some(clean("x", "y")),
        };
        b.found.repo = "two".into();
        let entries = analyze(vec![a, b]);
        assert!(entries.iter().all(|e| !e.is_dirty() && e.depth == 0));
    }

    #[test]
    fn a_cycle_of_bases_does_not_hang() {
        let entries = analyze(vec![
            pr(1, Some(clean("b", "a"))),
            pr(2, Some(clean("a", "b"))),
        ]);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn search_output_is_parsed_and_rejects_a_broken_shape() {
        let json = r#"[{"repository":{"name":"tools","nameWithOwner":"me/tools"},"number":4,
            "title":"fix x","url":"https://github.com/me/tools/pull/4","updatedAt":"2026-11-01T00:00:00Z","isDraft":true}]"#;
        let v = parse_search(json).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(
            (v[0].owner.as_str(), v[0].repo.as_str(), v[0].number),
            ("me", "tools", 4)
        );
        assert!(v[0].is_draft);
        assert!(parse_search("[]").unwrap().is_empty());
        assert!(parse_search("{").is_err());
        assert!(parse_search(r#"{"a":1}"#).is_err());
        assert!(parse_search(r#"[{"number":1}]"#).is_err());
        assert!(parse_search(r#"[{"repository":{"nameWithOwner":"a b/c"},"number":1}]"#).is_err());
    }

    fn node(
        number: u64,
        base: &str,
        head: &str,
        mergeable: &str,
        state: &str,
        commits: &[(u64, &[(u64, bool)])],
    ) -> String {
        let nodes: Vec<String> = commits
            .iter()
            .map(|(parents, assoc)| {
                let a: Vec<String> = assoc
                    .iter()
                    .map(|(n, m)| format!(r#"{{"number":{n},"merged":{m}}}"#))
                    .collect();
                format!(
                    r#"{{"commit":{{"oid":"o","parents":{{"totalCount":{parents}}},"associatedPullRequests":{{"nodes":[{}]}}}}}}"#,
                    a.join(",")
                )
            })
            .collect();
        format!(
            r#""p{number}":{{"number":{number},"baseRefName":"{base}","headRefName":"{head}","headRefOid":"sha{number}",
            "baseRef":{{"name":"{base}"}},"mergeable":"{mergeable}","mergeStateStatus":"{state}",
            "commits":{{"totalCount":{},"nodes":[{}]}}}}"#,
            commits.len(),
            nodes.join(",")
        )
    }

    fn response(nodes: &[String]) -> String {
        format!(r#"{{"data":{{"repository":{{{}}}}}}}"#, nodes.join(","))
    }

    #[test]
    fn graphql_response_yields_merge_commits_and_merged_ancestors_but_not_itself() {
        let json = response(&[
            // #5 のコミットは、自分(#5、open)と、マージ済みの #3 に属する。2 つ目は merge commit
            node(
                5,
                "main",
                "up",
                "MERGEABLE",
                "CLEAN",
                &[(1, &[(5, false), (3, true)]), (2, &[(5, false)])],
            ),
            node(
                6,
                "main",
                "other",
                "CONFLICTING",
                "DIRTY",
                &[(1, &[(6, false)])],
            ),
        ]);
        let m = parse_details(&json).unwrap();
        assert_eq!(m[&5].merged_ancestors, vec![3]);
        assert_eq!(m[&5].merge_commits, 1);
        assert_eq!((m[&5].commits_total, m[&5].commits_seen), (2, 2));
        assert!(m[&5].base_exists);
        assert_eq!(m[&6].mergeable, "CONFLICTING");
        assert!(m[&6].merged_ancestors.is_empty());
        // 自分自身が merged になっても(マージ直後の取りこぼし)、自分は祖先に数えない
        let me = response(&[node(
            8,
            "main",
            "h",
            "MERGEABLE",
            "CLEAN",
            &[(1, &[(8, true)])],
        )]);
        assert!(parse_details(&me).unwrap()[&8].merged_ancestors.is_empty());
    }

    #[test]
    fn graphql_nulls_and_missing_base_and_broken_shapes() {
        let mut gone = node(4, "gone-branch", "h", "MERGEABLE", "CLEAN", &[(1, &[])]);
        gone = gone.replace(r#""baseRef":{"name":"gone-branch"}"#, r#""baseRef":null"#);
        let json = format!(r#"{{"data":{{"repository":{{{gone},"p9":null}}}}}}"#);
        let m = parse_details(&json).unwrap();
        assert!(!m[&4].base_exists);
        assert!(!m.contains_key(&9), "閉じた・見えない PR は含めない");
        assert!(parse_details("{").is_err());
        assert!(parse_details(r#"{"data":null}"#).is_err());
    }

    #[test]
    fn the_query_names_every_pr_once_with_the_shared_fragment() {
        let q = build_query("me", "tools", &[4, 7]);
        assert!(
            q.contains(r#"repository(owner: "me", name: "tools")"#),
            "{q}"
        );
        assert!(q.contains("p4: pullRequest(number: 4) { ...F }"), "{q}");
        assert!(q.contains("p7: pullRequest(number: 7) { ...F }"), "{q}");
        assert!(q.contains("fragment F on PullRequest"), "{q}");
        assert!(q.contains("associatedPullRequests"), "{q}");
    }

    struct FakeGh {
        search: Option<String>,
        /// リポジトリ名 → 呼ばれるたびに返す応答(最後のものを使い回す)。
        responses: RefCell<BTreeMap<String, Vec<Option<String>>>>,
        graphql_calls: Cell<usize>,
        searched_owners: RefCell<Vec<String>>,
    }

    impl FakeGh {
        fn new(search: Option<&str>) -> Self {
            FakeGh {
                search: search.map(str::to_string),
                responses: RefCell::new(BTreeMap::new()),
                graphql_calls: Cell::new(0),
                searched_owners: RefCell::new(vec![]),
            }
        }
        fn on(self, repo: &str, responses: Vec<Option<String>>) -> Self {
            self.responses
                .borrow_mut()
                .insert(repo.to_string(), responses);
            self
        }
    }

    impl Gh for FakeGh {
        fn search_open_prs(&self, owners: &[String]) -> Option<String> {
            *self.searched_owners.borrow_mut() = owners.to_vec();
            self.search.clone()
        }
        fn graphql(&self, query: &str) -> Option<String> {
            self.graphql_calls.set(self.graphql_calls.get() + 1);
            let mut map = self.responses.borrow_mut();
            let (_, list) = map
                .iter_mut()
                .find(|(repo, _)| query.contains(&format!("name: \"{repo}\"")))?;
            if list.len() > 1 {
                list.remove(0)
            } else {
                list[0].clone()
            }
        }
    }

    fn search_json(items: &[(&str, &str, u64)]) -> String {
        let v: Vec<String> = items
            .iter()
            .map(|(o, r, n)| {
                format!(
                    r#"{{"repository":{{"nameWithOwner":"{o}/{r}"}},"number":{n},"title":"t{n}","url":"u{n}","updatedAt":"2026-11-02T00:00:00Z","isDraft":false}}"#
                )
            })
            .collect();
        format!("[{}]", v.join(","))
    }

    const RETRY: Retry = Retry {
        times: 3,
        delay: Duration::from_millis(0),
    };

    #[test]
    fn run_collects_classifies_and_exits_by_the_result() {
        let gh = FakeGh::new(Some(&search_json(&[
            ("me", "tools", 1),
            ("me", "tools", 2),
        ])))
        .on(
            "tools",
            vec![Some(response(&[
                node(
                    1,
                    "main",
                    "a",
                    "CONFLICTING",
                    "DIRTY",
                    &[(1, &[(1, false)])],
                ),
                node(2, "a", "b", "MERGEABLE", "CLEAN", &[(1, &[(2, false)])]),
            ]))],
        );
        let r = run(&gh, &["me".to_string()], RETRY, &|_| {}).unwrap();
        assert_eq!(r.exit_code(), 1);
        assert_eq!(r.dirty(), 2, "衝突の #1 と、その上の #2");
        assert_eq!(*gh.searched_owners.borrow(), vec!["me".to_string()]);
        let table = render_table(&r);
        assert!(
            table.contains("確認した open PR: 2 本(持ち主: me)、汚れ: 2 本"),
            "{table}"
        );
        assert!(table.contains("me/tools#1 conflict"), "{table}");
        assert!(table.contains("me/tools#2 below:#1"), "{table}");
        let json: Value = serde_json::from_str(&render_json(&r)).unwrap();
        assert_eq!(json["exitCode"], 1);
        assert_eq!(json["prs"][1]["below"], json!([1]));
        assert_eq!(json["prs"][0]["classes"], json!(["conflict"]));
    }

    #[test]
    fn clean_prs_exit_zero() {
        let gh = FakeGh::new(Some(&search_json(&[("me", "tools", 1)]))).on(
            "tools",
            vec![Some(response(&[node(
                1,
                "main",
                "a",
                "MERGEABLE",
                "CLEAN",
                &[(1, &[(1, false)])],
            )]))],
        );
        let r = run(&gh, &["me".into()], RETRY, &|_| {}).unwrap();
        assert_eq!((r.exit_code(), r.dirty()), (0, 0));
        assert!(render_table(&r).contains("汚れ: 0 本"));
    }

    #[test]
    fn results_outside_the_requested_owners_never_enter_the_report() {
        // 検索の絞りが効かず、別の側の持ち主の PR が返ってきても、報告にも問い合わせにも入れない
        let gh = FakeGh::new(Some(&search_json(&[
            ("me", "tools", 1),
            ("acme-corp", "secret", 77),
        ])))
        .on(
            "tools",
            vec![Some(response(&[node(
                1,
                "main",
                "a",
                "MERGEABLE",
                "CLEAN",
                &[(1, &[])],
            )]))],
        )
        .on(
            "secret",
            vec![Some(response(&[node(
                77,
                "main",
                "z",
                "CONFLICTING",
                "DIRTY",
                &[(1, &[])],
            )]))],
        );
        let r = run(&gh, &["me".into()], RETRY, &|_| {}).unwrap();
        assert_eq!(r.entries.len(), 1);
        assert_eq!(
            gh.graphql_calls.get(),
            1,
            "別の側のリポジトリには問い合わせない"
        );
        let all = format!("{}{}", render_table(&r), render_json(&r));
        assert!(
            !all.contains("acme-corp") && !all.contains("secret"),
            "{all}"
        );
    }

    #[test]
    fn a_pr_still_computing_is_retried_and_settles() {
        let computing = node(1, "main", "a", "UNKNOWN", "UNKNOWN", &[(1, &[(1, false)])]);
        let settled = node(
            1,
            "main",
            "a",
            "CONFLICTING",
            "DIRTY",
            &[(1, &[(1, false)])],
        );
        let gh = FakeGh::new(Some(&search_json(&[("me", "tools", 1)]))).on(
            "tools",
            vec![Some(response(&[computing])), Some(response(&[settled]))],
        );
        let slept = Cell::new(0);
        let r = run(&gh, &["me".into()], RETRY, &|_| slept.set(slept.get() + 1)).unwrap();
        assert_eq!(r.entries[0].classes, vec![Class::Conflict]);
        assert_eq!(slept.get(), 1, "確定したら、それ以上は待たない");
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn a_pr_that_never_settles_or_fails_to_query_is_unknown_and_exits_two() {
        let computing = node(1, "main", "a", "UNKNOWN", "UNKNOWN", &[(1, &[(1, false)])]);
        let gh = FakeGh::new(Some(&search_json(&[("me", "tools", 1), ("me", "lost", 2)])))
            .on("tools", vec![Some(response(&[computing]))])
            .on("lost", vec![None]);
        let slept = Cell::new(0);
        let r = run(&gh, &["me".into()], RETRY, &|_| slept.set(slept.get() + 1)).unwrap();
        assert!(r.entries.iter().all(|e| e.classes == vec![Class::Unknown]));
        assert_eq!(slept.get(), 3, "読み直しは、上限の回数まで");
        assert_eq!(r.exit_code(), 2, "確かな汚れが無く、判定できないものだけ");
    }

    #[test]
    fn a_definite_dirt_wins_over_unknown_in_the_exit_code() {
        let computing = node(1, "main", "a", "UNKNOWN", "UNKNOWN", &[(1, &[(1, false)])]);
        let behind = node(2, "main", "b", "MERGEABLE", "BEHIND", &[(1, &[(2, false)])]);
        let gh = FakeGh::new(Some(&search_json(&[
            ("me", "tools", 1),
            ("me", "tools", 2),
        ])))
        .on("tools", vec![Some(response(&[computing, behind]))]);
        let r = run(&gh, &["me".into()], RETRY, &|_| {}).unwrap();
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn search_failure_and_broken_output_are_errors() {
        let gh = FakeGh::new(None);
        assert!(run(&gh, &["me".into()], RETRY, &|_| {}).is_err());
        let gh = FakeGh::new(Some("not json"));
        assert!(run(&gh, &["me".into()], RETRY, &|_| {}).is_err());
    }

    #[test]
    fn many_prs_in_one_repository_are_queried_in_chunks() {
        let items: Vec<(&str, &str, u64)> = (1..=45).map(|n| ("me", "tools", n)).collect();
        let nodes: Vec<String> = (1..=45)
            .map(|n| {
                node(
                    n,
                    "main",
                    &format!("h{n}"),
                    "MERGEABLE",
                    "CLEAN",
                    &[(1, &[])],
                )
            })
            .collect();
        let gh = FakeGh::new(Some(&search_json(&items))).on("tools", vec![Some(response(&nodes))]);
        let r = run(&gh, &["me".into()], RETRY, &|_| {}).unwrap();
        assert_eq!(gh.graphql_calls.get(), 2, "40 + 5");
        assert_eq!(r.entries.len(), 45);
    }
}
