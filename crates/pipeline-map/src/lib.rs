//! `pipeline-map.toml`(自動発火する機構の地図、manifest)と、発火を宣言する登録箇所の
//! 突き合わせ。設計と根拠: `docs/adr/` の pipeline-map の ADR、使い方は
//! `docs/pipeline-map.md`。
//!
//! 図は manifest だけから作る(手描きの複写を持たない)。manifest のノードは、登録箇所
//! からのポインタで網羅性を保つ:
//!
//! - Claude Code の hook 宣言(`home/modules/claude.nix` の `claudeHookDeclarations`)は
//!   nix 属性 `node = "<id>";`。nix の評価時にも同じ検査が走るので、ポインタの無い
//!   宣言は適用(`hms`)に至らない。
//! - それ以外の登録箇所は、登録の直前のコメントに `pipeline-node: <id>`。
//!
//! 母集団(ポインタを要求する登録箇所)は [`Class`] の閉じた 7 クラスで、手動 CLI と
//! skill は含まない(`auto = false` のノードとして図に描くだけ)。
//!
//! 検査は [`audit`]、リポジトリ本体への適用は `tests/coverage.rs`(`cargo test`)。

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// 実装スタック(ノードの色分け)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stack {
    Rust,
    Nix,
    Yaml,
    CloudLlm,
    External,
    Skill,
    Git,
    Herdr,
    Store,
}

/// エッジの種類(何を次に渡すか)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Deny,
    Context,
    File,
    Issue,
    Pr,
    Notify,
    Invoke,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    pub id: String,
    pub label: String,
    pub order: u32,
    /// true = このレーンの機構は、ハブ(`hub = true` のノード)のセッションの中で発火する
    /// (hook の event)。図ではハブのコンテナの中に描き、ハブとの間のエッジは描かない。
    #[serde(default)]
    pub session: bool,
    /// 概観図の段(1 = セッションと一連の流れ、2 = 常駐・手動・外部)。省略時は 1。
    #[serde(default = "one")]
    pub row: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    pub id: String,
    pub label: String,
    pub order: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub lane: String,
    pub group: Option<String>,
    pub stack: Stack,
    #[serde(default)]
    pub llm: bool,
    pub pipelines: Vec<String>,
    pub entity: Option<String>,
    pub doc: Option<String>,
    /// true = 登録箇所からのポインタが 1 つ以上必須(人間が名前を打たなくても走る)。
    pub auto: bool,
    /// true = エージェントセッション(`session = true` のレーンを内側に持つ)。1 つだけ。
    #[serde(default)]
    pub hub: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: Kind,
    pub label: String,
    /// 省略時は両端のノードが共通に属するパイプライン。
    pub pipelines: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub lane: Vec<Lane>,
    #[serde(default)]
    pub pipeline: Vec<Pipeline>,
    #[serde(default)]
    pub node: Vec<Node>,
    #[serde(default)]
    pub edge: Vec<Edge>,
}

pub const MANIFEST: &str = "pipeline-map.toml";

pub fn load(root: &Path) -> Result<Manifest, String> {
    let p = root.join(MANIFEST);
    let s = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    toml::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
}

