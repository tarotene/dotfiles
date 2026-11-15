//! repo-scope-guard — 個人の契約の機体で、別の側・表に無い持ち主の**非公開**リポジトリを読む
//! `gh` の呼び出しを、PreToolUse で拒否する判定(設計と根拠は docs/agents/repo-scope-guard.md)。
//!
//! 判定の規則(表と理由)は `repo-scope` が持つ。ここは、`gh` のコマンドから「どのリポジトリ・
//! 持ち主を読もうとしているか」を取り出して、`repo_scope::Policy` に渡す部分である。
//!
//! 見るのは Bash の `gh` だけ。コマンド位置(先頭・区切りの直後・`NAME=値` の直後)にある `gh` を
//! トークン単位で見る(クォートの中の文言には当たらない)。`bash -c '…'` の中身は再帰して見る。
//! トークン化できないコマンドは判定不能として通す(fail-open)。
//!
//! 取り出す対象:
//! - `-R/--repo`、引数の URL(`https://github.com/o/r/…`)、`gh repo <verb> o/r`、
//!   `gh api repos/o/r/…`(`{owner}`・`{repo}` の置換は cwd の origin)。
//! - リポジトリを指さない `pr`・`issue`・`run` などは、cwd の origin。
//! - 横断検索(`gh search …`、`gh api search/…`): `--visibility public` か `is:public` で公開だけに
//!   絞るか、`--owner`/`--repo`/`user:`/`org:`/`repo:` がすべて読んでよいものか。絞りが無ければ拒否。
//! - 持ち主の単位(`gh repo list <持ち主>`、`gh api orgs/<持ち主>/…`、`gh status`)と、横断の一覧
//!   (`gh api user/repos`・`issues`・`notifications`)。公開だけに絞れないので、同じ側の持ち主だけ可。
//!
//! 見ないもの(`docs/agents/repo-scope-guard.md` に書く): `gh api graphql` の本文、ブラウザ、WebFetch、
//! `GH_REPO` 環境変数、`gh` を包む別の道具。これらは共有 AGENTS.md の規範が受け持つ。
//!
//! 逃げ道の環境変数は置かない。規則に例外が要るときは、持ち主の表を直す。

use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};
use command_inspection::command::{gh_command_at, is_gh, parse};
use command_inspection::shell::is_sep;
use repo_scope::{
    load_policy, machine_side, origin_repo, repo_from_url, split_nwo, visibility_of, DenyReason,
    Policy, Side, Verdict, Visibility,
};
use std::path::Path;

/// 再帰して見る `sh -c '…'` の深さ上限。
const MAX_DEPTH: usize = 2;

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];

/// 直後の語がコマンド位置になる予約語(`do gh …`、`! gh …` など)。
const TRANSPARENT: &[&str] = &["do", "then", "else", "!", "time", "exec", "{"];

/// リポジトリを `-R` などで指さなければ、cwd のリポジトリに作用する `gh` のコマンド。
const CWD_REPO_COMMANDS: &[&str] = &[
    "pr", "issue", "run", "workflow", "release", "label", "cache", "ruleset", "secret", "variable",
    "browse", "repo",
];

/// `gh repo <verb> <owner/repo>` の、最初の位置引数がリポジトリになる verb。
const REPO_VERBS: &[&str] = &[
    "view",
    "clone",
    "fork",
    "edit",
    "delete",
    "archive",
    "unarchive",
    "rename",
    "sync",
    "set-default",
    "deploy-key",
    "autolink",
];

/// `gh repo <verb>` で、次のトークンを値として取るフラグ(その値は位置引数に数えない。
/// `gh repo list --limit 5 <持ち主>` の `5` を持ち主と読まないため)。
const REPO_VALUE_FLAGS: &[&str] = &[
    "-L",
    "--limit",
    "--topic",
    "-l",
    "--language",
    "--visibility",
    "--json",
    "-q",
    "--jq",
    "-t",
    "--template",
    "-b",
    "--branch",
    "--source",
    "--fork-name",
    "--remote-name",
    "--org",
    "-d",
    "--description",
    "-h",
    "--homepage",
];

