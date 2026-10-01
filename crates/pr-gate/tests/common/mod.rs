//! 統合テストの共通部品(旧 `pr-gate.sh --selftest` の実験環境の移植、#415)。
//!
//! 対象は既定で cargo の bin。`PR_GATE_ORACLE` に bash 版スクリプトのパスを
//! 入れると `bash <path>` に差し替わる(移植手順の段 2: 同じケースを bash に
//! 向けて先に緑にする、docs/rust-migration.md)。
//!
//! 旧 selftest は 1 つの repo を順に書き換えながら全ケースを流していた。ここでは
//! ケースごとに同じ初期状態の repo を作り直し、そのケースが前提にしていた状態
//! (origin/HEAD・upstream 設定など)だけを明示的に再現する。

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 旧 selftest の gh スタブ(そのまま)。環境変数で応答を切り替える:
///   PR_GATE_STUB_NO_PR=1            : gh pr list が [] を返す
///   PR_GATE_STUB_PR_NUM / _BASE / _HEAD_OID : gh pr list が返す PR の中身
///   PR_GATE_STUB_RULES_FILE         : gh api rules/branches の応答(未指定なら [])
///   PR_GATE_STUB_RULES_FAIL=1       : gh api rules/branches を非ゼロ終了させる
///   PR_GATE_STUB_CHECKS_FILE        : gh pr checks --json の応答(未指定なら [])
///   PR_GATE_STUB_WATCH_RC           : gh pr checks --watch の exit code(既定 0)
///   PR_GATE_STUB_PR_BODY            : gh pr list が返す PR 本文(既定は
///     "Closes #1" + "No-Visual: selftest 既定本文" の 2 行)
///   PR_GATE_STUB_CHAIN_FILE         : gh pr list --state open(--head 無し)の応答
///   PR_GATE_STUB_STACK_EXT_RC       : gh stack --version の exit code(既定 0)
///   PR_GATE_STUB_STACKS_FILE / _RC  : gh api repos/<nwo>/stacks の応答 / exit code
///   PR_GATE_STUB_DRAFT              : gh pr list が返す isDraft(既定 false)
///   PR_GATE_STUB_HANDOFF_STATE / _FAIL : gh issue view の応答 / 失敗
///   PR_GATE_STUB_ALL_STATE          : gh pr list --state all が返す state
pub const STUB_GH: &str = r#"#!/usr/bin/env bash
jqbin="$(command -v jq)"
case "$1" in
  pr)
    case "$2" in
      list)
        if [[ "$*" == *"--state all"* ]]; then
          if [[ -n "${PR_GATE_STUB_ALL_STATE:-}" ]]; then
            "$jqbin" -n --arg s "${PR_GATE_STUB_ALL_STATE}" '[{state:$s}]'
          else
            echo '[]'
          fi
        elif [[ "$*" != *"--head "* ]]; then
          if [[ -n "${PR_GATE_STUB_CHAIN_FILE:-}" && -f "${PR_GATE_STUB_CHAIN_FILE:-}" ]]; then
            cat "${PR_GATE_STUB_CHAIN_FILE}"
          else
            "$jqbin" -n --arg num "${PR_GATE_STUB_PR_NUM:-37}" --arg base "${PR_GATE_STUB_BASE:-main}" \
              '[{number:($num|tonumber), headRefName:"selftest-branch-unused", baseRefName:$base}]'
          fi
        elif [[ "${PR_GATE_STUB_NO_PR:-0}" == "1" ]]; then
          echo '[]'
        else
          "$jqbin" -n --arg num "${PR_GATE_STUB_PR_NUM:-37}" \
            --arg base "${PR_GATE_STUB_BASE:-main}" \
            --arg head "${PR_GATE_STUB_HEAD_OID:-0000000000000000000000000000000000000000}" \
            --arg body "${PR_GATE_STUB_PR_BODY-Closes #1
