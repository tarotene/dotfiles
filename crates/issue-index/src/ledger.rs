//! 確認の台帳(ADR-1529 の置き場 6)。機体ごとに常設の Issue `verify-ledger: <host>` を 1 件持ち、マージ後の
//! 実機確認を本文の 1 行ずつで持つ。SessionStart で、その機体の台帳から「いま着手できる項目」を選んで注入し、
//! そのセッションのエージェントが `machine` の項目を自分で実行する(docs/agents/issue-index.md「確認の台帳」)。
//!
//! 項目の形(閉じた 1 行):
//!
//! ```text
//! - [ ] after:#<PR> <machine|human> — <何を確かめるか> — check: `<コマンド>`
//! - [ ] after:- <machine|human> — <何を確かめるか>
//! ```
//!
//! 着手できるかは、`after:#<PR>` の PR がマージ済みで、その merge commit がこの機体に適用した rev
//! (`~/.local/state/hms/last-apply.json` の `rev`)の祖先であることで決める。待つ PR の無い確認(機器の使い勝手、
//! 本人の操作だけで済む導入など)は `after:-` と書き、いつでも着手できるものとして扱う。判定は純粋関数で、
//! `gh`・`git` の呼び出しは `main.rs` が持つ。

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
    /// `after:#<PR>` の PR 番号。`after:-`(待つ PR が無い)なら `None`。
    pub pr: Option<u64>,
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
        let after = match self.pr {
            Some(n) => format!("#{n}"),
            None => "-".to_string(),
        };
        format!("after:{after} {kind} — {}", self.rest)
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
    let pr: Option<u64> = match after.strip_prefix("after:")? {
        "-" => None,
        n => Some(n.strip_prefix('#')?.parse().ok()?),
    };
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
/// 待つ PR の無い項目(`after:-`)は、[`item_readiness`] がこの関数を呼ばずに「着手できる」にする。
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

/// 項目 1 件の可否。`after:-` は待つものが無いので「着手できる」。`after:#N` は PR の状態(`states`)と適用で決める。
pub fn item_readiness(
    item: &Item,
    states: &std::collections::HashMap<u64, PrState>,
    applied: impl Fn(&str) -> Option<bool>,
) -> Readiness {
    match item.pr {
        None => Readiness::Ready,
        Some(n) => readiness(states.get(&n), applied),
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
        let mut prs: Vec<u64> = items.iter().filter_map(|i| i.pr).collect();
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

/// 台帳の本文への書き換え 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// 未了の行を、`needle` を含むちょうど 1 行で見つけて `- [x]` にし、行末に結果を足す。
    Mark { needle: String, result: String },
    /// 新しい確認の行を、見出し `### <section>` の節に足す(節が無ければ作る)。
    Add { section: String, line: String },
}

/// 行の中身(改行を除いたもの)と、その行末(`\n`・`\r\n`・無し)。
fn split_eol(line: &str) -> (&str, &str) {
    if let Some(c) = line.strip_suffix("\r\n") {
        (c, "\r\n")
    } else if let Some(c) = line.strip_suffix('\n') {
        (c, "\n")
    } else {
        (line, "")
    }
}

/// 結果・行の文は 1 行(改行を含まない)。台帳の本文の行の形を壊さないため。
fn one_line(what: &str, s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("{what}が空です"));
    }
    if t.contains(['\n', '\r']) {
        return Err(format!("{what}は 1 行で書いてください(改行を含みます)"));
    }
    Ok(t.to_string())
}