impl Edge {
    /// 実効のパイプライン集合(省略時は両端の共通集合)。
    pub fn effective_pipelines(&self, m: &Manifest) -> Vec<String> {
        if let Some(p) = &self.pipelines {
            return p.clone();
        }
        let find = |id: &str| m.node.iter().find(|n| n.id == id);
        match (find(&self.from), find(&self.to)) {
            (Some(a), Some(b)) => a
                .pipelines
                .iter()
                .filter(|p| b.pipelines.contains(p))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- scan

/// ポインタを要求する登録箇所のクラス(母集団の閉集合)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `claude.nix` の `claudeHookDeclarations` の 1 エントリ(nix 属性 `node`)。
    ClaudeHookDecl,
    /// `run ${registerCodexHooks}` / `run ${registerCopilotHooks}` の呼び出し。
    RegistrarCall,
    /// `systemd.user.timers.*` / `launchd.agents.*` / `systemd.user.tmpfiles.rules`。
    TimerOrAgent,
    /// `.github/workflows/*.yml`(`on:` の直前)。
    Workflow,
    /// `config/git/hooks/*`(先頭のコメント)。
    GitHook,
    /// `config/herdr/config.toml` の `type = "command"` 行。
    HerdrCommand,
    /// `claude.nix` の `statusLineCmd` の定義。
    Statusline,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::ClaudeHookDecl => "Claude hook 宣言",
            Class::RegistrarCall => "Codex/Copilot hook 登録",
            Class::TimerOrAgent => "timer / launchd agent",
            Class::Workflow => "workflow",
            Class::GitHook => "git hook",
            Class::HerdrCommand => "herdr command",
            Class::Statusline => "statusline",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Site {
    pub file: String,
    pub line: usize,
    pub class: Class,
    pub ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub sites: Vec<Site>,
    pub problems: Vec<String>,
}

const TOKEN: &str = "pipeline-node:";

/// ノード id の綴り: 小文字 ASCII・数字・`-`、先頭末尾は `-` 不可、`--` 不可。
pub fn is_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 1 行から `pipeline-node: a, b` の id を拾う。先頭から続く id だけを読み、
/// 最初に id でない語(`<id>`、`—` など)に当たったら止める。
fn tokens_in(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find(TOKEN) {
        let after = &rest[i + TOKEN.len()..];
        for item in after
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
        {
            if is_id(item) {
                out.push(item.to_string());
            } else {
                break;
            }
        }
        rest = after;
    }
    out
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// `idx` 行の直前に連続するコメント行から id を集める。
fn block_tokens(lines: &[&str], idx: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut j = idx;
    while j > 0 {
        j -= 1;
        if !is_comment(lines[j]) {
            break;
        }
        out.extend(tokens_in(lines[j]));
    }
    out
}

/// nix の `key = "value";` 形の行から value を取る。
fn attr_str(t: &str, key: &str) -> Option<String> {
    let rest = t.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn read_lines(root: &Path, rel: &str) -> Result<String, String> {
    fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

fn sorted_files(root: &Path, dir: &str, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root.join(dir)) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if keep(name) {
                    out.push(format!("{dir}/{name}"));
                }
            }
        }
    }
    out.sort();
    out
}

const CLAUDE_NIX: &str = "home/modules/claude.nix";

fn scan_claude_decls(root: &Path, scan: &mut Scan) {
    let src = match read_lines(root, CLAUDE_NIX) {
        Ok(s) => s,
        Err(e) => {
            scan.problems.push(e);
            return;
        }
    };
    let lines: Vec<&str> = src.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_start().starts_with("claudeHookDeclarations = ["))
    else {
        scan.problems.push(format!(
            "{CLAUDE_NIX}: claudeHookDeclarations が見つからない"
        ));
        return;
    };
    // (エントリ開始行, event, node)
    let mut cur: Option<(usize, Option<String>, Option<String>)> = None;
    let mut closed = false;
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("  ];") {
            closed = true;
            break;
        }
        if is_comment(l) {
            continue;
        }
        let t = l.trim();
        if t == "{" {
            cur = Some((i + 1, None, None));
        } else if t == "}" || t == "}," {
            if let Some((ln, event, node)) = cur.take() {
                match (event, node) {
                    (Some(_), Some(id)) => scan.sites.push(Site {
                        file: CLAUDE_NIX.into(),
                        line: ln,
                        class: Class::ClaudeHookDecl,
                        ids: vec![id],
                    }),
                    (Some(ev), None) => scan.problems.push(format!(
                        "{CLAUDE_NIX}:{ln}: hook 宣言(event = \"{ev}\")に node が無い"
                    )),
                    _ => {}
                }
            }
        } else if let Some(c) = cur.as_mut() {
            if let Some(v) = attr_str(t, "event") {
                c.1 = Some(v);
            }
            if let Some(v) = attr_str(t, "node") {
                c.2 = Some(v);
            }
        }
    }
    if !closed {
        scan.problems.push(format!(
            "{CLAUDE_NIX}: claudeHookDeclarations の終端(行頭 2 空白の `];`)が見つからない"
        ));
    }
}

