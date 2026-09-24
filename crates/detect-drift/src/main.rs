//! detect-drift [--porcelain]
//!
//! ad-hoc install(apt/cargo/npm -g/pipx)の宣言外在庫を検出して報告する
//! だけの detector(#4)。一切変更しない。コアロジック(パース・diff)は
//! lib.rs にあり `cargo test` で検証済み — このファイルはプロセス起動と
//! 出力フォーマットだけを担う。
//!
//! ADR-0005(binary-existence gating)に倣い、各レイヤーのツール自体が
//! PATH に無ければそのレイヤーを黙って skip する(未インストールを
//! エラーにしない)。
use std::process::{Command, ExitCode};

use detect_drift::{
    compose_issue_report, diff_apt, diff_cargo, diff_npm, diff_pipx, exit_code,
    filter_registry_excluded, parse_apt_declared, parse_apt_installed, parse_cargo_installed,
    parse_npm_global, parse_pipx_venvs, FileIssueOutcome, LayerDrift,
};

const USAGE: &str = "usage: detect-drift [--porcelain] [--file-issue <owner>/<repo>]\n\n\
Reports apt/cargo/npm -g/pipx packages installed outside their declaration\n\
(packages/declarative/apt-packages.txt for apt; cargo/npm/pipx have no\n\
declaration file, so every installed package is a candidate, ADR-0001).\n\
Detection only — never installs, removes, or modifies anything.\n\n\
Without --porcelain: human-readable OK/WARN lines, exit 0 if clean, 1 if\n\
drift was found.\n\
--porcelain: machine-readable TSV (layer\\tname\\tnix-attr-candidate).\n\
--file-issue <owner>/<repo>: on drift, file (or comment on) a `drift`-\n\
labelled GitHub Issue via `gh`. Excludes any cargo/npm/pipx name registered\n\
in update-own-tools' registry.toml (ADR-0025) — fails closed (files\n\
nothing) if that registry exists but cannot be parsed. Exit code reflects\n\
delivery, not drift presence: 0 = delivered (issue filed/commented, or\n\
nothing to report after ADR-0025 filtering), 3 = delivery failed.";

