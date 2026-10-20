//! manifest から D2 のソースを作る(概観 1 枚 + パイプライン別の詳細図)。
//!
//! 出力は manifest の並び順だけで決まる決定論的な文字列で、`d2 --layout=elk` が
//! SVG にする。図は manifest からだけ作り、手で直さない。
//!
//! - 概観(`overview`): レーン(発火のきっかけ)を番号付きで時間順に並べた「地図」。
//!   レーンの中は group(レーン内で畳んだ単位)ごとに 1 ノードにし、色 = スタック、
//!   六角形 = LLM、末尾の `P<n>` = 属するパイプライン。配線はレーン間の流れだけで、
//!   機構同士の配線は詳細図が受け持つ。
//! - 詳細(`<pipeline id>`): そのパイプラインに属するノードとエッジを描く。ハブ(エージェント
//!   セッション)は、`session = true` のレーンを内側に持つコンテナとして描き、ハブとそのレーン内の
//!   ノードの間のエッジは描かない(deny を持つノードは赤い太枠で示す)。ハブへ向かう配線が
//!   大量になり、全部描くと読めなくなるため。
//!
//! ラベルは ASCII のみ(manifest の検査が保証する)。日本語の説明は md の散文に置く。

use crate::{Edge, Kind, Lane, Manifest, Node, Stack};
use std::collections::BTreeSet;

pub const OVERVIEW: &str = "overview";

/// `(ファイル名の stem, D2 ソース)`。概観が先頭、続いてパイプラインの order 順。
pub fn render_all(m: &Manifest) -> Vec<(String, String)> {
    let mut out = vec![(OVERVIEW.to_string(), overview(m))];
    let mut pipes: Vec<_> = m.pipeline.iter().collect();
    pipes.sort_by_key(|p| p.order);
    for p in pipes {
        out.push((p.id.clone(), detail(m, &p.id)));
    }
    out
}

fn sorted_lanes(m: &Manifest) -> Vec<&Lane> {
    let mut lanes: Vec<_> = m.lane.iter().collect();
    lanes.sort_by_key(|l| l.order);
    lanes
}

fn ident(prefix: &str, id: &str) -> String {
    format!("{prefix}_{}", id.replace('-', "_"))
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ------------------------------------------------------------ スタイル

fn stack_class(s: Stack) -> &'static str {
    match s {
        Stack::Rust => "s_rust",
        Stack::Nix => "s_nix",
        Stack::Yaml => "s_yaml",
        Stack::CloudLlm => "s_cloud_llm",
        Stack::External => "s_external",
        Stack::Skill => "s_skill",
        Stack::Git => "s_git",
        Stack::Herdr => "s_herdr",
        Stack::Store => "s_store",
    }
}

fn stack_label(s: Stack) -> &'static str {
    match s {
        Stack::Rust => "Rust crate",
        Stack::Nix => "nix / home-manager",
        Stack::Yaml => "GitHub Actions workflow",
        Stack::CloudLlm => "LLM session / cloud",
        Stack::External => "external tool / service",
        Stack::Skill => "skill (LLM follows a procedure)",
        Stack::Git => "git hook",
        Stack::Herdr => "herdr",
        Stack::Store => "state file",
    }
}

const STACKS: [Stack; 9] = [
    Stack::Rust,
    Stack::Nix,
    Stack::Yaml,
    Stack::CloudLlm,
    Stack::External,
    Stack::Skill,
    Stack::Git,
    Stack::Herdr,
    Stack::Store,
];

const KINDS: [Kind; 7] = [
    Kind::Deny,
    Kind::Context,
    Kind::File,
    Kind::Issue,
    Kind::Pr,
    Kind::Notify,
    Kind::Invoke,
];

fn kind_class(k: Kind) -> &'static str {
    match k {
        Kind::Deny => "e_deny",
        Kind::Context => "e_context",
        Kind::File => "e_file",
        Kind::Issue => "e_issue",
        Kind::Pr => "e_pr",
        Kind::Notify => "e_notify",
        Kind::Invoke => "e_invoke",
    }
}

fn kind_help(k: Kind) -> &'static str {
    match k {
        Kind::Deny => "deny: stops the action",
        Kind::Context => "context: passes information",
        Kind::File => "file: writes a file or state",
        Kind::Issue => "issue: creates or closes an Issue",
        Kind::Pr => "pr: creates a PR",
        Kind::Notify => "notify: toast, display, check",
        Kind::Invoke => "invoke: calls or redirects to",
    }
}

