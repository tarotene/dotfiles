//! `apply-rulesets.sh` — GitHub リポジトリ自身が宣言した ruleset
//! (`<repo>/.github/rulesets/{security,quality,workflow}[,review].json`)を、
//! そのリポジトリの live な branch ruleset に適用する。
//! `scripts/apply-rulesets.sh` の Rust 移植(ADR-0024、#414)。
//!
//! 設計と根拠(ADR-503):
//! required_status_checks の正本を「対象リポジトリの外(このリポジトリの
//! `*-repo-governance` skill テンプレート)」に置いていたことが telepath#243 を
//! 含む複数リポジトリの BLOCKED 事故の直接原因だった — テンプレートの required
//! context が対象リポジトリの実際のジョブ名と同じ PR で編集される保証が無い
//! ため。
//!
//! この道具は「型」を引数に取らない(旧: rust/typst/astro/core/dotfiles の 5 型、
//! 3 skill それぞれの apply-rulesets.sh、共通コア `_rulesets-apply-core.sh` の
//! 4 層構成だった)。正本を対象リポジトリの `.github/rulesets/*.json` そのものに
//! 一本化したので、apply はどのリポジトリに対しても同じ処理で済む(D4「還元」)。
//!
//! PUT/POST の直前に、quality.json の `required_status_checks[].context` を
//! 「検証対象コミットが実際に報告する job 名」と突合する(D5)。ここでの「検証
//! 対象コミット」は既定で対象 ref に対する最新 PR の head SHA — default
//! branch の squash commit 自体には pull_request 系の run が存在しないため
//! (D14、`on.pull_request` トリガーは push を発火しない)。
//!
//! # bash 版との差
//!
//! - `--selftest` は持たない(テストは `tests/*.rs`、`cargo test -p apply-rulesets`)。
//! - `gh api --jq` / `jq` / `base64` に頼らず、`gh api` の JSON を自前で読む
//!   (実行時依存は `gh` だけ)。`gh` の呼び出し(パス・`-X`・`-F`・`--input -`)
//!   は bash 版と同一なので、書込み系の PreToolUse hook(`rulesets-write-guard`)
//!   と、テストの `gh` スタブ(パス→fixture)の規約はそのまま。
//! - `GITHUB_AUDIT_GH_BIN`(既定 `gh`)は bash 版が source していた
//!   `scripts/github-audit` の `GH_BIN` と同じ環境変数。
//! - 宣言 JSON が壊れていたとき、bash 版は `jq` のエラーでそのまま落ちたが、
//!   ここでは診断を出して exit 1 にする。

pub mod base64;
pub mod multi;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

const USAGE: &str = "\
usage: apply-rulesets.sh <owner/repo> [--ref REF] [--from-dir DIR]
                          [--verify-sha SHA] [--reconcile] [--dry-run]
                          [--unverified-contexts] [--delete-ruleset NAME]

対象リポジトリ自身が .github/rulesets/{security,quality,workflow}[,review].json
として持つ宣言を、そのリポジトリの live な branch ruleset に適用する。

宣言の取得元:
  既定          repos/<owner>/<repo>/contents/.github/rulesets を --ref
                (省略時は default branch)から読む。
  --from-dir    ローカルディレクトリから読む(seed 直後、まだ commit/push
                していない宣言を検証したいとき用)。

検証(D5): quality.json の required_status_checks[].context を、検証対象
コミットが実際に報告する job 名(Actions API 実測、YAML は静的パースしない
— D3)と突合する。報告されない context があれば拒否する(exit 4)。
検証対象コミットは既定で --ref に対する最新 PR の head SHA
(--verify-sha で明示指定可、D14: default branch の squash commit 自体には
pull_request 系の run が存在しないため)。--unverified-contexts で拒否を
スキップできる(seed 直後、まだ PR が無い時専用の明示的な脱出)。

--reconcile 無し: 名前一致で既存 ruleset を skip(create-only)。
--reconcile 有り: 名前一致で PUT(更新)。宣言に無い active な branch
  ruleset は報告のみ(削除しない)。