/// フラグと、値を取るフラグの値を除いた、位置引数。
fn positionals(rest: &[String]) -> impl Iterator<Item = &String> {
    let mut skip_next = false;
    rest.iter().filter(move |t| {
        if std::mem::take(&mut skip_next) {
            return false;
        }
        if REPO_VALUE_FLAGS.contains(&t.as_str()) {
            skip_next = true;
            return false;
        }
        !t.starts_with('-')
    })
}

/// 対象を絞る手がかり。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    Repo(String, String),
    Owner(String),
}

/// 判定の入力。環境に触れる部分(cwd の origin・可視性の照会)は、注入できる形で持つ。
struct Ctx<'a> {
    policy: &'a Policy,
    cwd_repo: &'a dyn Fn() -> Option<(String, String)>,
    visibility: &'a mut dyn FnMut(&str) -> Option<Visibility>,
}

const FOOTER: &str = "個人の契約は、入力を学習に使わないことを厳密には保証しません(会社の契約は保証します)。\
そのため、個人の契約の機体では、別の側・表に無い持ち主の**非公開**リポジトリを読みません\
(共有 AGENTS.md「機体の契約の外にある非公開リポジトリを読まない」)。公開リポジトリは読めます: \
`-R <持ち主>/<リポジトリ>`(公開と確かめられるもの)、`gh search … --visibility public`、検索語の `is:public`。\
横断検索は `--owner <同じ側の持ち主>` で絞ります(同じ側の持ち主は `repo-scope` の表 \
`~/.config/dotfiles/github-owners.toml` が決めます)。逃げ道の環境変数はありません。";

impl Ctx<'_> {
    fn deny(&self, reason: DenyReason) -> String {
        let head = match reason {
            DenyReason::NonPublic(nwo) => format!("`{nwo}` は非公開リポジトリです。"),
            DenyReason::VisibilityUnknown(nwo) => format!(
                "`{nwo}` が公開かどうかを確かめられませんでした(404・権限・ネットワーク)。\
公開と確かめられないものは、非公開として扱います。"
            ),
            DenyReason::OtherSideOwner(owner) => format!(
                "`{owner}` は、この機体の契約の側の持ち主ではなく、持ち主の単位の問い合わせは公開だけに絞れません。"
            ),
        };
        format!("{head}\n{FOOTER}")
    }

    fn deny_text(&self, head: &str) -> String {
        format!("{head}\n{FOOTER}")
    }

    fn check_repo(&mut self, owner: &str, repo: &str) -> Option<String> {
        match self.policy.check_repo(owner, repo, self.visibility) {
            Verdict::Allow => None,
            Verdict::Deny(r) => Some(self.deny(r)),
        }
    }

    fn check_owner(&self, owner: &str) -> Option<String> {
        // `@me` は、ログイン中のアカウント自身(個人の持ち主)を指す。
        if owner == "@me" {
            return None;
        }
        match self.policy.check_owner(owner) {
            Verdict::Allow => None,
            Verdict::Deny(r) => Some(self.deny(r)),
        }
    }

    fn check_scope(&mut self, s: &Scope) -> Option<String> {
        match s {
            Scope::Repo(o, r) => self.check_repo(o, r),
            Scope::Owner(o) => self.check_owner(o),
        }
    }

    /// リポジトリを指さない `gh` が作用する、cwd のリポジトリ。
    fn check_cwd(&mut self) -> Option<String> {
        let (o, r) = (self.cwd_repo)()?;
        self.check_repo(&o, &r)
    }
}

// ---- トークンの扱い ----

