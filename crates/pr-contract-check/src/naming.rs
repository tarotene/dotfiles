//! workflow-naming-check — CI workflow 命名基準(docs/adr/591-ci-workflow-naming.md)の
//! drift を、対象リポジトリの PR head に対して required check として検査する
//! (`.github/actions/pr-title` の一 step)。
//!
//! 判定は [`crate::workflows::judge_workflows`] の単一正本(`crates/github-audit` の
//! workflows ドメインと同じ関数)。この検査が足すのは、REST API から入力を集める層だけ。

use crate::gh::{self, Gh};
use crate::verdict::Verdict;
use crate::workflows::{judge_workflows, Inputs};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "\
usage: workflow-naming-check <owner>/<repo> [--ref REF]
       workflow-naming-check --dir <path>

judge_workflows(crates/pr-contract-check の workflows)を対象リポジトリの REST API
実測データに対して実行し、CI workflow 命名基準(docs/adr/591-ci-workflow-
naming.md)から drift していれば非0で終了する。

--ref を省略すると対象リポジトリの default branch を検査する。PR の head
を検査したいときは呼び出し側(.github/actions/pr-title)が
github.event.pull_request.head.sha を明示的に渡す。

--dir <path> は、REST API を使わず、手元のチェックアウト(<path>/.github/workflows/ と
<path>/.github/rulesets/quality.json)を読む。push の前に、CI の PR title 検査と同じ
判定を手元で回すための入口(gh も認証も要らない)。--ref とは同時に使えない。
";

/// quality.json の `required_status_checks` の各要素を `context` 順に並べた compact な JSON。
/// jq がエラーになる入力は `None`。比較は出力文字列で行う(キーの並びも違いに数える)。
const QUALITY_STATUS_CHECKS_JQ: &str = "[.rules[]? | select(.type == \"required_status_checks\") \
| .parameters.required_status_checks[]?] | sort_by(.context)";

fn quality_status_checks(text: &str) -> Option<String> {
    gh::jq(&["-c", QUALITY_STATUS_CHECKS_JQ], text).map(|s| s.trim_end_matches('\n').to_owned())
}

/// quality.json の単一正本の場所。`WORKFLOWS_CANONICAL_QUALITY_FILE` で差し替えられる。
/// 既定は実行ファイルの実体(realpath)の 1 つ上からの相対パス — リポジトリの checkout の
/// 中で動かしたときだけ実在し、CI で sparse-checkout した実体の隣には無い(その場合は
/// bash 版と同じく、この検査自体を飛ばす)。
///
/// `--dir` で手元のチェックアウトを検査するときは、そのチェックアウト内の同じテンプレートを使う
/// (`cargo run` した実行ファイルの隣には無いため)。
fn canonical_quality_file(local_dir: Option<&Path>) -> PathBuf {
    if let Some(p) = std::env::var_os("WORKFLOWS_CANONICAL_QUALITY_FILE").filter(|p| !p.is_empty())
    {
        return PathBuf::from(p);
    }
    if let Some(d) = local_dir {
        return d.join(CANONICAL_QUALITY_REL);
    }
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .unwrap_or_default();
    let dir = exe.parent().map(PathBuf::from).unwrap_or_default();
    dir.join(format!("../{CANONICAL_QUALITY_REL}"))
}

/// 単一正本の、リポジトリの根からの相対パス。
const CANONICAL_QUALITY_REL: &str =
    "config/agents/skills/repo-governance-common/templates/.github/rulesets/quality.json";

/// 取れなかった(無い)ファイルは空文字。
fn file_text(gh: &Gh, owner_repo: &str, path: &str, git_ref: &str) -> String {
    let out = gh.raw_file(owner_repo, path, git_ref);
    if out.ok {
        out.stdout
    } else {
        String::new()
    }
}