fn push_site(scan: &mut Scan, file: &str, line: usize, class: Class, ids: Vec<String>) {
    if ids.is_empty() {
        scan.problems.push(format!(
            "{file}:{line}: {} に `{TOKEN} <id>` が無い",
            class.name()
        ));
    }
    scan.sites.push(Site {
        file: file.into(),
        line,
        class,
        ids,
    });
}

/// `systemd.user.timers.<name> =` / `launchd.agents.<name> =` /
/// `systemd.user.tmpfiles.rules =` の束縛行か。
fn is_timer_decl(t: &str) -> bool {
    for prefix in ["systemd.user.timers.", "launchd.agents."] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return rest.split_once('=').is_some_and(|(name, _)| {
                let name = name.trim();
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            });
        }
    }
    t.starts_with("systemd.user.tmpfiles.rules") && t.contains('=')
}

fn scan_nix_modules(root: &Path, scan: &mut Scan) {
    for rel in sorted_files(root, "home/modules", |n| n.ends_with(".nix")) {
        let Ok(src) = read_lines(root, &rel) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if is_comment(l) {
                continue;
            }
            let t = l.trim();
            if t.starts_with("run ${registerCodexHooks}")
                || t.starts_with("run ${registerCopilotHooks}")
            {
                push_site(
                    scan,
                    &rel,
                    i + 1,
                    Class::RegistrarCall,
                    block_tokens(&lines, i),
                );
            } else if is_timer_decl(t) {
                push_site(
                    scan,
                    &rel,
                    i + 1,
                    Class::TimerOrAgent,
                    block_tokens(&lines, i),
                );
            } else if rel == CLAUDE_NIX && t.starts_with("statusLineCmd =") {
                push_site(
                    scan,
                    &rel,
                    i + 1,
                    Class::Statusline,
                    block_tokens(&lines, i),
                );
            }
        }
    }
}

fn scan_workflows(root: &Path, scan: &mut Scan) {
    for rel in sorted_files(root, ".github/workflows", |n| {
        n.ends_with(".yml") || n.ends_with(".yaml")
    }) {
        let Ok(src) = read_lines(root, &rel) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        match lines.iter().position(|l| l.starts_with("on:")) {
            Some(i) => push_site(scan, &rel, i + 1, Class::Workflow, block_tokens(&lines, i)),
            None => scan.problems.push(format!(
                "{rel}: `on:` が見つからない(workflow のトリガー宣言)"
            )),
        }
    }
}

fn scan_git_hooks(root: &Path, scan: &mut Scan) {
    for rel in sorted_files(root, "config/git/hooks", |_| true) {
        let Ok(src) = read_lines(root, &rel) else {
            continue;
        };
        let ids: Vec<String> = src.lines().take(15).flat_map(tokens_in).collect();
        push_site(scan, &rel, 1, Class::GitHook, ids);
    }
}

fn scan_herdr(root: &Path, scan: &mut Scan) {
    let rel = "config/herdr/config.toml";
    let Ok(src) = read_lines(root, rel) else {
        scan.problems.push(format!("{rel}: 読めない"));
        return;
    };
    for (i, l) in src.lines().enumerate() {
        if is_comment(l) || !l.contains("type = \"command\"") {
            continue;
        }
        push_site(scan, rel, i + 1, Class::HerdrCommand, tokens_in(l));
    }
}