fn is_env_assignment(t: &str) -> bool {
    t.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty()
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !k.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// `tokens[i]` がコマンド位置にあるか(先頭・区切りの直後・予約語の直後。`NAME=値` は読み飛ばす)。
fn at_command_position(tokens: &[String], mut i: usize) -> bool {
    loop {
        if i == 0 {
            return true;
        }
        let prev = &tokens[i - 1];
        if is_sep(prev) || TRANSPARENT.contains(&prev.as_str()) {
            return true;
        }
        if is_env_assignment(prev) {
            i -= 1;
            continue;
        }
        return false;
    }
}

/// `i` から次の区切りまで(区切りは含まない)。
fn segment(tokens: &[String], i: usize) -> &[String] {
    let end = tokens[i..]
        .iter()
        .position(|t| is_sep(t))
        .map_or(tokens.len(), |p| i + p);
    &tokens[i..end]
}

fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// `bash -c` / `-lc` のようなコマンド文字列を取るフラグか。
fn is_command_flag(t: &str) -> bool {
    t.starts_with('-') && !t.starts_with("--") && t.len() >= 2 && t.ends_with('c')
}

/// フラグ `names`(`-R` / `--repo` など)の値を、`--flag value`・`--flag=value`(短い綴りは `-Rvalue` も)の
/// 形からすべて取る。
fn flag_values<'a>(seg: &'a [String], short: &str, long: &str) -> Vec<&'a str> {
    let has_short = !short.is_empty();
    let long_eq = format!("{long}=");
    let mut out = Vec::new();
    let mut j = 0;
    while j < seg.len() {
        let t = seg[j].as_str();
        if t == "--" {
            break;
        }
        if t == long || (has_short && t == short) {
            if let Some(v) = seg.get(j + 1) {
                out.push(v.as_str());
                j += 2;
                continue;
            }
        } else if let Some(v) = t.strip_prefix(long_eq.as_str()) {
            out.push(v);
        } else if let Some(v) = t.strip_prefix(short).filter(|_| has_short) {
            // `-Rowner/repo`。`-R` で始まる別のフラグは、値に `/` を持つ形だけ拾う
            // (長い綴りは `--` で始まるので、ここには来ない)。
            if v.contains('/') {
                out.push(v);
            }
        }
        j += 1;
    }
    out
}

/// 位置引数でも値でも、URL の形をしたトークンから (持ち主, リポジトリ) を取る。
fn url_repos(seg: &[String]) -> Vec<(String, String)> {
    seg.iter().filter_map(|t| repo_from_url(t)).collect()
}

/// `%3A` のような、検索語に入りうる % エスケープを戻す(ASCII だけ。壊れた並びはそのまま)。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 検索語(クエリ文字列・空白区切りの語)から、`is:public` と、`user:`・`org:`・`repo:` の手がかりを取る。
fn scan_query_words(text: &str, public_only: &mut bool, scopes: &mut Vec<Scope>) {
    let decoded = percent_decode(text);
    for w in decoded.split(|c: char| c.is_whitespace() || matches!(c, '+' | '&' | '=' | '?' | ','))
    {
        if w == "is:public" {
            *public_only = true;
        } else if let Some(o) = w.strip_prefix("user:").or_else(|| w.strip_prefix("org:")) {
            if !o.is_empty() {
                scopes.push(Scope::Owner(o.to_string()));
            }
        } else if let Some(r) = w.strip_prefix("repo:") {
            if let Some((o, r)) = split_nwo(r) {
                scopes.push(Scope::Repo(o, r));
            }
        }
    }
}

// ---- `gh` 1 回の判定 ----

fn judge_gh(seg: &[String], ctx: &mut Ctx<'_>) -> Option<String> {
    let sub = seg.get(1)?.as_str();
    // ヘルプの表示は何も読まない。
    if !gh_command_at(seg, 0, &[sub]) {
        return None;
    }
    match sub {
        "search" => judge_search(seg, ctx),
        "api" => judge_api(seg, ctx),
        "status" => judge_status(seg, ctx),
        s if CWD_REPO_COMMANDS.contains(&s) => judge_repo_scoped(seg, ctx),
        _ => None,
    }
}

