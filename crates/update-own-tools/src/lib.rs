//! update-own-tools — 自作・タグ付きリリース未達の CLI を、ホストローカルの
//! レジストリに従って `origin/<branch>` からビルドしてインストールする
//! (ADR-0025、#276、docs/update-own-tools.md)。
//!
//! 対象リポジトリの名前は dotfiles のソースに一切書かない。レジストリは
//! `${XDG_CONFIG_HOME:-~/.config}/update-own-tools/registry.toml` に置く。
//! 対象リポジトリのチェックアウトには触れず、`git worktree add --detach` で
//! 作った一時 worktree の中でビルドする。

use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    #[serde(default, rename = "tool")]
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// 表示名。引数での絞り込みとキャッシュのディレクトリ名に使う。
    pub name: String,
    /// ローカルのクローンのパス、または clone できる URL。
    pub repo: String,
    /// crate のサブディレクトリ(リポジトリ相対)。既定は `.`。
    #[serde(default = "default_crate", rename = "crate")]
    pub crate_dir: String,
    /// ビルドするブランチ。既定は `main`。
    #[serde(default = "default_branch")]
    pub branch: String,
    /// インストールコマンド。`{dir}`(crate の絶対パス)と `{cache}`
    /// (ツールごとの target ディレクトリ)を置換する。
    #[serde(default = "default_install")]
    pub install: Vec<String>,
}

fn default_crate() -> String {
    ".".into()
}
fn default_branch() -> String {
    "main".into()
}
fn default_install() -> Vec<String> {
    [
        "cargo",
        "install",
        "--locked",
        "--path",
        "{dir}",
        "--target-dir",
        "{cache}",
    ]
    .map(String::from)
    .to_vec()
}

pub fn parse_registry(s: &str) -> Result<Registry, String> {
    let r: Registry = toml::from_str(s).map_err(|e| e.to_string())?;
    for t in &r.tools {
        if t.name.is_empty() || t.name.contains(['/', '\\']) || t.name.starts_with('.') {
            return Err(format!("tool.name が不正です: {:?}", t.name));
        }
        if t.install.is_empty() {
            return Err(format!("{}: install が空です", t.name));
        }
        if Path::new(&t.crate_dir).is_absolute() || t.crate_dir.split('/').any(|c| c == "..") {
            return Err(format!(
                "{}: crate はリポジトリ相対パスにしてください",
                t.name
            ));
        }
    }
    Ok(r)
}

/// 実行予定の 1 コマンド。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub cwd: Option<PathBuf>,
    pub argv: Vec<String>,
    /// 失敗しても後続を止めない(後片付け)。
    pub cleanup: bool,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(c) = &self.cwd {
            write!(f, "(cd {}) ", c.display())?;
        }
        write!(f, "{}", self.argv.join(" "))
    }
}

fn step(argv: &[&str]) -> Step {
    Step {
        cwd: None,
        argv: argv.iter().map(|s| s.to_string()).collect(),
        cleanup: false,
    }
}

/// `repo` がローカルのディレクトリなら URL ではない。
fn is_local(repo: &str) -> bool {
    Path::new(repo).is_dir()
}

/// 1 ツール分の手順を組み立てる。`cache` はキャッシュのルート。
pub fn plan(tool: &Tool, cache: &Path, run_id: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    let repo_dir: PathBuf = if is_local(&tool.repo) {
        PathBuf::from(&tool.repo)
    } else {
        let bare = cache.join("repos").join(format!("{}.git", tool.name));
        if !bare.is_dir() {
            steps.push(step(&[
                "git",
                "clone",
                "--bare",
                "--quiet",
                &tool.repo,
                &bare.to_string_lossy(),
            ]));
        }
        bare
    };
    let repo = repo_dir.to_string_lossy().into_owned();
    let remote_ref = format!("refs/remotes/origin/{}", tool.branch);
    let wt = cache
        .join("worktrees")
        .join(format!("{}-{}", tool.name, run_id));
    let wt_s = wt.to_string_lossy().into_owned();
    steps.push(step(&[
        "git",
        "-C",
        &repo,
        "fetch",
        "--quiet",
        "origin",
        &format!("+refs/heads/{}:{}", tool.branch, remote_ref),
    ]));
    steps.push(step(&[
        "git",
        "-C",
        &repo,
        "worktree",
        "add",
        "--quiet",
        "--detach",
        &wt_s,
        &remote_ref,
    ]));
    let dir = if tool.crate_dir == "." {
        wt.clone()
    } else {
        wt.join(&tool.crate_dir)
    };
    let target = cache.join("target").join(&tool.name);
    steps.push(Step {
        cwd: Some(dir.clone()),
        argv: tool
            .install
            .iter()
            .map(|a| {
                a.replace("{dir}", &dir.to_string_lossy())
                    .replace("{cache}", &target.to_string_lossy())
            })
            .collect(),
        cleanup: false,
    });
    steps.push(Step {
        cleanup: true,
        ..step(&["git", "-C", &repo, "worktree", "remove", "--force", &wt_s])
    });
    steps
}