/// Okabe-Ito の色覚多様性に配慮した組。(塗り, 塗りに対して読める文字色)
fn stack_style(s: Stack) -> (&'static str, &'static str) {
    match s {
        Stack::Rust => ("#E69F00", "#111111"),
        Stack::Nix => ("#56B4E9", "#111111"),
        Stack::Yaml => ("#009E73", "#FFFFFF"),
        Stack::CloudLlm => ("#CC79A7", "#111111"),
        Stack::External => ("#BBBBBB", "#111111"),
        Stack::Skill => ("#F0E442", "#111111"),
        Stack::Git => ("#0072B2", "#FFFFFF"),
        Stack::Herdr => ("#D55E00", "#FFFFFF"),
        Stack::Store => ("#FFFFFF", "#111111"),
    }
}

/// (色, 追加のスタイル行)
fn kind_style(k: Kind) -> (&'static str, &'static str) {
    match k {
        Kind::Deny => ("#D55E00", "stroke-dash: 4"),
        Kind::Context => ("#0072B2", "stroke-dash: 2"),
        Kind::File => ("#444444", ""),
        Kind::Issue => ("#009E73", ""),
        Kind::Pr => ("#009E73", "stroke-width: 3"),
        Kind::Notify => ("#888888", ""),
        Kind::Invoke => ("#000000", "stroke-width: 1"),
    }
}

const FLOW: &str = "style: {\n    stroke: \"#555555\"\n    stroke-width: 3\n  }";

fn header(out: &mut String, direction: &str) {
    out.push_str("# 自動生成: `pipeline-map render`。pipeline-map.toml から作る。手で直さない。\n");
    out.push_str(
        "vars: {\n  d2-config: {\n    layout-engine: elk\n    theme-id: 0\n    pad: 40\n  }\n}\n",
    );
    out.push_str(&format!("direction: {direction}\n\nclasses: {{\n"));
    for s in STACKS {
        let (fill, font) = stack_style(s);
        let dash = if s == Stack::Store {
            "\n      stroke-dash: 3"
        } else {
            ""
        };
        out.push_str(&format!(
            "  {}: {{\n    style: {{\n      fill: \"{fill}\"\n      font-color: \"{font}\"\n      font-size: 13\n      stroke: \"#333333\"{dash}\n    }}\n  }}\n",
            stack_class(s)
        ));
    }
    for k in KINDS {
        let (color, extra) = kind_style(k);
        let extra = if extra.is_empty() {
            String::new()
        } else {
            format!("\n      {extra}")
        };
        out.push_str(&format!(
            "  {}: {{\n    style: {{\n      stroke: \"{color}\"\n      font-color: \"{color}\"\n      font-size: 11{extra}\n    }}\n  }}\n",
            kind_class(k)
        ));
    }
    out.push_str("  lane: {\n    style: {\n      fill: \"#F4F6F8\"\n      stroke: \"#9AA5B1\"\n      font-size: 15\n      bold: true\n    }\n  }\n");
    out.push_str("  session: {\n    style: {\n      fill: \"#FBF3F8\"\n      stroke: \"#B05A8E\"\n      font-size: 17\n      bold: true\n    }\n  }\n");
    out.push_str("  plain: {\n    style: {\n      fill: \"#FFFFFF\"\n      stroke: \"#555555\"\n      font-size: 12\n    }\n  }\n");
    out.push_str("  invisible: {\n    style: {\n      stroke-width: 0\n      fill: transparent\n    }\n  }\n");
    out.push_str("}\n\n");
}

// ------------------------------------------------------------- 凡例

/// 凡例のチップ。`d2_id` はコンテナ内で一意な名前。
fn chip(out: &mut String, d2_id: &str, label: &str, class: &str, extra: &[&str]) {
    out.push_str(&format!(
        "  {d2_id}: \"{}\" {{\n    class: {class}\n",
        esc(label)
    ));
    for e in extra {
        out.push_str(&format!("    {e}\n"));
    }
    out.push_str("  }\n");
}