/// `pr`・`issue`・`repo` など。リポジトリを指していればそれを、指していなければ cwd のリポジトリを見る。
fn judge_repo_scoped(seg: &[String], ctx: &mut Ctx<'_>) -> Option<String> {
    let mut scopes: Vec<Scope> = Vec::new();
    for v in flag_values(seg, "-R", "--repo") {
        if let Some((o, r)) = split_nwo(v) {
            scopes.push(Scope::Repo(o, r));
        }
    }
    for (o, r) in url_repos(seg) {
        scopes.push(Scope::Repo(o, r));
    }
    if seg[1] == "repo" {
        if let Some(verb) = seg.get(2).map(String::as_str) {
            let rest = &seg[3..];
            let rest = &rest[..rest.iter().position(|t| t == "--").unwrap_or(rest.len())];
            let mut positional = positionals(rest);
            if REPO_VERBS.contains(&verb) {
                // 最初に `owner/repo` と読める位置引数(`gh repo clone o/r dest/dir` の `dest/dir` は読まない)。
                if let Some((o, r)) = positional.find_map(|t| split_nwo(t)) {
                    scopes.push(Scope::Repo(o, r));
                }
                for v in flag_values(rest, "", "--source") {
                    if let Some((o, r)) = split_nwo(v) {
                        scopes.push(Scope::Repo(o, r));
                    }
                }
            } else if verb == "list" {
                // `gh repo list <持ち主>`。持ち主を書かなければ、ログイン中のアカウント自身。
                if let Some(t) = positional.next() {
                    scopes.push(Scope::Owner(t.clone()));
                }
            }
        }
    }
    if scopes.is_empty() {
        // `gh repo list` のように、リポジトリに作用しない verb は cwd を見ない。
        let acts_on_cwd = !(seg[1] == "repo"
            && seg
                .get(2)
                .is_some_and(|v| v == "list" || v == "create" || v == "clone"));
        return if acts_on_cwd { ctx.check_cwd() } else { None };
    }
    scopes.iter().find_map(|s| ctx.check_scope(s))
}

/// `gh status` は、すべての持ち主の通知・メンション・レビュー依頼を集める。`-o/--org` で同じ側の持ち主に絞るときだけ可。
fn judge_status(seg: &[String], ctx: &mut Ctx<'_>) -> Option<String> {
    let orgs = flag_values(seg, "-o", "--org");
    if orgs.is_empty() {
        return Some(ctx.deny_text(
            "`gh status` は、すべての持ち主のリポジトリの通知を集めます。`--org <同じ側の持ち主>` で絞ってください。",
        ));
    }
    orgs.iter().find_map(|o| ctx.check_owner(o))
}

/// `gh search <prs|issues|code|commits|repos>`。
fn judge_search(seg: &[String], ctx: &mut Ctx<'_>) -> Option<String> {
    let mut public_only = false;
    let mut scopes: Vec<Scope> = Vec::new();
    let vis: Vec<&str> = flag_values(seg, "", "--visibility");
    if !vis.is_empty()
        && vis
            .iter()
            .all(|v| v.split(',').all(|x| x.trim() == "public"))
    {
        public_only = true;
    }
    for v in flag_values(seg, "", "--owner") {
        for o in v.split(',').filter(|o| !o.is_empty()) {
            scopes.push(Scope::Owner(o.to_string()));
        }
    }
    for v in flag_values(seg, "-R", "--repo") {
        for r in v.split(',') {
            if let Some((o, r)) = split_nwo(r) {
                scopes.push(Scope::Repo(o, r));
            }
        }
    }
    // 検索語(位置引数。`--` のあとも含む)。フラグの値も混ざるが、`is:public` などの形に当たらなければ無害。
    for t in &seg[2..] {
        scan_query_words(t, &mut public_only, &mut scopes);
    }
    decide_search(public_only, &scopes, ctx)
}

fn decide_search(public_only: bool, scopes: &[Scope], ctx: &mut Ctx<'_>) -> Option<String> {
    if public_only {
        return None;
    }
    if scopes.is_empty() {
        return Some(ctx.deny_text(
            "持ち主・リポジトリで絞らない横断検索は、別の側の持ち主の非公開リポジトリも拾います。",
        ));
    }
    scopes.iter().find_map(|s| ctx.check_scope(s))
}

