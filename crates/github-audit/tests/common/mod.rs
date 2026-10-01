//! 統合テストの共通部品(bash 版 `scripts/github-audit --selftest` の写し)。
//!
//! fixture は `tests/fixtures/<stage>/` に置く。bash 版 selftest の heredoc を
//! 機械的に抜き出したもので、段(main → snapshot → releaser → archived)ごとに
//! 同じ一時ディレクトリへ重ね書きする(bash 版が同じ `$tmp/fixtures` を順に
//! 上書きしていたのと同じ)。`gh` スタブも bash 版と同じもの(`--jq` を適用
//! せず、`api` の第 2 引数の `/` を `_` にしたファイル名の fixture を返す)。
//! スタブの実行に `bash` が、差分テストで bash 版を動かすのに `jq` が要る
//! (bash 版 selftest と同じ前提)。
#![allow(dead_code)]

use github_audit::{Config, Gh};
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

/// bash 版 selftest の `$tmp/bin/gh`。FIXTURES は呼び出し元プロセスの環境に
/// 頼らず埋め込む(ライブラリを直接呼ぶテストは環境変数を渡せないため)。
fn gh_stub(fixtures: &Path) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
FIXTURES="{}"
case "$1" in
  repo)
    cat "$FIXTURES/repo-list.json"
    ;;
  api)
    if [[ "$2" == "graphql" ]]; then
      cat "$FIXTURES/graphql-response.json"
    else
      key="$(printf '%s' "$2" | tr '/' '_')"
      if [[ -f "$FIXTURES/$key.json" ]]; then
        cat "$FIXTURES/$key.json"
      else
        printf '[]'
      fi
    fi
    ;;
  *)
    exit 1
    ;;
esac
"#,
        fixtures.display()
    )
}

fn fixtures_src(stage: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(stage)
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            continue;
        }
        fs::copy(&p, dst.join(e.file_name())).unwrap();
    }
}

pub struct Fx {
    pub dir: tempfile::TempDir,
}

impl Fx {
    /// bash 版 selftest の冒頭(fixtures / gh スタブ / config の .tsv 群)。
    pub fn new() -> Fx {
        let fx = Fx {
            dir: tempfile::tempdir().unwrap(),
        };
        copy_dir(&fixtures_src("main"), &fx.p("fixtures"));
        fs::create_dir_all(fx.p("state")).unwrap();
        fs::create_dir_all(fx.p("config")).unwrap();
        write_exec(&fx.p("bin/gh"), &gh_stub(&fx.p("fixtures")));
        fx.write("config/overrides.tsv", "exempted\t*\texempt\n");
        // ADR-0020 の閉語彙。*.local.tsv は作らない(PRIVATE 分が無い実機と同じ)。
        fx.write("config/descriptive-species.tsv", "toolbox\tfixture\n");
        fx.write(
            "config/codename-registry.tsv",
            "regcode\tfixture\t2020-01-01\n",
        );
        fx.write("config/site-domains.tsv", "known.site\t2020-01-01\n");
        fx.write("config/lifecycle-species.tsv", "study\nresearch\nexam\n");
        fx.write(
            "config/routines-auditor-sources.tsv",
            "tarotene/routines-ok\n",
        );
        fx
    }

    /// 段の fixture を重ねる(`state/` 配下は `$tmp/state` へ)。
    pub fn overlay(&self, stage: &str) {
        let src = fixtures_src(stage);
        copy_dir(&src, &self.p("fixtures"));
        if src.join("state").is_dir() {
            copy_dir(&src.join("state"), &self.p("state"));
        }
    }