/// 詳細図の凡例(下に置く)。使った stack と edge の kind だけを並べる。
fn legend(out: &mut String, stacks: &[Stack], kinds: &[Kind], llm: bool, deny_marked: bool) {
    out.push_str("legend: \"Legend\" {\n  near: bottom-center\n  class: lane\n  grid-columns: 4\n  grid-gap: 8\n");
    for s in stacks {
        chip(
            out,
            &format!("lg_{}", stack_class(*s)),
            stack_label(*s),
            stack_class(*s),
            &[],
        );
    }
    if llm {
        chip(
            out,
            "lg_llm",
            "hexagon: an LLM takes part in the decision",
            "s_cloud_llm",
            &["shape: hexagon"],
        );
    }
    if deny_marked {
        chip(
            out,
            "lg_deny_mark",
            "red thick border: can stop the agent's action",
            "s_rust",
            &["style.stroke: \"#D55E00\"", "style.stroke-width: 4"],
        );
    }
    chip(
        out,
        "lg_manual",
        "dashed border: not fired by itself (skill, CLI, service)",
        "s_external",
        &["style.stroke-dash: 4"],
    );
    for k in kinds {
        let (color, extra) = kind_style(*k);
        let dash = extra
            .strip_prefix("stroke-dash: ")
            .map(|d| format!("style.stroke-dash: {d}"));
        let mut lines = vec![
            format!("style.stroke: \"{color}\""),
            format!("style.font-color: \"{color}\""),
            "style.fill: \"#FFFFFF\"".to_string(),
            "style.stroke-width: 3".to_string(),
        ];
        lines.extend(dash);
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        chip(
            out,
            &format!("lg_{}", kind_class(*k)),
            &format!("edge {}", kind_help(*k)),
            "plain",
            &refs,
        );
    }
    out.push_str("}\n");
}

// ----------------------------------------------------------------- 共通

fn hub(m: &Manifest) -> Option<&Node> {
    m.node.iter().find(|n| n.hub)
}

fn is_session_lane(m: &Manifest, lane: &str) -> bool {
    m.lane.iter().any(|l| l.id == lane && l.session)
}

fn in_pipeline(n: &Node, p: &str) -> bool {
    n.pipelines.iter().any(|x| x == p)
}

fn node_shape(n: &Node) -> Option<&'static str> {
    if n.llm {
        Some("hexagon")
    } else if n.stack == Stack::Store {
        Some("cylinder")
    } else if n.stack == Stack::Skill {
        Some("document")
    } else {
        None
    }
}

// ----------------------------------------------------------------- 概観

/// 概観のノード = (レーン, group)。group が無ければノード自身。ハブは含めない(コンテナになる)。
struct Group<'a> {
    key: String,
    members: Vec<&'a Node>,
}

fn group_key(n: &Node) -> String {
    n.group.clone().unwrap_or_else(|| n.id.clone())
}

fn dominant_stack(members: &[&Node]) -> Stack {
    let mut counts: Vec<(Stack, usize)> = Vec::new();
    for n in members {
        match counts.iter_mut().find(|(s, _)| *s == n.stack) {
            Some(c) => c.1 += 1,
            None => counts.push((n.stack, 1)),
        }
    }
    // 件数が最大で、同数なら manifest で先に現れた方
    let mut best = counts[0];
    for c in &counts[1..] {
        if c.1 > best.1 {
            best = *c;
        }
    }
    best.0
}

fn lane_groups<'a>(m: &'a Manifest, lane: &str) -> Vec<Group<'a>> {
    let mut groups: Vec<Group> = Vec::new();
    for n in m.node.iter().filter(|n| n.lane == lane && !n.hub) {
        let key = group_key(n);
        match groups.iter_mut().find(|g| g.key == key) {
            Some(g) => g.members.push(n),
            None => groups.push(Group {
                key,
                members: vec![n],
            }),
        }
    }
    groups
}

/// 属するパイプラインの印。すべてなら `all`、それ以外は `P1,P3`。
fn pipeline_tag(m: &Manifest, members: &[&Node]) -> String {
    let mut orders: Vec<u32> = m
        .pipeline
        .iter()
        .filter(|p| members.iter().any(|n| in_pipeline(n, &p.id)))
        .map(|p| p.order)
        .collect();
    orders.sort_unstable();
    if orders.len() == m.pipeline.len() {
        "all".to_string()
    } else {
        let list: Vec<String> = orders.iter().map(u32::to_string).collect();
        format!("P{}", list.join(",P"))
    }
}