--delete-ruleset NAME: 宣言に同名が無いことを確認した上でその ruleset を
  DELETE する(旧 governance skill 群の --remove-review の後継。review 層
  を剥がすときに使う: review.json を宣言から消してから
  `--delete-ruleset Review` を実行する)。

rulesets-write-guard(PreToolUse hook)が deny するのは、Claude が直に発行する
`gh api -X POST/PUT/PATCH/DELETE repos/O/R/rulesets[/id]` だけである。この
コマンド自体(`apply-rulesets ...` の 1 回の呼び出し)は guard の対象外なので、
RULESETS_WRITE_GUARD_BYPASS=1 を前置する必要はない。auto モードでは、前置すると
分類器が safety bypass と判定して拒否する(#707)。
";

// ---------------------------------------------------------------------------
// gh api
// ---------------------------------------------------------------------------

struct Gh {
    bin: String,
}

impl Gh {
    fn from_env() -> Self {
        Gh {
            bin: std::env::var("GITHUB_AUDIT_GH_BIN")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "gh".to_string()),
        }
    }

    /// `gh api <args…>`(stderr は捨てる = bash の `2>/dev/null`)。終了コード 0 の
    /// ときだけ stdout を返す。JSON として読めなければ `None` ではなく生文字列。
    fn read(&self, args: &[&str]) -> Option<String> {
        let out = Command::new(&self.bin)
            .arg("api")
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn read_json(&self, args: &[&str]) -> Option<Value> {
        serde_json::from_str(&self.read(args)?).ok()
    }

    /// 書込み系(POST/PUT/DELETE)。失敗は gh の終了コードを返す(bash 版は
    /// `set -e` でそのコードのまま落ちていた)。stderr は素通し。
    /// guard の bypass 用環境変数は立てない: guard は Claude の Bash 呼び出しの
    /// コマンド文字列だけを見るので、この子プロセスの環境変数は誰にも読まれない(#707)。
    fn write(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, i32> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["api", "-X", method, path]).stdout(Stdio::piped());
        if body.is_some() {
            cmd.args(["--input", "-"]).stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        let mut child = cmd.spawn().map_err(|e| {
            eprintln!("ERROR: {}: {e}", self.bin);
            127
        })?;
        if let (Some(body), Some(mut stdin)) = (body, child.stdin.take()) {
            // `echo "$body" |` と同じく末尾に改行を足す。gh が読み終えずに落ちて
            // EPIPE になっても、判定は終了コードに任せる。
            let _ = stdin.write_all(body.as_bytes());
            let _ = stdin.write_all(b"\n");
        }
        let out = child.wait_with_output().map_err(|_| 1)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(out.status.code().unwrap_or(1))
        }
    }
}

/// jq の `-r` 相当(文字列は裸で、null は `null`、他は JSON 表記)。
fn raw(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "null".to_string(),
    }
}

fn names_of(v: &[String]) -> String {
    v.join(" ")
}

// ---------------------------------------------------------------------------
// 実測 job 名(scripts/github-audit の fetch_run_job_names / fetch_head_sha_job_names)
// ---------------------------------------------------------------------------