/// 7 クラスの登録箇所を列挙する。
pub fn scan(root: &Path) -> Scan {
    let mut s = Scan::default();
    scan_claude_decls(root, &mut s);
    scan_nix_modules(root, &mut s);
    scan_workflows(root, &mut s);
    scan_git_hooks(root, &mut s);
    scan_herdr(root, &mut s);
    s
}

// --------------------------------------------------------------- audit

#[derive(Debug, Default)]
pub struct Report {
    pub nodes: usize,
    pub auto_nodes: usize,
    pub sites: usize,
    pub edges: usize,
    pub problems: Vec<String>,
}

fn dup<'a>(ids: impl Iterator<Item = &'a String>, what: &str, problems: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            problems.push(format!("{what} `{id}` が重複している"));
        }
    }
}

fn check_ascii(label: &str, what: &str, problems: &mut Vec<String>) {
    if label.trim().is_empty() {
        problems.push(format!("{what}: label が空"));
    } else if !label.is_ascii() {
        problems.push(format!(
            "{what}: label `{label}` が ASCII でない(図のフォントは CJK を持たない。日本語は md の散文に書く)"
        ));
    } else if label
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        problems.push(format!(
            "{what}: label `{label}` に `\"`・`\\`・制御文字は使えない(D2 のソースに埋めるため)"
        ));
    }
}

