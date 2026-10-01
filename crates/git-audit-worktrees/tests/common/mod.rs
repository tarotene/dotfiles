//! 統合テストの共通部品。実 git の一時 repo + herdr/gh のスタブ実行ファイルを
//! 組み、本物の `git-audit-worktrees` バイナリを走らせる(bash 版 --selftest の
//! fixture をそのまま写したもの)。`gh` スタブは実 `jq` を呼ぶので、テスト環境に
//! `jq` と `bash` が要る(bash 版 selftest と同じ前提)。
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn write_exec(path: &Path, script: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, script).unwrap();
    let mut perm = fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm).unwrap();
}

fn base_cmd(prog: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(prog);
    // ホストの ~/.gitconfig(hook / push 設定など)に左右されない。
    c.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null());
    c
}

/// git を実行して成功を要求し、stdout(末尾改行つき)を返す。
pub fn git(dir: &Path, args: &[&str]) -> String {
    let o = base_cmd("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?} in {dir:?} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn git_trim(dir: &Path, args: &[&str]) -> String {
    git(dir, args).trim_end_matches('\n').to_string()
}

pub fn git_ok(dir: &Path, args: &[&str]) -> bool {
    base_cmd("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

pub fn config_repo(repo: &Path) {
    git(repo, &["config", "core.hooksPath", "/dev/null"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    git(repo, &["config", "user.name", "test"]);
    git(repo, &["config", "user.email", "test@example.invalid"]);
}

/// herdr スタブ(通知あり): `worktree list` は何も開いていないと答え、
/// `notification show` は呼び出しを記録して reason ファイルどおりに返す。
pub const HERDR_NOTIFY_STUB: &str = r#"#!/usr/bin/env bash
if [[ "$1 $2" == "worktree list" ]]; then
  printf '{"result":{"worktrees":[]}}\n'
  exit 0
fi
printf '%s\n' call >>"$GIT_WORKTREE_AUDIT_TEST_CALLS"
reason="$(cat "$GIT_WORKTREE_AUDIT_TEST_REASON")"
if [[ $reason == shown ]]; then shown=true; else shown=false; fi
printf '{"result":{"shown":%s,"reason":"%s"}}\n' "$shown" "$reason"
"#;

/// herdr スタブ(到達不能)。
pub const HERDR_DOWN_STUB: &str = "#!/usr/bin/env bash\nexit 1\n";

/// herdr スタブ(evidence 用): worktree list は空、他は失敗。
pub const HERDR_EVIDENCE_STUB: &str = r#"#!/usr/bin/env bash
if [[ "$1 $2" == "worktree list" ]]; then
  printf '{"result":{"worktrees":[]}}\n'
  exit 0
fi
exit 1
"#;

/// REST 形の gh スタブ。`gh api repos/<slug>/commits/<sha>/pulls --jq EXPR`
/// を、固定 fixture の PR 一覧(小文字 state・head.sha)に対して実 `jq` で
/// 実行する(gh_closed_pr が実際に渡す --jq 式をそのまま試す)。全呼び出しを
/// `$GH_STUB_CALLS` に記録する。`$GH_STUB_FAIL` があれば失敗する。
pub const GH_STUB: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >>"${GH_STUB_CALLS:-/dev/null}"
jqexpr=""
prev=""
for a in "$@"; do
  if [[ $prev == "--jq" ]]; then jqexpr="$a"; fi
  prev="$a"
done
[[ $1 == api && $2 =~ ^repos/[^/]+/[^/]+/commits/([0-9a-f]+)/pulls$ ]] || exit 1
sha="${BASH_REMATCH[1]}"
[[ -n $jqexpr ]] || exit 1
[[ -f "$GH_STUB_FIXTURE" ]] || exit 1
[[ ! -e "${GH_STUB_FAIL:-/nonexistent}" ]] || exit 1
jq --arg sha "$sha" '[.[] | select(.head.sha == $sha)]' "$GH_STUB_FIXTURE" | jq -r "$jqexpr"
"#;

pub struct Fx {
    pub tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub repo: PathBuf,
    pub bin: PathBuf,
    pub state: PathBuf,
    pub remote: PathBuf,
}

impl Fx {
    /// `ghr/github.com/acme/repo`(main に空 commit 1 つ・実 bare remote へ push 済み)
    /// と herdr スタブ(`herdr_stub`)を用意する。
    pub fn new(herdr_stub: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let repo = root.join("ghr/github.com/acme/repo");
        let bin = root.join("bin");
        let state = root.join("state");
        let remote = root.join("remote.git");
        for d in [&repo, &root.join("herdr"), &bin, &state] {
            fs::create_dir_all(d).unwrap();
        }
        // -b main: runner の init.defaultBranch に依らず初期ブランチを固定。
        git(&repo, &["init", "-qb", "main"]);
        config_repo(&repo);
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        git(&root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&repo, &["push", "-q", "origin", "main"]);
        // bare の HEAD は runner の init.defaultBranch に従う。shallow clone
        // fixture が解決できるよう明示的に固定する。
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        write_exec(&bin.join("herdr"), herdr_stub);
        fs::write(root.join("reason"), "busy").unwrap();
        Self {
            tmp,
            root,
            repo,
            bin,
            state,
            remote,
        }
    }

    pub fn set_herdr(&self, stub: &str) {
        write_exec(&self.bin.join("herdr"), stub);
    }

    pub fn set_reason(&self, r: &str) {
        fs::write(self.root.join("reason"), r).unwrap();
    }

    pub fn calls(&self) -> usize {
        fs::read_to_string(self.root.join("calls"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    pub fn state_json(&self) -> PathBuf {
        self.state.join("state.json")
    }

    /// `ghr/github.com/acme/shallow-repo`: file:// で本物の shallow clone。
    pub fn add_shallow_repo(&self) -> PathBuf {
        let shallow = self.root.join("ghr/github.com/acme/shallow-repo");
        fs::create_dir_all(&shallow).unwrap();
        // file:// でないと git が local clone 最適化で --depth を無視する。
        git(
            &self.root,
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                &format!("file://{}", self.remote.display()),
                shallow.to_str().unwrap(),
            ],
        );
        config_repo(&shallow);
        assert_eq!(
            git_trim(&shallow, &["rev-parse", "--is-shallow-repository"]),
            "true",
            "shallow fixture 自体が shallow にならなかった"
        );
        shallow
    }

    pub fn command(&self) -> Command {
        let mut c = base_cmd(env!("CARGO_BIN_EXE_git-audit-worktrees"));
        c.env("GIT_WORKTREE_AUDIT_GHR_DIR", self.root.join("ghr"))
            .env("GIT_WORKTREE_AUDIT_HERDR_DIR", self.root.join("herdr"))
            .env("GIT_WORKTREE_AUDIT_STATE_DIR", &self.state)
            .env("GIT_WORKTREE_AUDIT_HERDR_BIN", self.bin.join("herdr"))
            .env("GIT_WORKTREE_AUDIT_TEST_CALLS", self.root.join("calls"))
            .env("GIT_WORKTREE_AUDIT_TEST_REASON", self.root.join("reason"));
        c
    }

    pub fn run(&self, args: &[&str]) -> Out {
        finish(self.command().args(args))
    }
}

pub fn finish(c: &mut Command) -> Out {
    let o = c.output().unwrap();
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// PATH 上の実 `git` の絶対パス。
pub fn real_git() -> String {
    for d in std::env::var("PATH").unwrap().split(':') {
        let p = Path::new(d).join("git");
        if p.is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    panic!("git not found in PATH");
}

/// `awk -F'\t' '$4==p'` の最後の列(evidence)。無ければ None。
pub fn evidence_for_path(out: &str, path: &Path) -> Option<String> {
    let p = path.to_str().unwrap();
    out.lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f.len() > 3 && f[3] == p)
        .map(|f| f.last().unwrap().to_string())
}

/// `awk -F'\t' -v k=branch -v b=… '$1==k && $5==b'` の最後の列。
pub fn evidence_for_branch(out: &str, branch: &str) -> Option<String> {
    out.lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f.len() > 4 && f[0] == "branch" && f[4] == branch)
        .map(|f| f.last().unwrap().to_string())
}