fn lane_block(m: &Manifest, l: &Lane, indent: &str, out: &mut String) {
    out.push_str(&format!(
        "{indent}{}: \"{}  {}\" {{\n{indent}  class: lane\n{indent}  grid-columns: 1\n{indent}  grid-gap: 10\n",
        ident("l", &l.id),
        l.order,
        esc(&l.label)
    ));
    for g in lane_groups(m, &l.id) {
        let stack = dominant_stack(&g.members);
        let label = if g.members.len() == 1 {
            g.members[0].label.clone()
        } else {
            format!("{} ({})", g.key, g.members.len())
        };
        out.push_str(&format!(
            "{indent}  {}: \"{}  {}\" {{\n{indent}    class: {}\n",
            ident("g", &g.key),
            esc(&label),
            pipeline_tag(m, &g.members),
            stack_class(stack)
        ));
        if g.members.iter().any(|n| n.llm) {
            out.push_str(&format!("{indent}    shape: hexagon\n"));
        } else if stack == Stack::Store {
            out.push_str(&format!("{indent}    shape: cylinder\n"));
        }
        out.push_str(&format!("{indent}  }}\n"));
    }
    out.push_str(&format!("{indent}}}\n"));
}

pub fn overview(m: &Manifest) -> String {
    let mut out = String::new();
    header(&mut out, "down");
    let lanes: Vec<&Lane> = sorted_lanes(m)
        .into_iter()
        .filter(|l| m.node.iter().any(|n| n.lane == l.id && !n.hub))
        .collect();
    let mut rows: Vec<u32> = lanes.iter().map(|l| l.row).collect();
    rows.sort_unstable();
    rows.dedup();

    let hub = hub(m);
    let mut flow: Vec<String> = Vec::new(); // 一連の流れの矢印(row ごと)
    for r in &rows {
        let row_lanes: Vec<&&Lane> = lanes.iter().filter(|l| l.row == *r).collect();
        let session: Vec<&&Lane> = row_lanes.iter().copied().filter(|l| l.session).collect();
        // 段のセル数: セッションのコンテナ 1 つ + セッション外のレーン
        let cells =
            usize::from(!session.is_empty()) + row_lanes.iter().filter(|l| !l.session).count();
        let row_id = format!("row{r}");
        out.push_str(&format!(
            "{row_id}: \"\" {{\n  class: invisible\n  grid-columns: {cells}\n  grid-gap: 40\n"
        ));
        let mut placed_session = false;
        let mut prev: Option<String> = None;
        for l in &row_lanes {
            let cell = if l.session {
                if !placed_session {
                    placed_session = true;
                    out.push_str(&format!(
                        "  session: \"{}\" {{\n    class: session\n    grid-columns: {}\n    grid-gap: 20\n",
                        esc(hub.map_or("Agent session", |h| h.label.as_str())),
                        session.len()
                    ));
                    for sl in &session {
                        lane_block(m, sl, "    ", &mut out);
                    }
                    out.push_str("  }\n");
                    // セッションの中: レーンの流れ
                    for w in session.windows(2) {
                        flow.push(format!(
                            "{row_id}.session.{} -> {row_id}.session.{}: {{\n  {FLOW}\n}}\n",
                            ident("l", &w[0].id),
                            ident("l", &w[1].id)
                        ));
                    }
                }
                "session".to_string()
            } else {
                lane_block(m, l, "  ", &mut out);
                ident("l", &l.id)
            };
            if prev.as_deref() != Some(cell.as_str()) {
                // 一連の流れ(セッションを含む段だけ): 前のセルから次のセルへ
                if let Some(p) = &prev {
                    if !session.is_empty() {
                        flow.push(format!(
                            "{row_id}.{p} -> {row_id}.{cell}: {{\n  {FLOW}\n}}\n"
                        ));
                    }
                }
                prev = Some(cell);
            }
        }
        out.push_str("}\n\n");
    }
    for f in &flow {
        out.push_str(f);
    }
    // 段を縦に積む(辺が無いと、ELK は段を横に並べて図が横長になる)。
    for w in rows.windows(2) {
        out.push_str(&format!(
            "row{} -> row{}: {{\n  style.opacity: 0\n}}\n",
            w[0], w[1]
        ));
    }
    out.push('\n');
    overview_legend(m, &lanes, &mut out);
    out
}

