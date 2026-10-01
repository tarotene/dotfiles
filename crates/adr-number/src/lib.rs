//! `gh pr create` の直後に ADR-0000(起草中)を PR 番号へ自動改番する
//! PostToolUse hook(ADR-380、docs/claude/adr-numbering.md)。
//!
//! bash 版 `config/claude/hooks/adr-number.sh`(+ Codex adapter)の移植。
//!
//! 段 2(scripts/adr-number-check + CI required check)だけで採番衝突は既に
//! 構造的に不可能になっている。この hook は段 3 の利便性層に過ぎず、何も
//! deny しない — commit を跨いだフォローアップ push が要る事実だけを
//! additionalContext で伝え、忘れても CI が red になって気づける。
//!
//! opt-in: 対象プロジェクトに `scripts/adr-number-check`(governance
//! テンプレートが播く bash 複製)が無ければ何もしない。checker 自体は他
//! リポジトリに播かれる単一ソースなので bash のまま(Rust 化しない)で、
//! この hook は従来どおり対象プロジェクトのそれを子プロセスとして呼ぶ。
//!
//! 早期 exit: `docs/adr/0000-*.md` が無ければ stdin を読まずに終わる
//! (プロジェクト dir が環境変数で分かるときだけ)。
//!
//! PR 番号は tool_response を読まず `gh pr view --json number,baseRefName`
//! で解決する(スキーマ非依存にして `--web`・ブラウザ作成でも効かせる)。

use guard_core::command::{self, gh_command_at};
use guard_core::hook::{jq_r_path, ToolCall};
use hook_io::jqfmt::J;
use hook_io::proc::command_exists;
use hook_io::Agent;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// プロジェクト dir を示す環境変数(bash 版: Claude は `CLAUDE_PROJECT_DIR`、
/// Codex adapter は `CODEX_PROJECT_DIR`)。
fn project_env_var(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "CODEX_PROJECT_DIR",
        Agent::Claude | Agent::Copilot => "CLAUDE_PROJECT_DIR",
    }
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// 適用対象か: 起草中 ADR(`docs/adr/0000-*.md`)があり、かつプロジェクトが
/// 自前の `scripts/adr-number-check` を持つ(opt-in)。bash の `has_draft`。
pub fn has_draft(project: &Path) -> bool {
    if !is_executable(&project.join("scripts/adr-number-check")) {
        return false;
    }
    std::fs::read_dir(project.join("docs/adr"))
        .map(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with("0000-") && n.ends_with(".md")
            })
        })
        .unwrap_or(false)
}

/// 対象プロジェクト自身の `scripts/adr-number-check` を解決する。呼び出しの
/// たびに解決する。`ADR_NUMBER_CHECK_BIN` が set のときはテスト用の差し替えと
/// して最終決定扱い(実行できなければ `None`)。PATH 上の配備済みバイナリや
/// dotfiles source tree へのフォールバックは適用判定に使わない(使うと
/// ADR-380 方式でないリポジトリの `docs/adr/0000-template.md` まで改番する)。
pub fn resolve_adr_number_check(project: &Path) -> Option<PathBuf> {
    if let Some(bin) = std::env::var_os("ADR_NUMBER_CHECK_BIN").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(bin);
        return is_executable(&p).then_some(p);
    }
    let p = project.join("scripts/adr-number-check");
    is_executable(&p).then_some(p)
}

/// `gh pr create` がコマンド位置に 1 つでもあるか(bash の
/// `command_ran_pr_create`)。判定不能(unmatched quote 等)は偽。
pub fn command_ran_pr_create(cmd: &str) -> bool {
    command::parse(cmd)
        .map(|p| {
            !p.ranges(|t, i| gh_command_at(t, i, &["pr", "create"]).then_some(()))
                .is_empty()
        })
        .unwrap_or(false)
}

/// PostToolUse の additionalContext 出力(bash の `emit_context`、jq と同じ
/// バイト列・末尾改行込み)。
pub fn context_output(pr_number: &str, fix_out: &str) -> String {
    let msg = format!(
        "ADR-0000 を PR #{pr_number} の番号へ改番しました({fix_out})。commit して push してください。"
    );
    let v = J::obj(vec![(
        "hookSpecificOutput",
        J::obj(vec![
            ("hookEventName", J::str("PostToolUse")),
            ("additionalContext", J::str(msg)),
        ]),
    )]);
    format!("{}\n", v.pretty())
}

/// `$(...)` と同じく末尾の改行を落とす。
fn strip_trailing_newlines(mut s: String) -> String {
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

/// `gh pr view --json number,baseRefName --jq '"\(.number) \(.baseRefName)"'`
/// の結果を (番号, base) に割る(bash: `read -r pr_number pr_base`)。
fn pr_info(project: &Path) -> Option<(String, Option<String>)> {
    let out = Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "number,baseRefName",
            "--jq",
            r#""\(.number) \(.baseRefName)""#,
        ])
        .current_dir(project)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    // `read -r` は最初の行だけを読み、空白(IFS)で 2 語に割る。
    let line = s.lines().next().unwrap_or("");
    let mut it = line.split_whitespace();
    let number = it.next()?.to_string();
    let rest: Vec<&str> = it.collect();
    let base = (!rest.is_empty()).then(|| rest.join(" "));
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some((number, base))
}

/// checker を `--fix <PR>`(+ `--base origin/<base>`、#644)で呼び、stdout と
/// stderr を混ぜた出力を返す(bash の `2>&1`)。失敗(非 0)は `None`。
/// 混ぜ方を bash と揃えるため sh の `2>&1` に任せる(両 fd が同じ pipe)。
fn run_fix(project: &Path, checker: &Path, args: &[String]) -> Option<String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(r#"exec "$0" "$@" 2>&1"#)
        .arg(checker)
        .args(args)
        .current_dir(project)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(strip_trailing_newlines(
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// hook 本体。出力すべき JSON(あれば)を返す。stdin は必要になるまで読まない。
pub fn run(agent: Agent, stdin: &mut impl Read) -> Option<String> {
    // 最安の早期 exit: プロジェクト dir が環境変数で分かっていれば、stdin を
    // 読む前に draft の有無だけ見て終わる。
    let env_project = std::env::var(project_env_var(agent))
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(p) = &env_project {
        if !has_draft(Path::new(p)) {
            return None;
        }
    }

    let mut buf = String::new();
    stdin.read_to_string(&mut buf).ok()?;
    let call = ToolCall::parse(agent, &buf)?;
    let cmd = call.bash_command()?;

    let project = match env_project {
        Some(p) => p,
        None => jq_r_path(&call.raw, &["cwd"]).ok().flatten()?,
    };
    if project.is_empty() {
        return None;
    }
    let project = PathBuf::from(project);
    hook_io::git::toplevel(&project)?;

    if !has_draft(&project) || !command_ran_pr_create(&cmd) {
        return None;
    }
    let checker = resolve_adr_number_check(&project)?;
    if !command_exists("gh") {
        return None;
    }
    let (pr_number, pr_base) = pr_info(&project)?;
    let mut fix_args = vec!["--fix".to_string(), pr_number.clone()];
    if let Some(b) = pr_base {
        fix_args.push("--base".into());
        fix_args.push(format!("origin/{b}"));
    }
    let fix_out = run_fix(&project, &checker, &fix_args)?;
    Some(context_output(&pr_number, &fix_out))
}