/// 手順を実行する。後片付け以外の手順が失敗したら残りの通常手順を飛ばし、
/// 後片付けだけは必ず実行する。
pub fn execute(steps: &[Step]) -> Result<(), String> {
    let mut failure: Option<String> = None;
    for s in steps {
        if failure.is_some() && !s.cleanup {
            continue;
        }
        let mut cmd = Command::new(&s.argv[0]);
        cmd.args(&s.argv[1..]);
        if let Some(c) = &s.cwd {
            cmd.current_dir(c);
        }
        let ok = match cmd.status() {
            Ok(st) => st.success(),
            Err(e) => {
                if failure.is_none() && !s.cleanup {
                    failure = Some(format!("{s}: {e}"));
                }
                continue;
            }
        };
        if !ok && !s.cleanup && failure.is_none() {
            failure = Some(format!("失敗: {s}"));
        }
    }
    failure.map_or(Ok(()), Err)
}

/// `name` が PATH 上に実行ファイルとして存在するか。
pub fn on_path(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

pub fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation() {
        let r =
            parse_registry("[[tool]]\nname = \"t\"\nrepo = \"https://example.invalid/t.git\"\n")
                .unwrap();
        let t = &r.tools[0];
        assert_eq!(t.crate_dir, ".");
        assert_eq!(t.branch, "main");
        assert_eq!(t.install[0], "cargo");
        assert!(parse_registry("").unwrap().tools.is_empty());
        for bad in [
            "[[tool]]\nname = \"../x\"\nrepo = \"r\"\n",
            "[[tool]]\nname = \"x\"\nrepo = \"r\"\ninstall = []\n",
            "[[tool]]\nname = \"x\"\nrepo = \"r\"\ncrate = \"../y\"\n",
            "[[tool]]\nname = \"x\"\nrepo = \"r\"\ntypo = 1\n",
        ] {
            assert!(parse_registry(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn plan_for_url_clones_bare_once() {
        let d = tempfile::tempdir().unwrap();
        let t = Tool {
            name: "t".into(),
            repo: "https://example.invalid/t.git".into(),
            crate_dir: "crates/cli".into(),
            branch: "main".into(),
            install: default_install(),
        };
        let steps = plan(&t, d.path(), "1");
        let lines: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
        let c = d.path().display();
        assert_eq!(
            lines,
            vec![
                format!("git clone --bare --quiet https://example.invalid/t.git {c}/repos/t.git"),
                format!("git -C {c}/repos/t.git fetch --quiet origin +refs/heads/main:refs/remotes/origin/main"),
                format!("git -C {c}/repos/t.git worktree add --quiet --detach {c}/worktrees/t-1 refs/remotes/origin/main"),
                format!("(cd {c}/worktrees/t-1/crates/cli) cargo install --locked --path {c}/worktrees/t-1/crates/cli --target-dir {c}/target/t"),
                format!("git -C {c}/repos/t.git worktree remove --force {c}/worktrees/t-1"),
            ]
        );
        assert!(steps.last().unwrap().cleanup);
        std::fs::create_dir_all(d.path().join("repos/t.git")).unwrap();
        assert!(!plan(&t, d.path(), "1")[0]
            .argv
            .contains(&"clone".to_string()));
    }

    #[test]
    fn cleanup_runs_after_failure() {
        let d = tempfile::tempdir().unwrap();
        let marker = d.path().join("cleaned");
        let steps = vec![
            Step {
                cwd: None,
                argv: vec!["false".into()],
                cleanup: false,
            },
            Step {
                cwd: None,
                argv: vec!["touch".into(), "never".into()],
                cleanup: false,
            },
            Step {
                cwd: None,
                argv: vec!["touch".into(), marker.to_string_lossy().into()],
                cleanup: true,
            },
        ];
        assert!(execute(&steps).is_err());
        assert!(marker.exists());
        assert!(!Path::new("never").exists());
    }
}