    pub fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.p(rel);
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    /// bash 版 selftest の `export GITHUB_AUDIT_*` ブロック。workflows の
    /// 正本は、bash 版を checkout から動かしたときに解決される実ファイルを
    /// 両実装に明示する(Rust のバイナリは target/ 配下にあり相対解決できない)。
    pub fn envs(&self) -> Vec<(String, String)> {
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        let c = |f: &str| s(self.p(&format!("config/{f}")));
        vec![
            ("GITHUB_AUDIT_OWNER".into(), "tarotene".into()),
            ("GITHUB_AUDIT_GH_BIN".into(), s(self.p("bin/gh"))),
            ("GITHUB_AUDIT_STATE_DIR".into(), s(self.p("state"))),
            ("GITHUB_AUDIT_OVERRIDES_FILE".into(), c("overrides.tsv")),
            ("GITHUB_AUDIT_TEST_FIXTURES".into(), s(self.p("fixtures"))),
            (
                "GITHUB_AUDIT_DESCRIPTIVE_SPECIES_FILE".into(),
                c("descriptive-species.tsv"),
            ),
            (
                "GITHUB_AUDIT_CODENAME_REGISTRY_FILE".into(),
                c("codename-registry.tsv"),
            ),
            (
                "GITHUB_AUDIT_CODENAME_REGISTRY_LOCAL_FILE".into(),
                c("codename-registry.local.tsv"),
            ),
            (
                "GITHUB_AUDIT_SITE_DOMAINS_FILE".into(),
                c("site-domains.tsv"),
            ),
            (
                "GITHUB_AUDIT_SITE_DOMAINS_LOCAL_FILE".into(),
                c("site-domains.local.tsv"),
            ),
            (
                "GITHUB_AUDIT_LIFECYCLE_SPECIES_FILE".into(),
                c("lifecycle-species.tsv"),
            ),
            (
                "GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_FILE".into(),
                c("routines-auditor-sources.tsv"),
            ),
            (
                "GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_LOCAL_FILE".into(),
                c("routines-auditor-sources.local.tsv"),
            ),
            (
                "GITHUB_AUDIT_WORKFLOWS_CANONICAL_QUALITY_FILE".into(),
                s(repo_root().join(
                    "config/claude/skills/repo-governance-common/templates/.github/rulesets/quality.json",
                )),
            ),
        ]
    }

    fn run(&self, prog: &Path, args: &[&str], extra: &[(&str, &str)]) -> Out {
        let mut cmd = Command::new(prog);
        cmd.args(args).stdin(Stdio::null());
        // ホスト側の GITHUB_AUDIT_* に左右されない。
        for (k, _) in std::env::vars() {
            if k.starts_with("GITHUB_AUDIT_") {
                cmd.env_remove(&k);
            }
        }
        for (k, v) in self.envs() {
            cmd.env(k, v);
        }
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let o = cmd.output().unwrap();
        Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    /// Rust 版バイナリ。
    pub fn rust(&self, args: &[&str], extra: &[(&str, &str)]) -> Out {
        self.run(Path::new(env!("CARGO_BIN_EXE_github-audit")), args, extra)
    }

    /// 比較対象の bash 版(依存側の移植が終わるまでリポジトリに残る)。
    pub fn bash(&self, args: &[&str], extra: &[(&str, &str)]) -> Out {
        self.run(&repo_root().join("scripts/github-audit"), args, extra)
    }

    /// ライブラリを直接呼ぶ用の設定(`envs()` と同じ値)。
    pub fn cfg(&self) -> Config {
        let mut c = Config::from_env();
        let get = |k: &str| {
            self.envs()
                .into_iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v)
                .unwrap()
        };
        c.owner = "tarotene".into();
        c.gh_bin = get("GITHUB_AUDIT_GH_BIN");
        c.state_dir = get("GITHUB_AUDIT_STATE_DIR").into();
        c.overrides_file = get("GITHUB_AUDIT_OVERRIDES_FILE").into();
        c.repo_limit = "500".into();
        c.viewer_permission = None;
        c.descriptive_species_file = get("GITHUB_AUDIT_DESCRIPTIVE_SPECIES_FILE").into();
        c.codename_registry_file = get("GITHUB_AUDIT_CODENAME_REGISTRY_FILE").into();
        c.codename_registry_local_file = get("GITHUB_AUDIT_CODENAME_REGISTRY_LOCAL_FILE").into();
        c.site_domains_file = get("GITHUB_AUDIT_SITE_DOMAINS_FILE").into();
        c.site_domains_local_file = get("GITHUB_AUDIT_SITE_DOMAINS_LOCAL_FILE").into();
        c.lifecycle_species_file = get("GITHUB_AUDIT_LIFECYCLE_SPECIES_FILE").into();
        c.routines_auditor_sources_file = get("GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_FILE").into();
        c.routines_auditor_sources_local_file =
            get("GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_LOCAL_FILE").into();
        c.workflows_canonical_quality_file =
            get("GITHUB_AUDIT_WORKFLOWS_CANONICAL_QUALITY_FILE").into();
        c.app_snapshot_file = self.p("state/app-snapshot.json");
        c.releaser_app_name = "releaser".into();
        c.renovate_policy_preset = "github>tarotene/dotfiles//renovate/policy".into();
        c.lifecycle_now = None;
        c
    }

    pub fn gh(&self) -> Gh {
        Gh::new(&self.p("bin/gh").to_string_lossy(), "tarotene")
    }
}