/// `gh api <endpoint>`。
fn judge_api(seg: &[String], ctx: &mut Ctx<'_>) -> Option<String> {
    let mut search = false;
    let mut scopes: Vec<Scope> = Vec::new();
    let mut cross_cutting: Option<&str> = None;
    for tok in &seg[2..] {
        if tok.starts_with('-') {
            continue;
        }
        let path = tok.split('?').next().unwrap_or(tok);
        let path = format!("/{}", path.trim_start_matches('/'));
        let parts: Vec<&str> = path.split('/').collect(); // ["", "repos", o, r, …]
        match parts.get(1).copied() {
            Some("repos") => {
                if let (Some(o), Some(r)) = (parts.get(2), parts.get(3)) {
                    if *o == "{owner}" {
                        // `gh` が cwd のリポジトリに置き換える。
                        if let Some((co, cr)) = (ctx.cwd_repo)() {
                            scopes.push(Scope::Repo(co, cr));
                        }
                    } else if let Some((o, r)) = split_nwo(&format!("{o}/{r}")) {
                        scopes.push(Scope::Repo(o, r));
                    }
                }
            }
            Some("orgs") => {
                if let Some(o) = parts.get(2).filter(|o| !o.is_empty()) {
                    scopes.push(Scope::Owner((*o).to_string()));
                }
            }
            Some("search") => search = true,
            Some("notifications") => cross_cutting = Some("notifications"),
            Some("issues") if parts.len() == 2 => cross_cutting = Some("issues"),
            Some("user") if matches!(parts.get(2).copied(), Some("repos" | "issues")) => {
                cross_cutting = Some("user/repos・user/issues");
            }
            _ => {}
        }
    }
    if let Some(name) = cross_cutting {
        return Some(ctx.deny_text(&format!(
            "`gh api {name}` は、すべての持ち主のリポジトリを横断して返します。`repos/<持ち主>/<リポジトリ>/…` か、\
`gh search … --owner <同じ側の持ち主>` で絞ってください。"
        )));
    }
    if search {
        let mut public_only = false;
        let mut qscopes: Vec<Scope> = Vec::new();
        for t in &seg[2..] {
            scan_query_words(t, &mut public_only, &mut qscopes);
        }
        if let Some(d) = decide_search(public_only, &qscopes, ctx) {
            return Some(d);
        }
    }
    scopes.iter().find_map(|s| ctx.check_scope(s))
}

/// コマンド列全体を判定する。`bash -c '…'` の中身も見る。
fn judge_tokens(tokens: &[String], depth: usize, ctx: &mut Ctx<'_>) -> Option<String> {
    if depth < MAX_DEPTH {
        for i in 1..tokens.len().saturating_sub(1) {
            if is_command_flag(&tokens[i]) && SHELLS.contains(&basename(&tokens[i - 1])) {
                if let Some(inner) = parse(&tokens[i + 1]) {
                    if let Some(reason) = judge_tokens(&inner.tokens, depth + 1, ctx) {
                        return Some(reason);
                    }
                }
            }
        }
    }
    for i in 0..tokens.len() {
        if is_gh(&tokens[i]) && at_command_position(tokens, i) {
            if let Some(reason) = judge_gh(segment(tokens, i), ctx) {
                return Some(reason);
            }
        }
    }
    None
}

/// コマンド文字列を判定する。deny なら理由を返す。会社の契約の機体(`policy.side`)は何も止めない。
/// 環境に触れる 2 つ(cwd の origin・可視性の照会)は、注入する。
pub fn decide(
    cmd: &str,
    policy: &Policy,
    cwd_repo: &dyn Fn() -> Option<(String, String)>,
    visibility: &mut dyn FnMut(&str) -> Option<Visibility>,
) -> Option<String> {
    if policy.side == Side::Company {
        return None;
    }
    let parsed = parse(cmd)?;
    let mut ctx = Ctx {
        policy,
        cwd_repo,
        visibility,
    };
    judge_tokens(&parsed.tokens, 0, &mut ctx)
}

/// コマンドのどこかに `gh` があるか(表や側を読む前の、安い足切り)。
fn mentions_gh(cmd: &str) -> bool {
    // `bash -c 'gh …'` のように、1 トークンの中に空白区切りで入っている `gh` も数える(足切りは広い側に倒す)。
    parse(cmd).is_some_and(|p| {
        p.tokens
            .iter()
            .any(|t| is_gh(t) || t.split_whitespace().any(is_gh))
    })
}