fn main() -> ExitCode {
    let mut porcelain = false;
    let mut file_issue_repo: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--porcelain" => porcelain = true,
            "--file-issue" => match args.next() {
                Some(repo) => file_issue_repo = Some(repo),
                None => {
                    eprintln!("--file-issue requires <owner>/<repo>\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown option: {other}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let mut drifts = Vec::new();
    if let Some(d) = check_apt() {
        drifts.push(d);
    }
    if let Some(d) = check_cargo() {
        drifts.push(d);
    }
    if let Some(d) = check_npm() {
        drifts.push(d);
    }
    if let Some(d) = check_pipx() {
        drifts.push(d);
    }

    let any_drift = drifts.iter().any(|d| !d.is_clean());

    if porcelain {
        for d in &drifts {
            for name in &d.undeclared {
                let attr = nix_locate_candidate(name).unwrap_or_default();
                println!("{}\t{}\t{}", d.layer, name, attr);
            }
        }
    } else {
        for d in &drifts {
            if d.is_clean() {
                println!("OK   {}: 宣言外の在庫なし", d.layer);
            } else {
                for name in &d.undeclared {
                    match nix_locate_candidate(name) {
                        Some(attr) => println!(
                            "WARN {}: '{name}' が宣言に無い(nixpkgs 候補: {attr})",
                            d.layer
                        ),
                        None => println!("WARN {}: '{name}' が宣言に無い", d.layer),
                    }
                }
            }
        }
    }

    let outcome = match file_issue_repo {
        None => FileIssueOutcome::NotRequested,
        Some(_) if !any_drift => FileIssueOutcome::Delivered,
        Some(repo) => match file_issue(&repo, &drifts) {
            Ok(()) => FileIssueOutcome::Delivered,
            Err(msg) => {
                eprintln!("WARN detect-drift --file-issue: {msg}(起票せず終了、exit 3)");
                FileIssueOutcome::Failed
            }
        },
    };

    ExitCode::from(exit_code(any_drift, outcome))
}

/// `--file-issue`: レジストリで ADR-0025 対象を除外してから、`drift`
/// ラベルの open Issue を探し、無ければ新規起票・あればコメント追記する。
/// レジストリファイルが存在するのにパースできない場合は fail-closed で
/// 何もしない(存在しない場合は「登録ゼロ」として続行する — ADR-0005 の
/// binary/config-existence gating と同じ扱い)。
fn file_issue(owner_repo: &str, drifts: &[LayerDrift]) -> Result<(), String> {
    let registry = match update_own_tools::xdg_dir("XDG_CONFIG_HOME", ".config") {
        Some(base) => {
            let path = base.join("update-own-tools/registry.toml");
            match std::fs::read_to_string(&path) {
                Ok(text) => match update_own_tools::parse_registry(&text) {
                    Ok(r) => r,
                    Err(e) => {
                        return Err(format!(
                            "registry.toml が存在するがパースできない({e}) — ADR-0025 により fail-closed"
                        ))
                    }
                },
                Err(_) => update_own_tools::Registry { tools: Vec::new() },
            }
        }
        None => update_own_tools::Registry { tools: Vec::new() },
    };

    let (filtered, excluded) = filter_registry_excluded(drifts, &registry);
    if filtered.iter().all(LayerDrift::is_clean) {
        // 全件 ADR-0025 対象で除外された(実質的に報告すべき内容が無い)。
        return Ok(());
    }

    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string());
    let body = compose_issue_report(&hostname, &filtered, excluded);
    let title = format!("ad-hoc install drift detected on {hostname}");

    // `drift` ラベルが無いと `gh issue create --label drift` はエラーに
    // なる。`--force` で作成/更新を1本化し、事前のワンタイム手動セットアップ
    // を要求しない(冪等 — 既にあれば説明文を上書きするだけ)。このラベル
    // 作成自体の失敗(既存ラベルへの権限不足等)は無視してよいので、
    // 通常の `run()`(黙って skip)のままにするが、原因は stderr に残す
    // (これも起票経路の一部なので ADR-0005 の「未インストール」の話とは別)。
    if run(
        "gh",
        &[
            "label",
            "create",
            "drift",
            "--repo",
            owner_repo,
            "--description",
            "detect-drift(#4)が検出した宣言外の ad-hoc install",
            "--force",
        ],
    )
    .is_none()
    {
        eprintln!("WARN detect-drift --file-issue: gh label create drift に失敗(続行)");
    }

    let existing = run_gh(&[
        "issue", "list", "--repo", owner_repo, "--label", "drift", "--state", "open", "--json",
        "number", "--limit", "1",
    ])
    .map_err(|e| format!("gh issue list に失敗: {e}"))?;

    let numbers: Vec<serde_json::Value> = serde_json::from_str(&existing)
        .map_err(|e| format!("gh issue list の出力が JSON でない: {e}"))?;

    if let Some(n) = numbers
        .first()
        .and_then(|v| v.get("number"))
        .and_then(|n| n.as_i64())
    {
        run_gh(&[
            "issue",
            "comment",
            &n.to_string(),
            "--repo",
            owner_repo,
            "--body",
            &body,
        ])
        .map_err(|e| format!("gh issue comment #{n} に失敗: {e}"))?;
    } else {
        run_gh(&[
            "issue", "create", "--repo", owner_repo, "--title", &title, "--body", &body, "--label",
            "drift",
        ])
        .map_err(|e| format!("gh issue create に失敗: {e}"))?;
    }
    Ok(())
}

/// コマンドを実行し stdout を返す。バイナリ自体が PATH に無い場合や実行
/// エラーの場合は `None`(ADR-0005: 未インストールは skip、エラーにしない)。
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// `gh` を実行し stdout を返す。`run()` と違い、`gh` 自体が PATH に無い場合
/// も含めて失敗理由を `Err` に残す — 起票経路(`file_issue`)専用のヘルパ。
/// 検出層の `run()` は ADR-0005 どおり黙って skip し続ける(こちらは変えない)。
/// 失敗時のメッセージは stderr の先頭行(無ければ実行自体の失敗内容)。
fn run_gh(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("gh の実行に失敗: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let first_line = stderr.lines().next().unwrap_or("").trim();
        return Err(if first_line.is_empty() {
            format!("終了コード {}", out.status)
        } else {
            first_line.to_string()
        });
    }
    String::from_utf8(out.stdout).map_err(|e| format!("gh の出力が UTF-8 でない: {e}"))
}

fn check_apt() -> Option<LayerDrift> {
    let declared_text = std::fs::read_to_string(apt_declared_path()).unwrap_or_default();
    let declared = parse_apt_declared(&declared_text);
    let installed_text = run("apt-mark", &["showmanual"])?;
    let installed = parse_apt_installed(&installed_text);
    Some(diff_apt(&declared, &installed))
}

/// 解決順は `scripts/apply-rulesets.sh`(#417)と同じ3段: 明示指定 >
/// home-manager 配備先 > checkout 相対。`~/.local/bin/detect-drift` は
/// systemd timer(cwd は checkout と無関係)から動くのが常用経路なので、
/// 配備先を優先する。checkout 相対は `cargo run`/`cargo test` からの
/// 手動実行専用のフォールバック。
fn apt_declared_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("DETECT_DRIFT_APT_DECLARED") {
        return std::path::PathBuf::from(p);
    }
    let xdg_config = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.config")));
    if let Some(base) = xdg_config {
        let deployed = std::path::PathBuf::from(base).join("dotfiles/apt-packages.txt");
        if deployed.is_file() {
            return deployed;
        }
    }
    std::path::PathBuf::from("packages/declarative/apt-packages.txt")
}

fn check_cargo() -> Option<LayerDrift> {
    let text = run("cargo", &["install", "--list"])?;
    let installed = parse_cargo_installed(&text);
    Some(diff_cargo(&installed))
}

fn check_npm() -> Option<LayerDrift> {
    let text = run("npm", &["ls", "-g", "--depth=0", "--json"])?;
    let installed = parse_npm_global(&text);
    Some(diff_npm(&installed))
}

fn check_pipx() -> Option<LayerDrift> {
    let text = run("pipx", &["list", "--json"])?;
    let installed = parse_pipx_venvs(&text);
    Some(diff_pipx(&installed))
}

/// `nix-locate --top-level --whole-name "bin/<name>"` を best-effort で
/// 呼び、最初の候補行の先頭トークン(属性パス)を返す。`nix-locate` が
/// PATH に無い、または一致が無い場合は `None`(注記が無いだけで、検出結果
/// そのものには影響しない)。
fn nix_locate_candidate(name: &str) -> Option<String> {
    let pattern = format!("bin/{name}");
    let out = run("nix-locate", &["--top-level", "--whole-name", &pattern])?;
    let first_line = out.lines().next()?;
    let attr = first_line.split_whitespace().next()?;
    Some(attr.to_string())
}