/// `owner/repo` の `git_ref` 時点の workflow 命名を検査する。
pub fn check_repo(gh: &Gh, owner_repo: &str, git_ref: &str) -> Verdict {
    let ref_arg = format!("ref={git_ref}");
    let file_names = gh
        .lines(
            &format!("repos/{owner_repo}/contents/.github/workflows"),
            &["-X", "GET", "-F", &ref_arg],
            ".[] | select(.type == \"file\") | .name",
        )
        .unwrap_or_default();
    let ci = file_text(gh, owner_repo, ".github/workflows/ci.yml", git_ref);
    let pr_title = file_text(gh, owner_repo, ".github/workflows/pr-title.yml", git_ref);
    let quality = file_text(gh, owner_repo, ".github/rulesets/quality.json", git_ref);
    let canonical = std::fs::read_to_string(canonical_quality_file(None)).ok();
    judge(
        &format!("{owner_repo}@{git_ref}"),
        &file_names,
        &ci,
        &pr_title,
        &quality,
        canonical.as_deref(),
    )
}

/// 手元のチェックアウト `dir` の workflow 命名を検査する(`--dir`。API を使わない)。
/// 読めない・無いファイルは、API 版の「取れなかった」と同じく空文字として扱う。
pub fn check_dir(dir: &Path) -> Verdict {
    let read = |rel: &str| std::fs::read_to_string(dir.join(rel)).unwrap_or_default();
    let mut file_names: Vec<String> = std::fs::read_dir(dir.join(".github/workflows"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    file_names.sort();
    let ci = read(".github/workflows/ci.yml");
    let pr_title = read(".github/workflows/pr-title.yml");
    let quality = read(".github/rulesets/quality.json");
    let canonical = std::fs::read_to_string(canonical_quality_file(Some(dir))).ok();
    judge(
        &dir.display().to_string(),
        &file_names,
        &ci,
        &pr_title,
        &quality,
        canonical.as_deref(),
    )
}

/// 入力が出そろった後の判定と、結果の整形(API 版と `--dir` 版で共通)。`label` は結果の見出しに出す対象。
fn judge(
    label: &str,
    file_names: &[String],
    ci: &str,
    pr_title: &str,
    quality: &str,
    canonical: Option<&str>,
) -> Verdict {
    let missing = judge_workflows(
        &Inputs {
            file_names,
            ci_text: ci,
            pr_title_text: pr_title,
            quality_text: quality,
            canonical_quality_text: canonical,
        },
        &quality_status_checks,
    );
    if missing.is_empty() {
        return Verdict {
            code: 0,
            out: vec!["workflow-naming-check: OK".into()],
            err: vec![],
        };
    }
    let mut err = vec![format!(
        "workflow-naming-check: {label} has drifted from the CI workflow naming basis (docs/adr/591-ci-workflow-naming.md):"
    )];
    err.extend(missing.iter().map(|m| format!("  - {m}")));
    Verdict {
        code: 1,
        out: vec![],
        err,
    }
}

/// CLI の入口。`args` は `argv[1..]`。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    let gh = Gh::from_env("WORKFLOW_NAMING_CHECK_GH_BIN");
    let mut owner_repo: Option<&str> = None;
    let mut git_ref: Option<&str> = None;
    let mut dir: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--ref" => {
                let Some(v) = args.get(i + 1) else {
                    eprint!("{USAGE}");
                    return 2;
                };
                git_ref = Some(v);
                i += 2;
            }
            "--dir" => {
                let Some(v) = args.get(i + 1) else {
                    eprint!("{USAGE}");
                    return 2;
                };
                dir = Some(v);
                i += 2;
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            a if a.starts_with('-') => {
                eprint!("{USAGE}");
                return 2;
            }
            a => {
                owner_repo = Some(a);
                i += 1;
            }
        }
    }
    if let Some(d) = dir {
        // 手元のチェックアウトを検査する。API の引数(<owner>/<repo>・--ref)とは同時に使えない。
        if owner_repo.is_some() || git_ref.is_some() {
            eprint!("{USAGE}");
            return 2;
        }
        return check_dir(Path::new(d)).emit();
    }
    let Some(owner_repo) = owner_repo else {
        eprint!("{USAGE}");
        return 2;
    };

    let default_branch;
    let git_ref = match git_ref {
        Some(r) => r,
        None => {
            let out = gh.api(&format!("repos/{owner_repo}"), &["--jq", ".default_branch"]);
            if !out.ok {
                eprintln!(
                    "workflow-naming-check: {owner_repo} の default branch 取得に失敗しました(fail-closed)"
                );
                return 1;
            }
            default_branch = out.stdout.trim_end_matches('\n').to_owned();
            &default_branch
        }
    };
    check_repo(&gh, owner_repo, git_ref).emit()
}