/// 未了の行(`- [ ] `)のうち `needle` を含むちょうど 1 行を、`- [x] ` にして、行末に ` — 結果: <result>` を足す。
/// 行頭のインデントと行末の改行(CRLF を含む)は保つ。当たる行が無い・複数ある場合は、直すための手がかり付きの `Err`。
pub fn mark_done(body: &str, needle: &str, result: &str) -> Result<String, String> {
    let needle = one_line("行を探す語", needle)?;
    let result = one_line("結果", result)?;
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let hits: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let (c, _) = split_eol(l);
            c.trim_start().starts_with("- [ ] ") && c.contains(&needle)
        })
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [] => Err(format!("未了の行に「{needle}」を含むものがありません")),
        [i] => {
            let mut out = String::with_capacity(body.len() + result.len() + 16);
            for (k, l) in lines.iter().enumerate() {
                if k != *i {
                    out.push_str(l);
                    continue;
                }
                let (c, eol) = split_eol(l);
                let indent = &c[..c.len() - c.trim_start().len()];
                let rest = c
                    .trim_start()
                    .strip_prefix("- [ ] ")
                    .unwrap_or("")
                    .trim_end();
                out.push_str(&format!("{indent}- [x] {rest} — 結果: {result}{eol}"));
            }
            Ok(out)
        }
        many => {
            let heads: Vec<String> = many
                .iter()
                .map(|i| {
                    let (c, _) = split_eol(lines[*i]);
                    c.trim_start().chars().take(60).collect::<String>()
                })
                .collect();
            Err(format!(
                "「{needle}」を含む未了の行が {} 行あります。1 行に絞ってください:\n  {}",
                many.len(),
                heads.join("\n  ")
            ))
        }
    }
}

/// 項目の行(`- [ ] ` の有無は問わない)を、台帳の形(`after:#<PR> <machine|human> — …`)に合うか確かめて、
/// `- [ ] ` つきの 1 行にする。形に合わなければ `Err`。
fn item_line(line: &str) -> Result<String, String> {
    let t = one_line("項目の行", line)?;
    let rest = t.strip_prefix("- [ ] ").unwrap_or(&t).trim();
    if parse_item(rest).is_none() {
        return Err(format!(
            "項目の形に合いません(`after:#<PR> <machine|human> — <何を確かめるか>`): {rest}"
        ));
    }
    Ok(format!("- [ ] {rest}"))
}

/// 新しい確認の行を、見出し `### <section>`(その語を含む見出しのうち、ちょうど 1 つ)の節に足す。
/// 節に項目があれば最後の項目の次の行に、無ければ見出しの後の空行の次に入れる。節が無ければ、本文の末尾
/// (署名の行 `🤖 Generated with` があればその前)に、見出しごと作る。
pub fn add_item(body: &str, section: &str, line: &str) -> Result<String, String> {
    let section = one_line("節の名前", section)?;
    let item = item_line(line)?;
    let eol = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = body.split_inclusive('\n').map(str::to_string).collect();
    let is_heading = |l: &str| split_eol(l).0.trim_start().starts_with("### ");
    let hits: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_heading(l) && split_eol(l).0.contains(&section))
        .map(|(i, _)| i)
        .collect();
    let with_eol = |s: &str| format!("{s}{eol}");
    match hits.as_slice() {
        [] => {
            // 節を作る。署名の前(無ければ末尾)に、見出し・空行・項目・空行。
            let at = lines
                .iter()
                .position(|l| split_eol(l).0.starts_with("🤖 Generated with"))
                .unwrap_or(lines.len());
            if at == lines.len() {
                if let Some(last) = lines.last_mut() {
                    if !last.ends_with('\n') {
                        last.push_str(eol);
                    }
                }
            }
            let block = [
                with_eol(&format!("### {section}")),
                with_eol(""),
                with_eol(&item),
                with_eol(""),
            ];
            lines.splice(at..at, block);
        }
        [h] => {
            // 節の終わり(次の見出し、または末尾)までで、最後の項目の行を探す。
            let end = lines[h + 1..]
                .iter()
                .position(|l| split_eol(l).0.trim_start().starts_with('#'))
                .map_or(lines.len(), |p| h + 1 + p);
            let last_item = (h + 1..end)
                .rev()
                .find(|&i| split_eol(&lines[i]).0.trim_start().starts_with("- ["));
            let at = match last_item {
                Some(i) => i + 1,
                // 項目が無い節: 見出しの次の空行の次。
                None => (h + 1..end)
                    .find(|&i| split_eol(&lines[i]).0.trim().is_empty())
                    .map_or(h + 1, |i| i + 1),
            };
            // 最後の行に改行が無いとき(本文の末尾の節)は、足してから次の行を置く。
            if at > 0 && !lines[at - 1].ends_with('\n') {
                lines[at - 1].push_str(eol);
            }
            lines.insert(at, with_eol(&item));
        }
        many => {
            let heads: Vec<String> = many
                .iter()
                .map(|i| split_eol(&lines[*i]).0.chars().take(60).collect::<String>())
                .collect();
            return Err(format!(
                "「{section}」を含む見出しが {} 個あります。1 つに絞ってください:\n  {}",
                many.len(),
                heads.join("\n  ")
            ));
        }
    }
    Ok(lines.concat())
}