No-Visual: selftest 既定本文}" \
            --argjson draft "${PR_GATE_STUB_DRAFT:-false}" \
            '[{number:($num|tonumber), baseRefName:$base, headRefOid:$head, body:$body, isDraft:$draft}]'
        fi
        ;;
      checks)
        if [[ "$*" == *"--watch"* ]]; then
          exit "${PR_GATE_STUB_WATCH_RC:-0}"
        fi
        if [[ -n "${PR_GATE_STUB_CHECKS_FILE:-}" && -f "${PR_GATE_STUB_CHECKS_FILE:-}" ]]; then
          cat "${PR_GATE_STUB_CHECKS_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  issue)
    case "$2" in
      view)
        if [[ "${PR_GATE_STUB_HANDOFF_FAIL:-0}" == "1" ]]; then
          exit 1
        fi
        printf '%s\n' "${PR_GATE_STUB_HANDOFF_STATE:-OPEN}"
        ;;
      *) exit 1 ;;
    esac
    ;;
  stack)
    case "$2" in
      --version) exit "${PR_GATE_STUB_STACK_EXT_RC:-0}" ;;
      *) exit 1 ;;
    esac
    ;;
  api)
    case "$*" in
      *rules/branches*)
        if [[ "${PR_GATE_STUB_RULES_FAIL:-0}" == "1" ]]; then
          exit 1
        fi
        if [[ -n "${PR_GATE_STUB_RULES_FILE:-}" && -f "${PR_GATE_STUB_RULES_FILE:-}" ]]; then
          cat "${PR_GATE_STUB_RULES_FILE}"
        else
          echo '[]'
        fi
        ;;
      *stacks*)
        if [[ "${PR_GATE_STUB_STACKS_RC:-0}" != "0" ]]; then
          exit "${PR_GATE_STUB_STACKS_RC}"
        fi
        if [[ -n "${PR_GATE_STUB_STACKS_FILE:-}" && -f "${PR_GATE_STUB_STACKS_FILE:-}" ]]; then
          cat "${PR_GATE_STUB_STACKS_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
"#;

pub const RULES_2: &str = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"job-a"},{"context":"job-b"}]}}]"#;
pub const CHECKS_2PASS: &str = r#"[{"name":"job-a","bucket":"pass","link":"https://x/runs/1/job/11","workflow":"CI"},{"name":"job-b","bucket":"pass","link":"https://x/runs/1/job/12","workflow":"CI"}]"#;
pub const CHECKS_1OF2: &str =
    r#"[{"name":"job-a","bucket":"pass","link":"https://x/runs/1/job/11","workflow":"CI"}]"#;
pub const CHECKS_PARTIAL_PENDING: &str = r#"[{"name":"job-a","bucket":"pass","link":"https://x/runs/1/job/11","workflow":"CI"},{"name":"job-b","bucket":"pending","link":"https://x/runs/1/job/12","workflow":"CI"}]"#;
pub const CHECKS_PARTIAL_FAIL: &str = r#"[{"name":"job-a","bucket":"pass","link":"https://x/runs/1/job/11","workflow":"CI"},{"name":"job-b","bucket":"fail","link":"https://x/runs/1/job/12","workflow":"CI"}]"#;
pub const CHECKS_QUIESCE_PASS: &str = r#"[{"name":"Shell script validation","bucket":"pass","link":"https://x/runs/2/job/21","workflow":"CI"}]"#;
pub const CHECKS_QUIESCE_FAIL: &str = r#"[{"name":"Shell script validation","bucket":"fail","link":"https://x/runs/2/job/21","workflow":"CI"}]"#;
pub const ZERO_OID: &str = "0000000000000000000000000000000000000000";

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// stdout + stderr(旧 selftest の `"$out$(cat "$dir/err")"`)。
    pub fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
    /// SessionStart の additionalContext(取れなければ空)。
    pub fn ctx(&self) -> String {
        serde_ctx(&self.stdout)
    }
}