fn overview_legend(m: &Manifest, lanes: &[&Lane], out: &mut String) {
    let mut stacks: BTreeSet<usize> = BTreeSet::new();
    let mut llm = false;
    for l in lanes {
        for g in lane_groups(m, &l.id) {
            let s = dominant_stack(&g.members);
            if let Some(i) = STACKS.iter().position(|x| *x == s) {
                stacks.insert(i);
            }
            llm |= g.members.iter().any(|n| n.llm);
        }
    }
    out.push_str("legend: \"Legend\" {\n  near: bottom-center\n  class: lane\n  grid-columns: 4\n  grid-gap: 8\n");
    for i in stacks {
        let s = STACKS[i];
        chip(
            out,
            &format!("lg_{}", stack_class(s)),
            stack_label(s),
            stack_class(s),
            &[],
        );
    }
    if llm {
        chip(
            out,
            "lg_llm",
            "hexagon: an LLM takes part",
            "s_cloud_llm",
            &["shape: hexagon"],
        );
    }
    let mut pipes: Vec<_> = m.pipeline.iter().collect();
    pipes.sort_by_key(|p| p.order);
    for p in pipes {
        chip(
            out,
            &format!("lg_p{}", p.order),
            &format!("P{}  {}", p.order, p.label),
            "plain",
            &[],
        );
    }
    out.push_str("}\n");
}

// ----------------------------------------------------------------- 詳細

/// レーンの中のノードが多いときは、縦 1 列にせず格子にする(列数は 6 件ごとに 1 列、最大 3)。
/// 格子の中のノードへの配線は端点どうしの直線になりノードを横切るので、描く配線が 1 本も
/// 触れていないレーン(`connected = false`)だけを格子にする。
fn grid_lines(n: usize, connected: bool, indent: &str) -> String {
    let cols = n.div_ceil(6).clamp(1, 3);
    if cols == 1 || connected {
        String::new()
    } else {
        format!("{indent}grid-columns: {cols}\n{indent}grid-gap: 12\n")
    }
}

/// ハブとセッション内のノードの間のエッジは描かない。
fn is_folded(m: &Manifest, e: &Edge) -> bool {
    let Some(h) = hub(m) else { return false };
    let other = if e.from == h.id {
        &e.to
    } else if e.to == h.id {
        &e.from
    } else {
        return false;
    };
    m.node
        .iter()
        .find(|n| &n.id == other)
        .is_some_and(|n| is_session_lane(m, &n.lane))
}

fn node_ref(m: &Manifest, n: &Node) -> String {
    match hub(m) {
        Some(h) if n.id == h.id => ident("n", &n.id),
        Some(h) if is_session_lane(m, &n.lane) => format!(
            "{}.{}.{}",
            ident("n", &h.id),
            ident("l", &n.lane),
            ident("n", &n.id)
        ),
        _ => format!("{}.{}", ident("l", &n.lane), ident("n", &n.id)),
    }
}

fn node_decl(n: &Node, deny_marked: bool, indent: &str, out: &mut String) {
    out.push_str(&format!(
        "{indent}{}: \"{}\" {{\n{indent}  class: {}\n",
        ident("n", &n.id),
        esc(&n.label),
        stack_class(n.stack)
    ));
    if let Some(shape) = node_shape(n) {
        out.push_str(&format!("{indent}  shape: {shape}\n"));
    }
    if deny_marked {
        out.push_str(&format!(
            "{indent}  style.stroke: \"#D55E00\"\n{indent}  style.stroke-width: 4\n"
        ));
    } else if !n.auto {
        out.push_str(&format!("{indent}  style.stroke-dash: 4\n"));
    }
    out.push_str(&format!("{indent}}}\n"));
}