fn fetch_run_job_names(gh: &Gh, owner: &str, repo: &str, run_id: &str) -> Vec<String> {
    let path = format!("repos/{owner}/{repo}/actions/runs/{run_id}/jobs");
    gh.read_json(&[&path])
        .and_then(|v| {
            v.get("jobs").and_then(Value::as_array).map(|jobs| {
                jobs.iter()
                    .filter_map(|j| j.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// 特定コミット(`sha`)に紐づく、あらゆる workflow の run が実際に報告した
/// job 名の和集合(昇順・重複なし)。run が複数の workflow に渡ることがある
/// (reusable workflow・matrix・複数 push トリガーの workflow が同じコミットで
/// 並走する)実態を吸収する。取得失敗は `[]`。
fn fetch_head_sha_job_names(gh: &Gh, owner: &str, repo: &str, sha: &str) -> Vec<String> {
    let path = format!("repos/{owner}/{repo}/actions/runs");
    let head = format!("head_sha={sha}");
    let run_ids: Vec<String> = gh
        .read_json(&[&path, "-X", "GET", "-F", &head, "-F", "per_page=100"])
        .and_then(|v| {
            v.get("workflow_runs")
                .and_then(Value::as_array)
                .map(|runs| {
                    runs.iter()
                        .filter_map(|r| r.get("id").map(|id| raw(Some(id))))
                        .collect()
                })
        })
        .unwrap_or_default();
    let mut names = std::collections::BTreeSet::new();
    for id in run_ids {
        names.extend(fetch_run_job_names(gh, owner, repo, &id));
    }
    names.into_iter().collect()
}

// ---------------------------------------------------------------------------
// 宣言の読み込みと検査
// ---------------------------------------------------------------------------

/// ファイル名(`security.json` …)→ 本文。`BTreeMap` なので走査順は
/// bash の `"$dir"/*.json` グロブと同じ昇順。
type Decls = BTreeMap<String, String>;

const DECL_NAMES: [&str; 4] = ["security", "quality", "workflow", "review"];

/// 本文に `__UPPER_SNAKE__` 形(`__[A-Z_]+__`)が残っているか。
fn has_placeholder(text: &str) -> bool {
    let b = text.as_bytes();
    let is_body = |c: u8| c.is_ascii_uppercase() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"__") {
            // `__` で始まり、[A-Z_]+ が 1 文字以上続いて `__` で終わる部分列を探す。
            let mut j = i + 2;
            while j < b.len() && is_body(b[j]) {
                j += 1;
            }
            // 走査範囲は b[i+2 .. j](すべて body 文字)。`__` が b[k..k+2] に
            // あって k >= i+3(中身が 1 文字以上)かつ k+2 <= j なら一致。
            if (i + 3..j.saturating_sub(1)).any(|k| b[k..].starts_with(b"__")) {
                return true;
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    false
}

/// 宣言を読む。`security`/`quality`/`workflow` が揃わなければ exit 3。
fn load_declarations(
    gh: &Gh,
    owner_repo: &str,
    reference: &str,
    from_dir: &str,
) -> Result<Decls, i32> {
    let mut decls = Decls::new();
    if !from_dir.is_empty() {
        for name in DECL_NAMES {
            let f = Path::new(from_dir).join(format!("{name}.json"));
            if let Ok(bytes) = std::fs::read(&f) {
                decls.insert(
                    format!("{name}.json"),
                    String::from_utf8_lossy(&bytes).into_owned(),
                );
            }
        }
    } else {
        let list_path = format!("repos/{owner_repo}/contents/.github/rulesets");
        let ref_flag = format!("ref={reference}");
        let names: Vec<String> = gh
            .read_json(&[&list_path, "-X", "GET", "-F", &ref_flag])
            .and_then(|v| {
                v.as_array().map(|a| {
                    a.iter()
                        .filter_map(|e| e.get("name").and_then(Value::as_str).map(str::to_string))
                        .collect()
                })
            })
            .unwrap_or_default();
        for name in names {
            if !DECL_NAMES.iter().any(|d| name == format!("{d}.json")) {
                continue;
            }
            let path = format!("{list_path}/{name}");
            let content = gh
                .read_json(&[&path, "-X", "GET", "-F", &ref_flag])
                .and_then(|v| v.get("content").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            // `base64 -d … || true`: 壊れた base64 でもファイルは(途中まで)残る。
            let (bytes, _) = base64::decode_lenient(&content);
            decls.insert(name, String::from_utf8_lossy(&bytes).into_owned());
        }
    }

    let missing: Vec<String> = ["security", "quality", "workflow"]
        .iter()
        .map(|n| format!("{n}.json"))
        .filter(|f| !decls.contains_key(f))
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "ERROR: {owner_repo}@{reference} has no .github/rulesets/ declaration (missing: {}).",
            names_of(&missing)
        );
        eprintln!(
            "  seed it first (copy-files.sh of the matching *-repo-governance skill, or copy"
        );
        eprintln!("  .github/rulesets/ by hand), then re-run.");
        return Err(3);
    }
    Ok(decls)
}

fn check_no_placeholders(decls: &Decls) -> Result<(), i32> {
    for (name, body) in decls {
        if has_placeholder(body) {
            eprintln!(
                "ERROR: {name} still has an unreplaced placeholder (__X__) after substitution."
            );
            return Err(5);
        }
    }
    Ok(())
}

fn parse_decl(name: &str, body: &str) -> Result<Value, i32> {
    serde_json::from_str(body).map_err(|e| {
        eprintln!("ERROR: {name} is not valid JSON: {e}");
        1
    })
}

fn decl_name(file: &str, body: &str) -> Result<String, i32> {
    Ok(raw(parse_decl(file, body)?.get("name")))
}

/// quality.json の `required_status_checks[].context`(昇順・重複なし)。
/// 読めなければ `[]`(bash: `jq … || printf '[]'`)。
fn declared_contexts(quality_json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(quality_json) else {
        return Vec::new();
    };
    let mut out = std::collections::BTreeSet::new();
    for rule in v
        .get("rules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if rule.get("type").and_then(Value::as_str) != Some("required_status_checks") {
            continue;
        }
        if let Some(checks) = rule
            .pointer("/parameters/required_status_checks")
            .and_then(Value::as_array)
        {
            out.extend(
                checks
                    .iter()
                    .filter_map(|c| c.get("context").and_then(Value::as_str).map(str::to_string)),
            );
        }
    }
    out.into_iter().collect()
}

fn minus(contexts: &[String], jobs: &[String]) -> Vec<String> {
    contexts
        .iter()
        .filter(|c| !jobs.contains(c))
        .cloned()
        .collect()
}

/// 全 context が報告可能なら 0。sha 無しは 2、報告不能があれば 1(診断は stderr)。
fn verify_contexts(gh: &Gh, owner: &str, repo: &str, sha: &str, contexts: &[String]) -> i32 {
    if sha.is_empty() {
        eprintln!("  (検証対象コミットが無いため context の実測を行いません)");
        return 2;
    }
    let jobs = fetch_head_sha_job_names(gh, owner, repo, sha);
    let unreportable = minus(contexts, &jobs);
    if !unreportable.is_empty() {
        eprintln!("  以下の required context は {sha} の実測 job 名に含まれません:");
        for c in &unreportable {
            eprintln!("    - {c}");
        }
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------
// 適用
// ---------------------------------------------------------------------------

struct Opts {
    reference: String,
    from_dir: String,
    verify_sha: String,
    reconcile: bool,
    dry_run: bool,
    unverified_contexts: bool,
    delete_ruleset: String,
}

/// 既存 ruleset 一覧のうち `name` に一致する最初の id(jq の `-r` 表記)。
fn existing_id(existing: &[Value], name: &str) -> Option<String> {
    existing
        .iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
        .map(|r| raw(r.get("id")))
}

fn apply_one_ruleset(
    gh: &Gh,
    owner_repo: &str,
    existing: &[Value],
    file: &str,
    body: &str,
    opts: &Opts,
) -> Result<String, i32> {
    let name = decl_name(file, body)?;
    let Some(id) = existing_id(existing, &name) else {
        if opts.dry_run {
            println!("  DRY-RUN: would POST ruleset '{name}'");
        } else {
            let result = gh.write("POST", &format!("repos/{owner_repo}/rulesets"), Some(body))?;
            let id = serde_json::from_str::<Value>(&result)
                .map(|v| raw(v.get("id")))
                .unwrap_or_else(|_| "null".to_string());
            println!("  ✓  Created '{name}' (id={id})");
        }
        return Ok(name);
    };

    if !opts.reconcile {
        println!(
            "  ⚠   '{name}' already exists (id={id}) — skipping (pass --reconcile to update)."
        );
        return Ok(name);
    }
    if opts.dry_run {
        println!("  DRY-RUN: would PUT '{name}' (id={id})");
    } else {
        gh.write(
            "PUT",
            &format!("repos/{owner_repo}/rulesets/{id}"),
            Some(body),
        )?;
        println!("  ✓  Reconciled '{name}' (id={id})");
    }
    Ok(name)
}

/// 新たに required になった context について、その context を報告しない open PR
/// を列挙して警告する(bleep#32 型の時間差クラス — required 追加前に開いた PR の
/// head には、新しく required にした workflow が走っていないので、apply 後も
/// そのまま BLOCKED になる)。
fn warn_open_prs_missing_contexts(gh: &Gh, owner_repo: &str, contexts: &[String]) {
    let (owner, repo) = split_owner_repo(owner_repo);
    let prs = gh
        .read_json(&[
            &format!("repos/{owner_repo}/pulls"),
            "-X",
            "GET",
            "-F",
            "state=open",
            "-F",
            "per_page=50",
        ])
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    for pr in prs {
        let number = raw(pr.get("number"));
        let title = raw(pr.get("title"));
        let sha = pr
            .pointer("/head/sha")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let jobs = fetch_head_sha_job_names(gh, owner, repo, sha);
        let missing = minus(contexts, &jobs);
        if !missing.is_empty() {
            eprintln!(
                "  ⚠   PR #{number} ({title}) の head には次の required context が走っていません — rebase して再 push してください:"
            );
            for c in &missing {
                eprintln!("      - {c}");
            }
        }
    }
}

/// bash の `${owner_repo%%/*}` と `${owner_repo#*/}`。
fn split_owner_repo(owner_repo: &str) -> (&str, &str) {
    owner_repo
        .split_once('/')
        .unwrap_or((owner_repo, owner_repo))
}

fn parse_opts(args: &[String]) -> Result<Opts, i32> {
    let mut o = Opts {
        reference: String::new(),
        from_dir: String::new(),
        verify_sha: String::new(),
        reconcile: false,
        dry_run: false,
        unverified_contexts: false,
        delete_ruleset: String::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| -> Result<String, i32> {
            it.next().cloned().ok_or_else(|| {
                eprintln!("ERROR: {flag} requires a value");
                1
            })
        };
        match a.as_str() {
            "--ref" => o.reference = value("--ref")?,
            "--from-dir" => o.from_dir = value("--from-dir")?,
            "--verify-sha" => o.verify_sha = value("--verify-sha")?,
            "--reconcile" => o.reconcile = true,
            "--dry-run" => o.dry_run = true,
            "--unverified-contexts" => o.unverified_contexts = true,
            "--delete-ruleset" => o.delete_ruleset = value("--delete-ruleset")?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Err(0);
            }
            other => {
                eprintln!("Unknown option: {other}");
                return Err(2);
            }
        }
    }
    Ok(o)
}

fn run_inner(args: &[String]) -> Result<(), i32> {
    let Some(owner_repo) = args.first() else {
        eprint!("{USAGE}");
        return Err(2);
    };
    if !owner_repo.contains('/') {
        eprintln!("ERROR: '{owner_repo}' is not owner/repo");
        return Err(2);
    }
    let opts = parse_opts(&args[1..])?;
    let gh = Gh::from_env();
    // bash 版は `command -v gh`(PATH 上)を見ていた。差し替え(GITHUB_AUDIT_GH_BIN)
    // 時はその実体を見る。
    if !hook_io::proc::command_exists(&gh.bin) {
        eprintln!("ERROR: 'gh' not found");
        return Err(1);
    }
    let (owner, repo) = split_owner_repo(owner_repo);

    let mut reference = opts.reference.clone();
    if reference.is_empty() {
        reference = gh
            .read_json(&[&format!("repos/{owner_repo}")])
            .and_then(|v| {
                v.get("default_branch")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
    }
    if reference.is_empty() {
        eprintln!("ERROR: could not resolve default branch for {owner_repo}");
        return Err(1);
    }

    let decls = load_declarations(&gh, owner_repo, &reference, &opts.from_dir)?;
    check_no_placeholders(&decls)?;

    if !opts.delete_ruleset.is_empty() {
        return delete_ruleset(&gh, owner_repo, &decls, &opts);
    }

    let sha = if opts.verify_sha.is_empty() {
        gh.read_json(&[
            &format!("repos/{owner_repo}/pulls"),
            "-X",
            "GET",
            "-F",
            "state=all",
            "-F",
            "sort=updated",
            "-F",
            "direction=desc",
            "-F",
            "per_page=1",
        ])
        .and_then(|v| {
            v.pointer("/0/head/sha")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
    } else {
        opts.verify_sha.clone()
    };

    let contexts = declared_contexts(decls.get("quality.json").map(String::as_str).unwrap_or(""));
    if !contexts.is_empty() {
        let rc = verify_contexts(&gh, owner, repo, &sha, &contexts);
        if rc != 0 {
            if opts.unverified_contexts {
                eprintln!(
                    "  WARNING: applying {} unverified context(s) (--unverified-contexts).",
                    contexts.len()
                );
            } else if rc == 1 {
                eprintln!(
                    "ERROR: refusing to apply — required contexts are not reportable by the checked commit."
                );
                eprintln!(
                    "  --unverified-contexts to override (only for a repository that has never had a PR yet)."
                );
                return Err(4);
            }
        }
    }

    println!(
        "Applying declared rulesets to: {owner_repo} (ref={reference}, reconcile={})",
        opts.reconcile
    );
    println!();

    let existing = list_rulesets(&gh, owner_repo);
    let mut declared_names = Vec::new();
    for (file, body) in &decls {
        declared_names.push(apply_one_ruleset(
            &gh, owner_repo, &existing, file, body, &opts,
        )?);
    }

    let stray: Vec<&Value> = existing
        .iter()
        .filter(|r| {
            r.get("target").and_then(Value::as_str) == Some("branch")
                && !r
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| declared_names.iter().any(|d| d == n))
        })
        .collect();
    if !stray.is_empty() {
        println!();
        println!(
            "NOTE: {} active branch ruleset(s) not in the declaration:",
            stray.len()
        );
        for r in stray {
            println!("  - {} (id={})", raw(r.get("name")), raw(r.get("id")));
        }
        println!("  These are reported, not deleted.");
    }

    if !opts.dry_run {
        warn_open_prs_missing_contexts(&gh, owner_repo, &contexts);
    }
    Ok(())
}

fn list_rulesets(gh: &Gh, owner_repo: &str) -> Vec<Value> {
    gh.read_json(&[&format!("repos/{owner_repo}/rulesets")])
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

/// `--delete-ruleset NAME`: 宣言に同名が無いことを確認した上で DELETE する。
fn delete_ruleset(gh: &Gh, owner_repo: &str, decls: &Decls, opts: &Opts) -> Result<(), i32> {
    let target = &opts.delete_ruleset;
    let existing = list_rulesets(gh, owner_repo);
    for (file, body) in decls {
        if &decl_name(file, body)? == target {
            eprintln!(
                "ERROR: '{target}' is still declared in .github/rulesets/ — remove it from the declaration first."
            );
            return Err(1);
        }
    }
    let Some(id) = existing_id(&existing, target) else {
        println!("  '{target}' is not an active branch ruleset — nothing to delete.");
        return Ok(());
    };
    if opts.dry_run {
        println!("  DRY-RUN: would DELETE '{target}' (id={id})");
    } else {
        gh.write("DELETE", &format!("repos/{owner_repo}/rulesets/{id}"), None)?;
        println!("  ✓  Deleted '{target}' (id={id})");
    }
    Ok(())
}

/// `apply-rulesets` の入口。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--selftest") => {
            eprintln!(
                "apply-rulesets: --selftest は `cargo test -p apply-rulesets` に移った(#414)"
            );
            0
        }
        Some("--help") | Some("-h") => {
            print!("{USAGE}");
            0
        }
        _ => run_inner(args).err().unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_matches_the_posix_regex() {
        for s in [
            "__X__",
            "a __CLI_CRATE__ CLI",
            "_____",
            "x__A_B__y",
            "__A__B__",
        ] {
            assert!(has_placeholder(s), "{s}");
        }
        for s in [
            "", "____", "__x__", "_A_", "__ABC", "ABC__", "__ __", "a_b_c",
        ] {
            assert!(!has_placeholder(s), "{s}");
        }
    }

    #[test]
    fn declared_contexts_are_sorted_unique_and_tolerate_bad_json() {
        let q = r#"{"rules":[{"type":"deletion"},{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"b"},{"context":"a"},{"context":"b"}]}}]}"#;
        assert_eq!(declared_contexts(q), ["a", "b"]);
        assert!(declared_contexts("not json").is_empty());
        assert!(declared_contexts(r#"{"rules":5}"#).is_empty());
    }
}