fn serde_ctx(stdout: &str) -> String {
    // jq -r '.hookSpecificOutput.additionalContext'
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| {
            v.pointer("/hookSpecificOutput/additionalContext")
                .and_then(|c| c.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn oracle() -> Option<PathBuf> {
    std::env::var_os("PR_GATE_ORACLE")
        .filter(|v| !v.is_empty())
        .map(|p| std::fs::canonicalize(p).expect("oracle path"))
}

/// 親の PATH から実行ファイルの絶対パスを引く(子の PATH を差し替えるため)。
pub fn which(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("{name} not on PATH"))
}

pub fn write_exec(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// 1 ケース分の隔離環境: github remote 付きの git repo(base → c1 → c2、
/// origin/main = base、FETCH_HEAD は新しい)と gh スタブ。
pub struct Fx {
    pub tmp: tempfile::TempDir,
    pub repo: PathBuf,
    pub real_head: String,
    /// 対象を呼ぶときの PATH(既定は stub + 親の PATH)。
    pub path: String,
}

impl Fx {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        write_exec(&bin.join("gh"), STUB_GH);
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let mut fx = Fx {
            tmp,
            repo,
            real_head: String::new(),
            path,
        };
        fx.git(&["init", "-q"]);
        fx.commit("base");
        fx.git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/example/example.git",
        ]);
        let base = fx.rev("HEAD");
        fx.git(&["update-ref", "refs/remotes/origin/main", &base]);
        // fetch(ネットワーク I/O)は対象外。FETCH_HEAD を作って TTL 判定
        // (「新しければ fetch しない」)だけを検査する。
        std::fs::write(fx.repo.join(".git/FETCH_HEAD"), "").unwrap();
        fx.commit("c1");
        fx.commit("c2");
        fx.real_head = fx.rev("HEAD");
        std::fs::write(fx.allowlist(), "example/example\n").unwrap();
        fx
    }

    pub fn dir(&self) -> &Path {
        self.tmp.path()
    }
    pub fn allowlist(&self) -> PathBuf {
        self.dir().join("allowlist")
    }

    fn git_env(c: &mut Command) {
        c.env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
    }

    /// `git -C <dir> …`(失敗は panic)。stdout(末尾改行除去)を返す。
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut c = Command::new("git");
        c.arg("-C").arg(dir).args(args);
        Self::git_env(&mut c);
        let out = c.stdin(Stdio::null()).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
    pub fn git(&self, args: &[&str]) -> String {
        let repo = self.repo.clone();
        self.git_in(&repo, args)
    }
    pub fn commit(&self, msg: &str) {
        self.git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            msg,
        ]);
    }
    pub fn rev(&self, r: &str) -> String {
        self.git(&["rev-parse", r])
    }
    pub fn branch(&self) -> String {
        self.git(&["branch", "--show-current"])
    }
    pub fn write(&self, name: &str, body: &str) -> String {
        let p = self.dir().join(name);
        std::fs::write(&p, body).unwrap();
        p.display().to_string()
    }

    /// 現在のブランチの upstream を origin/gate-test-upstream(= `at`)に張る。
    pub fn set_upstream(&self, at: &str) {
        self.git(&["update-ref", "refs/remotes/origin/gate-test-upstream", at]);
        let b = self.branch();
        self.git(&["config", &format!("branch.{b}.remote"), "origin"]);
        self.git(&[
            "config",
            &format!("branch.{b}.merge"),
            "refs/heads/gate-test-upstream",
        ]);
    }

    /// origin/HEAD を origin/<name>(= `at`、None なら既存の ref のまま)に向ける。
    pub fn set_origin_head(&self, name: &str, at: Option<&str>) {
        if let Some(at) = at {
            self.git(&["update-ref", &format!("refs/remotes/origin/{name}"), at]);
        }
        self.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            &format!("refs/remotes/origin/{name}"),
        ]);
    }

    /// 対象を呼ぶ。`sub` は "stop" / "session-start"。
    pub fn run_in(&self, project: &Path, sub: &str, stdin: &str, envs: &[(&str, &str)]) -> Run {
        let mut c = match oracle() {
            Some(p) => {
                let mut c = Command::new(which("bash"));
                c.arg(p);
                c
            }
            None => Command::new(env!("CARGO_BIN_EXE_pr-gate")),
        };
        c.arg(sub).env_clear();
        for k in ["LANG", "LC_ALL", "LC_CTYPE", "TMPDIR"] {
            if let Some(v) = std::env::var_os(k) {
                c.env(k, v);
            }
        }
        c.env("HOME", self.dir().join("home"))
            .env("PATH", &self.path)
            .env("PR_GATE_DIR", self.dir().join("state"))
            .env("PR_GATE_ALLOWLIST", self.allowlist())
            .env("PR_GATE_CHECK_APPEAR_TIMEOUT", "0")
            .env("PR_GATE_QUIESCE", "0")
            .env("PR_GATE_CI_TIMEOUT", "30")
            .env("CLAUDE_PROJECT_DIR", project)
            // G_prior の `git fetch origin <base>` をネットワークに出さない。
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_TERMINAL_PROMPT", "0");
        Self::git_env(&mut c);
        for (k, v) in envs {
            c.env(k, v);
        }
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let o = child.wait_with_output().unwrap();
        Run {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    pub fn hookinput(&self, sid: &str) -> String {
        format!(
            r#"{{"cwd":"{}","session_id":"{sid}"}}"#,
            self.repo.display()
        )
    }

    pub fn stop(&self, sid: &str, envs: &[(&str, &str)]) -> Run {
        self.run_in(&self.repo, "stop", &self.hookinput(sid), envs)
    }
    pub fn session_start(&self, sid: &str, envs: &[(&str, &str)]) -> Run {
        self.run_in(&self.repo, "session-start", &self.hookinput(sid), envs)
    }

    /// 旧 selftest の `glink`: push 済み + 全 required pass(「あとは終わるだけ」)
    /// の状態で本文だけを差し替えて stop を流す。
    pub fn glink(&self, sid: &str, body: &str) -> Run {
        let rules = self.write("rules-2.json", RULES_2);
        let checks = self.write("checks-2pass.json", CHECKS_2PASS);
        self.stop(
            sid,
            &[
                ("PR_GATE_STUB_HEAD_OID", &self.real_head),
                ("PR_GATE_STUB_RULES_FILE", &rules),
                ("PR_GATE_STUB_CHECKS_FILE", &checks),
                ("PR_GATE_STUB_PR_BODY", body),
            ],
        )
    }
}

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}