/// hook 本体。`hook-router` が同じ関数を呼ぶ。入力が読めない・Bash でない・`gh` が無い・
/// この機体に持ち主の表が無い(規則を配っていない)ときは `None`。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    let call = ToolCall::parse(agent, raw)?;
    let cmd = call.bash_command()?;
    if !mentions_gh(&cmd) {
        return None;
    }
    // 契約が会社と確かめられるのは Claude だけ。Codex・Copilot は、機体がどちらでも個人として扱う。
    let side = if agent == Agent::Claude {
        machine_side()
    } else {
        Side::Personal
    };
    if side == Side::Company {
        return None;
    }
    let policy = match load_policy(side) {
        Ok(Some(p)) => p,
        Ok(None) => return None,
        Err(e) => {
            return Some(Outcome::Deny(format!(
                "持ち主の表を読めないので、`gh` を止めます(閉じる側に倒しています): {e}\n\
`~/.config/dotfiles/github-owners.toml` を直してください(正本は非公開の設定リポジトリで、`hms` が配ります)。"
            )))
        }
    };
    let cwd = call
        .raw
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    let cwd_repo = move || cwd.as_deref().and_then(|d: &Path| origin_repo(d));
    decide(&cmd, &policy, &cwd_repo, &mut |nwo| visibility_of(nwo)).map(Outcome::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;
    use repo_scope::Owners;

    // 試験に使う名前は、すべて架空(実在の持ち主の名前をここに書かない。ADR-0034)。
    const TABLE: &str = r#"
[owners]
me = "personal"
friends-org = "personal"
acme-corp = "company"
"#;

    fn policy(side: Side) -> Policy {
        Policy {
            side,
            owners: Owners::parse(TABLE).unwrap(),
        }
    }

    fn visibility(nwo: &str) -> Option<Visibility> {
        match nwo {
            "acme-corp/open" | "stranger/oss" => Some(Visibility::Public),
            "acme-corp/secret" | "stranger/priv" => Some(Visibility::NonPublic),
            _ => None,
        }
    }

    fn run_in(cmd: &str, side: Side, cwd: Option<(&str, &str)>) -> Option<String> {
        let cwd = cwd.map(|(o, r)| (o.to_string(), r.to_string()));
        decide(cmd, &policy(side), &|| cwd.clone(), &mut |n| visibility(n))
    }

    fn run(cmd: &str) -> Option<String> {
        run_in(cmd, Side::Personal, Some(("me", "dotfiles")))
    }

    fn denied(cmd: &str) -> bool {
        run(cmd).is_some()
    }

    #[test]
    fn denies_private_repos_of_the_other_side_and_of_unknown_owners() {
        for c in [
            "gh pr list -R acme-corp/secret",
            "gh pr list --repo acme-corp/secret --state open",
            "gh pr list --repo=acme-corp/secret",
            "gh pr list -Racme-corp/secret",
            "gh pr view 12 -R github.com/acme-corp/secret",
            "gh issue list -R stranger/priv",
            "gh pr view https://github.com/acme-corp/secret/pull/3",
            "gh api repos/acme-corp/secret/pulls",
            "gh api /repos/acme-corp/secret/issues --paginate",
            "gh api -X GET repos/acme-corp/secret/pulls --jq '.[].title'",
            "gh repo view acme-corp/secret",
            "gh repo view --json name acme-corp/secret",
            "gh repo clone acme-corp/secret",
            "gh run view 99 -R acme-corp/secret",
            "gh repo sync me/x --source acme-corp/secret",
            "gh repo list acme-corp",
        ] {
            let r = run(c);
            assert!(
                r.as_deref().is_some_and(|m| m.contains("非公開")),
                "{c}: {r:?}"
            );
        }
    }

    #[test]
    fn denies_when_visibility_cannot_be_confirmed() {
        for c in [
            "gh pr list -R acme-corp/unknown",
            "gh api repos/stranger/gone/pulls",
        ] {
            let r = run(c);
            assert!(
                r.as_deref()
                    .is_some_and(|m| m.contains("確かめられませんでした")),
                "{c}: {r:?}"
            );
        }
    }

    #[test]
    fn allows_the_own_side_and_public_repos() {
        for c in [
            "gh pr list -R me/dotfiles",
            "gh pr list -R ME/dotfiles",
            "gh pr view https://github.com/friends-org/x/pull/1",
            "gh api repos/me/dotfiles/pulls",
            "gh pr list -R acme-corp/open",
            "gh pr list -R stranger/oss",
            "gh repo view stranger/oss",
            "gh repo clone me/x ~/dest/dir",
            "gh api graphql -f query='{ viewer { login } }'",
            "gh api user",
            "gh api rate_limit",
            "gh auth status",
            "gh repo list",
            "gh repo list me",
            "gh repo list --limit 5 friends-org",
            "gh repo list friends-org --limit 5",
            "gh repo view --json name,visibility",
            "gh repo create new-thing --private",
            "gh gist list",
            "gh --version",
        ] {
            assert_eq!(run(c), None, "{c}");
        }
    }

    #[test]
    fn repo_scoped_commands_without_a_repo_use_the_cwd_origin() {
        let in_cwd = |c: &str, cwd: (&str, &str)| run_in(c, Side::Personal, Some(cwd));
        assert_eq!(in_cwd("gh pr list", ("me", "dotfiles")), None);
        assert_eq!(in_cwd("gh pr list", ("stranger", "oss")), None);
        assert!(in_cwd("gh pr list", ("acme-corp", "secret")).is_some());
        assert!(in_cwd("gh issue create --title x", ("acme-corp", "secret")).is_some());
        assert!(in_cwd("gh repo view", ("acme-corp", "secret")).is_some());
        // `{owner}`・`{repo}` は cwd のリポジトリに置き換わる
        assert_eq!(
            in_cwd("gh api repos/{owner}/{repo}/pulls", ("me", "x")),
            None
        );
        assert!(in_cwd("gh api repos/{owner}/{repo}/pulls", ("acme-corp", "secret")).is_some());
        // 明示の `-R` が cwd より優先され、cwd は見ない
        assert_eq!(in_cwd("gh pr list -R me/x", ("acme-corp", "secret")), None);
        // cwd の origin が引けない(git の外)ときは、止めない
        assert_eq!(run_in("gh pr list", Side::Personal, None), None);
    }

    #[test]
    fn cross_repo_search_must_be_scoped() {
        for c in [
            // 2026-11-15 に、絞らずに打って、会社側の非公開の PR の題名を表示した形
            "gh search prs --author @me --state open --limit 100 --json repository,number,title",
            "gh search prs --owner acme-corp",
            "gh search prs --owner me,acme-corp",
            "gh search prs --owner me --owner acme-corp",
            "gh search issues repo:acme-corp/secret bug",
            "gh search issues --repo acme-corp/secret",
            "gh search code user:acme-corp foo",
            "gh search repos --owner stranger",
            "gh search prs --visibility public,private --owner acme-corp",
            "gh search prs --visibility private",
        ] {
            assert!(denied(c), "{c}");
        }
        for c in [
            "gh search prs --author @me --owner me",
            "gh search prs --author @me --owner me --owner friends-org",
            "gh search prs --author @me --visibility public",
            "gh search prs --author @me --visibility=public",
            "gh search prs --author @me is:public",
            "gh search prs is:public author:@me --owner acme-corp",
            "gh search issues repo:me/dotfiles bug",
            "gh search issues --repo acme-corp/open",
            "gh search code org:friends-org foo",
            "gh search repos --owner @me",
            "gh search prs --help",
        ] {
            assert_eq!(run(c), None, "{c}");
        }
    }

    #[test]
    fn api_search_and_cross_cutting_endpoints() {
        for c in [
            "gh api search/issues -f q='is:pr author:@me'",
            "gh api 'search/issues?q=user:acme-corp'",
            "gh api search/issues -f q='repo:acme-corp/secret'",
            "gh api search/issues?q=org%3Aacme-corp",
            "gh api user/repos",
            "gh api /user/repos --paginate",
            "gh api user/issues",
            "gh api issues",
            "gh api notifications",
            "gh api orgs/acme-corp/repos",
            "gh api orgs/stranger/repos",
            "gh status",
            "gh status --exclude me/x",
            "gh status --org acme-corp",
        ] {
            assert!(denied(c), "{c}");
        }
        for c in [
            "gh api search/issues -f q='is:public author:@me'",
            "gh api 'search/issues?q=is:public+author:@me'",
            "gh api search/issues -f q='user:me author:@me'",
            "gh api orgs/friends-org/repos",
            "gh api user/orgs",
            "gh status --org me",
            "gh status -o friends-org",
        ] {
            assert_eq!(run(c), None, "{c}");
        }
    }

    #[test]
    fn finds_gh_in_compound_commands_and_nested_shells() {
        for c in [
            "cd /tmp && gh pr list -R acme-corp/secret",
            "echo hi; gh search prs --author @me",
            "gh pr list -R me/x | gh pr view -R acme-corp/secret",
            "GH_PAGER= gh pr list -R acme-corp/secret",
            "FOO=1 BAR=2 gh pr list -R acme-corp/secret",
            "for r in a b; do gh pr list -R acme-corp/secret; done",
            "bash -c 'gh pr list -R acme-corp/secret'",
            "bash -lc \"gh search prs --author @me\"",
            "/usr/bin/gh pr list -R acme-corp/secret",
            "gh pr list -R me/x && gh pr list -R acme-corp/secret",
        ] {
            assert!(denied(c), "{c}");
        }
    }

    #[test]
    fn quoted_mentions_and_other_commands_are_not_judged() {
        for c in [
            r#"git commit -m "docs: gh pr list -R acme-corp/secret を試した""#,
            r#"echo "gh search prs --author @me""#,
            r#"rg -n "gh api user/repos" docs"#,
            "ls -R acme-corp/secret",
            "git -C x remote add origin acme-corp/secret",
            "gh pr create --title 'fix' --body 'acme-corp/secret の話'",
            "git commit -F - <<'EOF'\nfix: gh search prs --author @me\nEOF",
            "",
            // トークン化できない(unmatched quote)ものは、判定不能として通す
            "gh pr list -R acme-corp/secret 'x",
        ] {
            assert_eq!(run(c), None, "{c}");
        }
    }

    #[test]
    fn company_machine_reads_everything() {
        for c in [
            "gh pr list -R acme-corp/secret",
            "gh pr list -R me/private-thing",
            "gh search prs --author @me",
            "gh api user/repos",
        ] {
            assert_eq!(
                run_in(c, Side::Company, Some(("acme-corp", "secret"))),
                None,
                "{c}"
            );
        }
    }

    #[test]
    fn the_deny_message_names_the_reason_and_how_to_proceed() {
        let m = run("gh pr list -R acme-corp/secret").unwrap();
        assert!(
            m.contains("`acme-corp/secret` は非公開リポジトリです。"),
            "{m}"
        );
        assert!(m.contains("--visibility public"), "{m}");
        assert!(m.contains("is:public"), "{m}");
        assert!(m.contains("逃げ道の環境変数はありません"), "{m}");
        let m = run("gh search prs --author @me").unwrap();
        assert!(m.contains("横断検索"), "{m}");
    }

    #[test]
    fn percent_decoding_is_ascii_only_and_tolerates_broken_input() {
        assert_eq!(percent_decode("org%3Aacme-corp"), "org:acme-corp");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%3"), "%3");
    }

    #[test]
    fn hook_reads_bash_only_and_skips_commands_without_gh() {
        let bash = |c: &str| {
            serde_json::json!({"tool_name":"Bash","tool_input":{"command":c},"cwd":"/nonexistent"})
                .to_string()
        };
        // `gh` を含まないコマンド・Bash でない tool・壊れた JSON は、表も側も読まずに `None`。
        assert!(hook(Agent::Claude, &bash("ls -la")).is_none());
        let write = r#"{"tool_name":"Write","tool_input":{"command":"gh search prs"}}"#;
        assert!(hook(Agent::Claude, write).is_none());
        assert!(hook(Agent::Claude, "{").is_none());
    }
}