pub fn detail(m: &Manifest, pipeline: &str) -> String {
    let mut out = String::new();
    header(&mut out, "right");
    let hub = hub(m);
    let nodes: Vec<&Node> = m.node.iter().filter(|n| in_pipeline(n, pipeline)).collect();
    let edges: Vec<&Edge> = m
        .edge
        .iter()
        .filter(|e| e.effective_pipelines(m).iter().any(|x| x == pipeline))
        .collect();

    // ハブへ向かう deny を持つ、セッション内のノード(赤い太枠)
    let deny_marked: BTreeSet<&str> = match hub {
        Some(h) => edges
            .iter()
            .filter(|e| e.kind == Kind::Deny && e.to == h.id && is_folded(m, e))
            .map(|e| e.from.as_str())
            .collect(),
        None => BTreeSet::new(),
    };

    // 描く配線(畳まれないエッジ)が触れているノード
    let touched: BTreeSet<&str> = edges
        .iter()
        .filter(|e| !is_folded(m, e))
        .flat_map(|e| [e.from.as_str(), e.to.as_str()])
        .collect();
    let is_connected = |ns: &[&Node]| ns.iter().any(|n| touched.contains(n.id.as_str()));

    let lanes = sorted_lanes(m);
    let lane_nodes = |l: &Lane| -> Vec<&Node> {
        nodes
            .iter()
            .copied()
            .filter(|n| n.lane == l.id && !n.hub)
            .collect()
    };

    // ハブ: セッション内のレーンを内側に持つコンテナ(内側が無ければ普通の箱)
    if let Some(h) = hub.filter(|h| in_pipeline(h, pipeline)) {
        out.push_str(&format!(
            "{}: \"{}\" {{\n  class: session\n",
            ident("n", &h.id),
            esc(&h.label)
        ));
        let mut prev: Option<&Lane> = None;
        for l in lanes.iter().filter(|l| l.session) {
            let ns = lane_nodes(l);
            if ns.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "  {}: \"{}\" {{\n    class: lane\n{}",
                ident("l", &l.id),
                esc(&l.label),
                grid_lines(ns.len(), is_connected(&ns), "    ")
            ));
            for n in ns {
                node_decl(n, deny_marked.contains(n.id.as_str()), "    ", &mut out);
            }
            out.push_str("  }\n");
            if let Some(p) = prev {
                out.push_str(&format!(
                    "  {} -> {}: {{\n    style.opacity: 0\n  }}\n",
                    ident("l", &p.id),
                    ident("l", &l.id)
                ));
            }
            prev = Some(l);
        }
        out.push_str("}\n\n");
    }
    for l in lanes.iter().filter(|l| !l.session) {
        let ns = lane_nodes(l);
        if ns.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "{}: \"{}\" {{\n  class: lane\n{}",
            ident("l", &l.id),
            esc(&l.label),
            grid_lines(ns.len(), is_connected(&ns), "  ")
        ));
        for n in ns {
            node_decl(n, false, "  ", &mut out);
        }
        out.push_str("}\n\n");
    }

    let by_id = |id: &str| m.node.iter().find(|n| n.id == id);
    let mut kinds: Vec<Kind> = Vec::new();
    for e in edges.iter().filter(|e| !is_folded(m, e)) {
        let (Some(a), Some(b)) = (by_id(&e.from), by_id(&e.to)) else {
            continue;
        };
        out.push_str(&format!(
            "{} -> {}: \"{}\" {{\n  class: {}\n}}\n",
            node_ref(m, a),
            node_ref(m, b),
            esc(&e.label),
            kind_class(e.kind)
        ));
        if !kinds.contains(&e.kind) {
            kinds.push(e.kind);
        }
    }
    out.push('\n');

    let used_stacks: Vec<Stack> = STACKS
        .iter()
        .copied()
        .filter(|s| nodes.iter().any(|n| n.stack == *s))
        .collect();
    let used_kinds: Vec<Kind> = KINDS
        .iter()
        .copied()
        .filter(|k| kinds.contains(k))
        .collect();
    legend(
        &mut out,
        &used_stacks,
        &used_kinds,
        nodes.iter().any(|n| n.llm),
        !deny_marked.is_empty(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = r#"
lane = [
  { id = "tool-call", label = "Tool call", order = 1, session = true },
  { id = "ci", label = "CI", order = 2 },
  { id = "timer", label = "Timers", order = 3, row = 2 },
  { id = "external", label = "Services", order = 4, row = 2 },
]
pipeline = [ { id = "p1", label = "P1", order = 1 }, { id = "p2", label = "P2", order = 2 } ]
node = [
  { id = "guard-a", label = "guard-a", lane = "tool-call", group = "guards", stack = "rust", pipelines = ["p1"], auto = true },
  { id = "guard-b", label = "guard-b", lane = "tool-call", group = "guards", stack = "rust", pipelines = ["p1"], auto = true },
  { id = "build", label = "ci.yml", lane = "ci", stack = "yaml", pipelines = ["p1"], auto = true },
  { id = "tick", label = "tick", lane = "timer", stack = "nix", pipelines = ["p2"], auto = true },
  { id = "session", label = "Session", lane = "external", stack = "cloud-llm", llm = true, pipelines = ["p1", "p2"], auto = false, hub = true },
  { id = "inbox", label = "inbox", lane = "external", stack = "store", pipelines = ["p2"], auto = false },
]
edge = [
  { from = "guard-a", to = "session", kind = "deny", label = "blocks", pipelines = ["p1"] },
  { from = "guard-b", to = "session", kind = "context", label = "info", pipelines = ["p1"] },
  { from = "guard-a", to = "build", kind = "invoke", label = "pushes", pipelines = ["p1"] },
  { from = "session", to = "inbox", kind = "file", label = "--add", pipelines = ["p2"] },
]
"#;

    fn manifest() -> Manifest {
        toml::from_str(M).unwrap()
    }

    #[test]
    fn render_is_deterministic_and_ordered() {
        let m = manifest();
        let a = render_all(&m);
        assert_eq!(a, render_all(&m));
        let names: Vec<&str> = a.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["overview", "p1", "p2"]);
    }

    #[test]
    fn overview_is_a_numbered_lane_map_without_hub_edges() {
        let o = overview(&manifest());
        // 同じ group は 1 ノードに畳まれ、パイプラインの印が付く。
        assert!(o.contains("g_guards: \"guards (2)  P1\""), "{o}");
        // ハブはノードにならず、セッションのコンテナのラベルになる。
        assert!(o.contains("session: \"Session\""), "{o}");
        assert!(!o.contains("g_session"), "{o}");
        // レーンは番号付き。段ごとのグリッド。
        assert!(o.contains("l_tool_call: \"1  Tool call\""), "{o}");
        assert!(
            o.contains("row1: \"\" {") && o.contains("row2: \"\" {"),
            "{o}"
        );
        // 流れの矢印は セッション -> ci だけ。ハブとの間のエッジは無い。
        assert!(o.contains("row1.session -> row1.l_ci:"), "{o}");
        assert!(!o.contains("blocks"), "{o}");
        // 状態ファイルは円柱。
        assert!(o.contains("shape: cylinder"), "{o}");
    }

    #[test]
    fn detail_folds_hub_edges_and_marks_deny_nodes() {
        let m = manifest();
        let p1 = detail(&m, "p1");
        // ハブはコンテナで、セッション内のレーンを内側に持つ。
        assert!(
            p1.contains("n_session: \"Session\" {\n  class: session\n"),
            "{p1}"
        );
        assert!(p1.contains("  l_tool_call: \"Tool call\""), "{p1}");
        // ハブとの間のエッジ(deny も context も)は描かない。deny を持つ guard-a は赤い太枠。
        assert!(!p1.contains("blocks") && !p1.contains("\"info\""), "{p1}");
        assert!(
            p1.contains(
                "n_guard_a: \"guard-a\" {\n      class: s_rust\n      style.stroke: \"#D55E00\""
            ),
            "{p1}"
        );
        assert!(
            !p1.contains("n_guard_b: \"guard-b\" {\n      class: s_rust\n      style.stroke"),
            "{p1}"
        );
        // セッション内のノードを端点にする他のエッジは、入れ子のパスで描く。
        assert!(
            p1.contains("n_session.l_tool_call.n_guard_a -> l_ci.n_build: \"pushes\""),
            "{p1}"
        );
        assert!(p1.contains("red thick border"), "{p1}");
    }

    #[test]
    fn detail_draws_only_the_pipelines_nodes_and_edges() {
        let m = manifest();
        let p2 = detail(&m, "p2");
        assert!(p2.contains("l_external.n_inbox"), "{p2}");
        assert!(
            p2.contains("n_session -> l_external.n_inbox: \"--add\""),
            "{p2}"
        );
        assert!(!p2.contains("n_guard_a"), "{p2}");
        // セッション内の node が無いパイプラインでは、ハブは普通の箱になる。
        assert!(
            p2.contains("n_session: \"Session\" {\n  class: session\n}"),
            "{p2}"
        );
    }

    #[test]
    fn legend_lists_only_used_stacks_and_kinds() {
        let p2 = detail(&manifest(), "p2");
        assert!(
            p2.contains("lg_s_cloud_llm") && p2.contains("lg_s_store"),
            "{p2}"
        );
        assert!(!p2.contains("lg_s_rust"), "{p2}");
        assert!(
            p2.contains("lg_e_file") && !p2.contains("lg_e_deny"),
            "{p2}"
        );
    }

    #[test]
    fn quotes_in_labels_are_escaped() {
        assert_eq!(esc("a\"b\\c"), "a\\\"b\\\\c");
    }
}