/// `issue-index ledger` の引数(機体・書き換え・書き出し先)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditArgs {
    pub host: String,
    pub ops: Vec<Op>,
    pub out: String,
}

/// `<host> [--mark <語> --result <結果>]… [--add <行> --section <節>]… --out <ファイル>` を読む。
/// `--result` は直前の `--mark` の結果、`--section` は直前の `--add` の節。組が揃わない・書き換えが 0 件・
/// `--out` が無いのは `Err`。
pub fn parse_edit_args(args: &[String]) -> Result<EditArgs, String> {
    let mut host = None;
    let mut out = None;
    let mut ops = Vec::new();
    let mut mark: Option<String> = None;
    let mut add: Option<String> = None;
    let mut it = args.iter();
    let value = |flag: &str, it: &mut std::slice::Iter<String>| -> Result<String, String> {
        it.next()
            .filter(|v| !v.starts_with("--"))
            .cloned()
            .ok_or_else(|| format!("{flag} には値を渡してください"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--mark" | "--add" if mark.is_some() || add.is_some() => {
                return Err("--mark は --result と、--add は --section と組にしてください".into());
            }
            "--mark" => mark = Some(value("--mark", &mut it)?),
            "--add" => add = Some(value("--add", &mut it)?),
            "--result" => {
                let needle = mark
                    .take()
                    .ok_or("--result は --mark の後に書いてください")?;
                let result = value("--result", &mut it)?;
                ops.push(Op::Mark { needle, result });
            }
            "--section" => {
                let line = add
                    .take()
                    .ok_or("--section は --add の後に書いてください")?;
                let section = value("--section", &mut it)?;
                ops.push(Op::Add { section, line });
            }
            "--out" => out = Some(value("--out", &mut it)?),
            s if s.starts_with("--") => return Err(format!("知らない引数です: {s}")),
            s if host.is_none() => host = Some(s.to_string()),
            s => return Err(format!("機体の名前が 2 つあります: {s}")),
        }
    }
    if mark.is_some() || add.is_some() {
        return Err("--mark は --result と、--add は --section と組にしてください".into());
    }
    if ops.is_empty() {
        return Err(
            "書き換え(--mark/--result か --add/--section)を 1 件以上指定してください".into(),
        );
    }
    Ok(EditArgs {
        host: host.ok_or("機体の名前を指定してください")?,
        ops,
        out: out.ok_or("--out(書き出し先のファイル)を指定してください")?,
    })
}