pub fn audit(root: &Path, m: &Manifest) -> Report {
    let mut r = Report {
        nodes: m.node.len(),
        auto_nodes: m.node.iter().filter(|n| n.auto).count(),
        edges: m.edge.len(),
        ..Default::default()
    };
    let p = &mut r.problems;

    // ---- manifest の整合
    dup(m.lane.iter().map(|x| &x.id), "lane", p);
    dup(m.pipeline.iter().map(|x| &x.id), "pipeline", p);
    dup(m.node.iter().map(|x| &x.id), "node", p);
    let lanes: BTreeSet<&str> = m.lane.iter().map(|x| x.id.as_str()).collect();
    let pipes: BTreeSet<&str> = m.pipeline.iter().map(|x| x.id.as_str()).collect();
    let nodes: BTreeMap<&str, &Node> = m.node.iter().map(|n| (n.id.as_str(), n)).collect();

    let hubs: Vec<&Node> = m.node.iter().filter(|n| n.hub).collect();
    if hubs.len() > 1 {
        p.push("hub = true のノードが 2 つ以上ある(エージェントセッションは 1 つ)".into());
    }
    if m.lane.iter().any(|l| l.session) && hubs.is_empty() {
        p.push("session = true のレーンがあるのに hub = true のノードが無い".into());
    }
    for h in &hubs {
        if h.auto {
            p.push(format!(
                "node `{}`: hub は登録箇所を持たない(auto = false)",
                h.id
            ));
        }
        if m.lane
            .iter()
            .find(|l| l.id == h.lane)
            .is_some_and(|l| l.session)
        {
            p.push(format!(
                "node `{}`: hub 自身が session = true のレーンに属している",
                h.id
            ));
        }
    }
    let session_rows: BTreeSet<u32> = m.lane.iter().filter(|l| l.session).map(|l| l.row).collect();
    if session_rows.len() > 1 {
        p.push("session = true のレーンが複数の row にまたがっている(同じ row に置く)".into());
    }
    for l in &m.lane {
        if l.row == 0 {
            p.push(format!("lane `{}`: row は 1 以上", l.id));
        }
        if !is_id(&l.id) {
            p.push(format!("lane `{}`: id の綴りが不正", l.id));
        }
        check_ascii(&l.label, &format!("lane `{}`", l.id), p);
    }
    for x in &m.pipeline {
        if !is_id(&x.id) {
            p.push(format!("pipeline `{}`: id の綴りが不正", x.id));
        }
        check_ascii(&x.label, &format!("pipeline `{}`", x.id), p);
    }
    for n in &m.node {
        let what = format!("node `{}`", n.id);
        if !is_id(&n.id) {
            p.push(format!("{what}: id の綴りが不正(小文字・数字・`-`)"));
        }
        check_ascii(&n.label, &what, p);
        if !lanes.contains(n.lane.as_str()) {
            p.push(format!("{what}: 未定義の lane `{}`", n.lane));
        }
        if n.pipelines.is_empty() {
            p.push(format!("{what}: pipelines が空"));
        }
        for x in &n.pipelines {
            if !pipes.contains(x.as_str()) {
                p.push(format!("{what}: 未定義の pipeline `{x}`"));
            }
        }
        for (key, path) in [("entity", &n.entity), ("doc", &n.doc)] {
            if let Some(path) = path {
                if !root.join(path).exists() {
                    p.push(format!("{what}: {key} `{path}` が存在しない"));
                }
            }
        }
    }
    for l in &m.lane {
        if !m.node.iter().any(|n| n.lane == l.id) {
            p.push(format!("lane `{}`: 属する node が無い", l.id));
        }
    }
    for x in &m.pipeline {
        if !m.node.iter().any(|n| n.pipelines.contains(&x.id)) {
            p.push(format!("pipeline `{}`: 属する node が無い", x.id));
        }
    }
    for e in &m.edge {
        let what = format!("edge {} -> {}", e.from, e.to);
        for end in [&e.from, &e.to] {
            if !nodes.contains_key(end.as_str()) {
                p.push(format!("{what}: 未定義の node `{end}`"));
            }
        }
        check_ascii(&e.label, &what, p);
        let eff = e.effective_pipelines(m);
        if eff.is_empty() {
            p.push(format!(
                "{what}: 両端に共通の pipeline が無い(edge に pipelines を書く)"
            ));
        }
        for x in &eff {
            if !pipes.contains(x.as_str()) {
                p.push(format!("{what}: 未定義の pipeline `{x}`"));
            }
            for end in [&e.from, &e.to] {
                if let Some(n) = nodes.get(end.as_str()) {
                    if !n.pipelines.contains(x) {
                        p.push(format!(
                            "{what}: pipeline `{x}` に描くが、node `{end}` はその pipeline に属さない"
                        ));
                    }
                }
            }
        }
    }

    // ---- 登録箇所との突き合わせ
    let s = scan(root);
    r.sites = s.sites.len();
    let p = &mut r.problems;
    p.extend(s.problems);
    let mut registered: BTreeMap<&str, usize> = BTreeMap::new();
    for site in &s.sites {
        for id in &site.ids {
            match nodes.get(id.as_str()) {
                None => p.push(format!(
                    "{}:{}: `{id}` は {MANIFEST} に定義が無い",
                    site.file, site.line
                )),
                Some(n) if !n.auto => p.push(format!(
                    "{}:{}: `{id}` は auto = false なのに登録箇所からポインタされている(auto = true にするか、ポインタを外す)",
                    site.file, site.line
                )),
                Some(_) => {
                    *registered.entry(id.as_str()).or_insert(0) += 1;
                }
            }
        }
    }
    for n in m.node.iter().filter(|n| n.auto) {
        if !registered.contains_key(n.id.as_str()) {
            p.push(format!(
                "node `{}`: auto = true だが、ポインタする登録箇所が 1 つも無い(機構が消えたなら node を消す。手動なら auto = false)",
                n.id
            ));
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    const CLAUDE_OK: &str = "\
  claudeHookDeclarations = [
    {
      event = \"PreToolUse\";
      node = \"guard-a\";
      command = a;
    }
    # コメント行は読み飛ばす
    {
      event = \"Stop\";
      node = \"guard-b\";
    }
  ];
";

    const MANIFEST_OK: &str = r#"
[[lane]]
id = "tool-call"
label = "Tool call"
order = 1
[[lane]]
id = "timer"
label = "Timer"
order = 2
[[pipeline]]
id = "p1"
label = "P1"
order = 1
[[node]]
id = "guard-a"
label = "guard-a"
lane = "tool-call"
stack = "rust"
pipelines = ["p1"]
auto = true
[[node]]
id = "guard-b"
label = "guard-b"
lane = "tool-call"
stack = "rust"
pipelines = ["p1"]
auto = true
[[node]]
id = "tick"
label = "tick"
lane = "timer"
stack = "nix"
pipelines = ["p1"]
auto = true
[[node]]
id = "cli"
label = "cli"
lane = "timer"
stack = "rust"
pipelines = ["p1"]
auto = false
[[edge]]
from = "guard-a"
to = "guard-b"
kind = "deny"
label = "blocks"
"#;

    /// 全クラスが揃った最小のリポジトリ。
    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(r, "home/modules/claude.nix", CLAUDE_OK);
        write(
            r,
            "home/modules/herdr.nix",
            "  home.activation.x = ''\n    # pipeline-node: guard-a\n    run ${registerCodexHooks} \"$HOME/h.json\" \\\n      Stop \"\" cmd 10\n  '';\n",
        );
        write(
            r,
            "home/modules/t.nix",
            "# pipeline-node: tick\nsystemd.user.timers.tick = lib.mkIf pkgs.stdenv.isLinux {\n};\n# pipeline-node: tick\nlaunchd.agents.tick = {\n};\n",
        );
        write(
            r,
            ".github/workflows/ci.yml",
            "name: ci\n# pipeline-node: tick\non:\n  push:\n",
        );
        write(
            r,
            "config/git/hooks/pre-commit",
            "#!/bin/sh\n# pipeline-node: guard-b\n",
        );
        write(
            r,
            "config/herdr/config.toml",
            "x = [\n  { type = \"command\", command = \"c\" }, # pipeline-node: tick\n]\n",
        );
        write(r, MANIFEST, MANIFEST_OK);
        d
    }

    fn problems(root: &Path) -> Vec<String> {
        let m = load(root).unwrap();
        audit(root, &m).problems
    }

    #[test]
    fn minimal_repo_passes() {
        let d = repo();
        let m = load(d.path()).unwrap();
        let r = audit(d.path(), &m);
        assert!(r.problems.is_empty(), "{:#?}", r.problems);
        // claude 宣言 2 + registrar 1 + timer/agent 2 + workflow 1 + git hook 1 + herdr 1
        assert_eq!(r.sites, 8);
    }

    #[test]
    fn token_parsing_stops_at_non_id() {
        assert_eq!(tokens_in("# pipeline-node: a, b-c  d"), ["a", "b-c", "d"]);
        assert_eq!(
            tokens_in("# `pipeline-node: <id>` を書く"),
            Vec::<String>::new()
        );
        assert_eq!(tokens_in("# pipeline-node: a — 理由"), ["a"]);
        assert!(!is_id("-a") && !is_id("a--b") && !is_id("A") && !is_id(""));
    }

    #[test]
    fn claude_decl_without_node_fails() {
        let d = repo();
        write(
            d.path(),
            CLAUDE_NIX,
            &CLAUDE_OK.replace("      node = \"guard-b\";\n", ""),
        );
        let p = problems(d.path());
        assert!(p.iter().any(|x| x.contains("に node が無い")), "{p:#?}");
    }

    #[test]
    fn registrar_call_without_token_fails() {
        let d = repo();
        write(
            d.path(),
            "home/modules/herdr.nix",
            "  home.activation.x = ''\n    run ${registerCodexHooks} \"$HOME/h.json\" \\\n      Stop \"\" cmd 10\n  '';\n",
        );
        let p = problems(d.path());
        assert!(
            p.iter()
                .any(|x| x.contains("Codex/Copilot hook 登録") && x.contains("が無い")),
            "{p:#?}"
        );
    }

    #[test]
    fn timer_without_token_fails_and_comments_are_ignored() {
        let d = repo();
        write(
            d.path(),
            "home/modules/t.nix",
            "# systemd.user.timers.ghost = は説明\nsystemd.user.timers.tick = {\n};\n# pipeline-node: tick\nlaunchd.agents.tick = {\n};\n",
        );
        let p = problems(d.path());
        assert_eq!(
            p.iter()
                .filter(|x| x.contains("timer / launchd agent"))
                .count(),
            1,
            "{p:#?}"
        );
    }

    #[test]
    fn workflow_git_hook_and_herdr_without_token_fail() {
        let d = repo();
        write(d.path(), ".github/workflows/ci.yml", "on:\n  push:\n");
        write(d.path(), "config/git/hooks/pre-commit", "#!/bin/sh\n");
        write(
            d.path(),
            "config/herdr/config.toml",
            "x = [\n  { type = \"command\", command = \"c\" },\n]\n",
        );
        let p = problems(d.path());
        for c in ["workflow", "git hook", "herdr command"] {
            assert!(
                p.iter().any(|x| x.contains(c) && x.contains("が無い")),
                "{c}: {p:#?}"
            );
        }
    }

    #[test]
    fn unknown_id_and_auto_mismatch_fail() {
        let d = repo();
        write(
            d.path(),
            "config/git/hooks/pre-commit",
            "#!/bin/sh\n# pipeline-node: nope, cli\n",
        );
        let p = problems(d.path());
        assert!(
            p.iter()
                .any(|x| x.contains("`nope`") && x.contains("定義が無い")),
            "{p:#?}"
        );
        assert!(
            p.iter()
                .any(|x| x.contains("`cli`") && x.contains("auto = false")),
            "{p:#?}"
        );
    }

    #[test]
    fn auto_node_without_site_fails() {
        let d = repo();
        let m = MANIFEST_OK.replace(
            "[[node]]\nid = \"cli\"",
            "[[node]]\nid = \"orphan\"\nlabel = \"orphan\"\nlane = \"timer\"\nstack = \"rust\"\npipelines = [\"p1\"]\nauto = true\n[[node]]\nid = \"cli\"",
        );
        write(d.path(), MANIFEST, &m);
        let p = problems(d.path());
        assert!(
            p.iter()
                .any(|x| x.contains("`orphan`") && x.contains("登録箇所が 1 つも無い")),
            "{p:#?}"
        );
    }

    #[test]
    fn manifest_consistency_errors() {
        let d = repo();
        let m = MANIFEST_OK
            .replace("label = \"guard-a\"", "label = \"ガード\"")
            .replace(
                "lane = \"timer\"\nstack = \"nix\"",
                "lane = \"nolane\"\nstack = \"nix\"",
            )
            .replace("to = \"guard-b\"", "to = \"ghost\"");
        write(d.path(), MANIFEST, &m);
        let p = problems(d.path());
        assert!(p.iter().any(|x| x.contains("ASCII でない")), "{p:#?}");
        assert!(
            p.iter().any(|x| x.contains("未定義の lane `nolane`")),
            "{p:#?}"
        );
        assert!(
            p.iter().any(|x| x.contains("未定義の node `ghost`")),
            "{p:#?}"
        );
    }

    #[test]
    fn unknown_enum_and_field_are_rejected_at_load() {
        let d = repo();
        write(
            d.path(),
            MANIFEST,
            &MANIFEST_OK.replace("\"rust\"", "\"python\""),
        );
        assert!(load(d.path()).is_err());
        write(
            d.path(),
            MANIFEST,
            &MANIFEST_OK.replace("auto = false", "auto = false\nextra = 1"),
        );
        assert!(load(d.path()).is_err());
    }

    #[test]
    fn edge_pipeline_must_contain_both_endpoints() {
        let d = repo();
        // guard-a は p1 だけに属するので、p2 を足した edge は両端が描けない。
        let m = MANIFEST_OK
            .replace("[[pipeline]]\nid = \"p1\"\nlabel = \"P1\"\norder = 1\n", "[[pipeline]]\nid = \"p1\"\nlabel = \"P1\"\norder = 1\n[[pipeline]]\nid = \"p2\"\nlabel = \"P2\"\norder = 2\n")
            .replace("label = \"blocks\"", "label = \"blocks\"\npipelines = [\"p2\"]");
        write(d.path(), MANIFEST, &m);
        let p = problems(d.path());
        assert!(
            p.iter()
                .any(|x| x.contains("pipeline `p2` に描くが") && x.contains("guard-a")),
            "{p:#?}"
        );
    }

    #[test]
    fn labels_cannot_hold_quotes_or_backslashes() {
        let d = repo();
        write(
            d.path(),
            MANIFEST,
            &MANIFEST_OK.replace("label = \"blocks\"", "label = \"say \\\"hi\\\"\""),
        );
        let p = problems(d.path());
        assert!(p.iter().any(|x| x.contains("制御文字は使えない")), "{p:#?}");
    }

    #[test]
    fn hub_and_session_lane_rules() {
        let d = repo();
        // session のレーンがあるのに hub が無い
        let m = MANIFEST_OK.replace(
            "id = \"tool-call\"\nlabel = \"Tool call\"\norder = 1\n",
            "id = \"tool-call\"\nlabel = \"Tool call\"\norder = 1\nsession = true\n",
        );
        write(d.path(), MANIFEST, &m);
        let p = problems(d.path());
        assert!(
            p.iter().any(|x| x.contains("hub = true のノードが無い")),
            "{p:#?}"
        );
        // hub が 2 つ、auto な hub
        let m2 = m
            .replace("lane = \"timer\"\nstack = \"rust\"\npipelines = [\"p1\"]\nauto = false\n", "lane = \"timer\"\nstack = \"rust\"\npipelines = [\"p1\"]\nauto = false\nhub = true\n")
            .replace("lane = \"tool-call\"\nstack = \"rust\"\npipelines = [\"p1\"]\nauto = true\n[[node]]\nid = \"guard-b\"", "lane = \"tool-call\"\nstack = \"rust\"\npipelines = [\"p1\"]\nauto = true\nhub = true\n[[node]]\nid = \"guard-b\"");
        write(d.path(), MANIFEST, &m2);
        let p = problems(d.path());
        assert!(p.iter().any(|x| x.contains("2 つ以上")), "{p:#?}");
        assert!(
            p.iter().any(|x| x.contains("hub は登録箇所を持たない")),
            "{p:#?}"
        );
        assert!(
            p.iter()
                .any(|x| x.contains("session = true のレーンに属している")),
            "{p:#?}"
        );
    }

    #[test]
    fn session_lanes_must_share_a_row() {
        let d = repo();
        let m = MANIFEST_OK
            .replace(
                "id = \"tool-call\"\nlabel = \"Tool call\"\norder = 1\n",
                "id = \"tool-call\"\nlabel = \"Tool call\"\norder = 1\nsession = true\n",
            )
            .replace(
                "id = \"timer\"\nlabel = \"Timer\"\norder = 2\n",
                "id = \"timer\"\nlabel = \"Timer\"\norder = 2\nsession = true\nrow = 2\n",
            );
        write(d.path(), MANIFEST, &m);
        let p = problems(d.path());
        assert!(
            p.iter().any(|x| x.contains("複数の row にまたがっている")),
            "{p:#?}"
        );
    }

    #[test]
    fn duplicate_node_ids_fail() {
        let d = repo();
        write(
            d.path(),
            MANIFEST,
            &MANIFEST_OK.replace("id = \"cli\"", "id = \"tick\""),
        );
        let p = problems(d.path());
        assert!(p.iter().any(|x| x.contains("node `tick` が重複")), "{p:#?}");
    }
}