/// 書き換えを順に当てる。1 件でも失敗したら、全体を `Err`(本文は変えない)。
pub fn apply(body: &str, ops: &[Op]) -> Result<String, String> {
    let mut cur = body.to_string();
    for (n, op) in ops.iter().enumerate() {
        cur = match op {
            Op::Mark { needle, result } => mark_done(&cur, needle, result),
            Op::Add { section, line } => add_item(&cur, section, line),
        }
        .map_err(|e| format!("{} 件目の書き換え: {e}", n + 1))?;
    }
    Ok(cur)
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
                    pr: Some(1404),
                    kind: Kind::Machine,
                    rest: "pkill -f の deny を読む — check: `pkill-guard --check 'pkill -f x'`"
                        .to_string()
                },
                Item {
                    pr: Some(1469),
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
    fn after_dash_means_no_pr_to_wait_for() {
        let p = parse("- [ ] after:- human — カメラの LED が点くのを見る\n- [ ] after:-x machine — 形が違う\n");
        assert_eq!(
            p.open,
            vec![Item {
                pr: None,
                kind: Kind::Human,
                rest: "カメラの LED が点くのを見る".to_string()
            }]
        );
        assert_eq!(p.malformed, vec!["after:-x machine — 形が違う".to_string()]);
        assert_eq!(
            p.open[0].line(),
            "after:- human — カメラの LED が点くのを見る"
        );
        let none = std::collections::HashMap::new();
        assert_eq!(
            item_readiness(&p.open[0], &none, |_| Some(false)),
            Readiness::Ready
        );
        let s = section(1, "vega", &p, &[Readiness::Ready]).unwrap();
        assert!(
            s.contains("着手できる(PR がマージ済みで、この機体に適用済み):\n- after:- human"),
            "{s}"
        );
    }

    #[test]
    fn ledger_title_matches_the_prefix() {
        assert_eq!(ledger_title("altair"), "verify-ledger: altair");
        assert!(ledger_title("vega").starts_with(LEDGER_TITLE_PREFIX));
    }

    const EDIT_BODY: &str = "台帳の説明。\n\n### 元 #1 一つ目の節\n\n- [ ] after:#10 machine — 一つ目 — check: `true`\n- [x] after:#11 machine — 済み — 結果: exit 0\n- [ ] after:#12 human — 三つ目を本人が確かめる\n\n### 元 #2 空の節\n\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\n";

    #[test]
    fn mark_done_checks_the_one_matching_open_line_and_appends_the_result() {
        let out = mark_done(EDIT_BODY, "after:#10 machine", "exit 0(2026-11-15、vega)").unwrap();
        assert!(out.contains(
            "- [x] after:#10 machine — 一つ目 — check: `true` — 結果: exit 0(2026-11-15、vega)\n"
        ));
        // ほかの行・見出し・署名は 1 バイトも変わらない。
        let diff: Vec<_> = EDIT_BODY
            .lines()
            .zip(out.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(diff.len(), 1);
        assert_eq!(parse(&out).done, parse(EDIT_BODY).done + 1);
        assert_eq!(parse(&out).open.len(), parse(EDIT_BODY).open.len() - 1);
        assert!(out.ends_with("claude-code)\n"));
    }

    #[test]
    fn mark_done_rejects_no_match_many_matches_done_lines_and_multiline_text() {
        // 当たる行が無い。済みの行(`- [x]`)には当たらない。
        assert!(mark_done(EDIT_BODY, "存在しない語", "ok")
            .unwrap_err()
            .contains("ありません"));
        assert!(mark_done(EDIT_BODY, "済み", "ok").is_err());
        // 複数に当たる: 候補の頭を添えて、絞らせる。
        let e = mark_done(EDIT_BODY, "after:#1", "ok").unwrap_err();
        assert!(
            e.contains("2 行") && e.contains("after:#10") && e.contains("after:#12"),
            "{e}"
        );
        // 結果・語が空、結果が複数行。
        assert!(mark_done(EDIT_BODY, "after:#10", "  ").is_err());
        assert!(mark_done(EDIT_BODY, "", "ok").is_err());
        assert!(mark_done(EDIT_BODY, "after:#10", "a\nb")
            .unwrap_err()
            .contains("1 行"));
    }

    #[test]
    fn mark_done_keeps_crlf_indent_and_a_missing_final_newline() {
        let crlf = "- [ ] after:#1 machine — a\r\n  - [ ] after:#2 machine — b";
        let out = mark_done(crlf, "after:#1 ", "ok").unwrap();
        assert_eq!(
            out,
            "- [x] after:#1 machine — a — 結果: ok\r\n  - [ ] after:#2 machine — b"
        );
        let out = mark_done(crlf, "after:#2 ", "ok").unwrap();
        assert_eq!(
            out,
            "- [ ] after:#1 machine — a\r\n  - [x] after:#2 machine — b — 結果: ok"
        );
    }

    #[test]
    fn add_item_appends_after_the_last_item_of_the_section() {
        let line = "after:#20 machine — 新しい確認 — check: `true`";
        let out = add_item(EDIT_BODY, "元 #1", line).unwrap();
        let want = "- [ ] after:#12 human — 三つ目を本人が確かめる\n- [ ] after:#20 machine — 新しい確認 — check: `true`\n\n### 元 #2";
        assert!(out.contains(want), "{out}");
        assert_eq!(parse(&out).open.len(), parse(EDIT_BODY).open.len() + 1);
        // `- [ ] ` つきで渡しても同じ。
        assert_eq!(
            add_item(EDIT_BODY, "元 #1", &format!("- [ ] {line}")).unwrap(),
            out
        );
    }

    #[test]
    fn add_item_to_an_empty_section_and_to_a_new_section_before_the_footer() {
        let line = "after:- human — 空の節に足す";
        let out = add_item(EDIT_BODY, "元 #2", line).unwrap();
        assert!(
            out.contains("### 元 #2 空の節\n\n- [ ] after:- human — 空の節に足す\n"),
            "{out}"
        );
        // 見出しが無ければ、署名の前に節ごと作る。
        let out = add_item(EDIT_BODY, "PR #99 の適用後", "after:#99 machine — x").unwrap();
        assert!(
            out.contains(
                "\n### PR #99 の適用後\n\n- [ ] after:#99 machine — x\n\n🤖 Generated with"
            ),
            "{out}"
        );
        // 署名の無い本文の末尾(改行なし)にも作れる。
        let out = add_item("- [x] a", "新", "after:- human — y").unwrap();
        assert_eq!(out, "- [x] a\n### 新\n\n- [ ] after:- human — y\n\n");
    }

    #[test]
    fn add_item_rejects_a_malformed_line_and_an_ambiguous_section() {
        assert!(add_item(EDIT_BODY, "元 #1", "after:#1 robot — 形が違う").is_err());
        assert!(add_item(EDIT_BODY, "元 #1", "ただの文").is_err());
        assert!(add_item(EDIT_BODY, "元 #1", "after:#1 machine — a\nb").is_err());
        let e = add_item(EDIT_BODY, "元 #", "after:- human — x").unwrap_err();
        assert!(
            e.contains("2 個") && e.contains("元 #1") && e.contains("元 #2"),
            "{e}"
        );
    }

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_edit_args_pairs_marks_with_results_and_adds_with_sections() {
        let a = parse_edit_args(&v(&[
            "vega",
            "--mark",
            "after:#1 ",
            "--result",
            "exit 0",
            "--add",
            "after:- human — x",
            "--section",
            "新",
            "--out",
            "/t/o.md",
        ]))
        .unwrap();
        assert_eq!(a.host, "vega");
        assert_eq!(a.out, "/t/o.md");
        assert_eq!(
            a.ops,
            vec![
                Op::Mark {
                    needle: "after:#1 ".into(),
                    result: "exit 0".into()
                },
                Op::Add {
                    section: "新".into(),
                    line: "after:- human — x".into()
                },
            ]
        );
        // 組が揃わない・順が逆・値が無い・書き換えが 0 件・出力先が無い・機体が無い。
        for bad in [
            v(&["vega", "--mark", "a", "--out", "o"]),
            v(&["vega", "--result", "r", "--out", "o"]),
            v(&[
                "vega",
                "--mark",
                "a",
                "--add",
                "l",
                "--section",
                "s",
                "--out",
                "o",
            ]),
            v(&["vega", "--mark", "--result", "r", "--out", "o"]),
            v(&["vega", "--out", "o"]),
            v(&["vega", "--mark", "a", "--result", "r"]),
            v(&["--mark", "a", "--result", "r", "--out", "o"]),
            v(&[
                "vega", "altair", "--mark", "a", "--result", "r", "--out", "o",
            ]),
            v(&["vega", "--nope", "--out", "o"]),
        ] {
            assert!(parse_edit_args(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn apply_runs_ops_in_order_and_fails_as_a_whole() {
        let ops = vec![
            Op::Mark {
                needle: "after:#10 ".into(),
                result: "exit 0".into(),
            },
            Op::Add {
                section: "元 #1".into(),
                line: "after:#30 machine — 追加".into(),
            },
        ];
        let out = apply(EDIT_BODY, &ops).unwrap();
        let p = parse(&out);
        assert_eq!(p.done, 2);
        assert!(p.open.iter().any(|i| i.pr == Some(30)));
        // 2 件目が失敗したら、全体を Err にして、件の番号を添える。
        let bad = vec![
            ops[0].clone(),
            Op::Mark {
                needle: "無い".into(),
                result: "x".into(),
            },
        ];
        assert!(apply(EDIT_BODY, &bad)
            .unwrap_err()
            .starts_with("2 件目の書き換え"));
    }
}
