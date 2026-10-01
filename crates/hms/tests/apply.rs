//! hms の統合テスト: PATH 上のスタブ(nix / home-manager / systemctl /
//! tailscale-prefs)で適用 runbook の流れと、wrapper 経路(ADR-0034)の引数組み立て・
//! 降格 guard(#567)・失敗時の縮退を固定する。bash 版 selftest(純関数 16 ケース)は
//! `src/lib.rs` の単体テストに写してあり、ここは bash 版が selftest で触れていなかった
//! 副作用側のテスト。実環境の `$HOME` / nix / systemd には触れない。
//! `HMS_UNDER_TEST` に bash 版(`scripts/hms.sh`)を渡すと同じケースを bash 版に流せる
//! (bash 版は jq を使うので、テストが実 jq を stub ディレクトリへ symlink する)。

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MAIN: &str = "github:tarotene/dotfiles";
const WRAPPER: &str = "git+https://example.invalid/wrapper";

fn under_test() -> PathBuf {
    std::env::var_os("HMS_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_hms")))
}

fn exe(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

struct T {
    dir: tempfile::TempDir,
}

impl T {
    fn new() -> Self {
        let t = T {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(t.stubs().join("meta")).unwrap();
        fs::create_dir_all(t.home().join(".config/dotfiles")).unwrap();
        fs::write(t.home().join(".config/dotfiles/host"), "testhost\n").unwrap();
        if let Some(jq) = find_in_path("jq") {
            symlink(jq, t.stubs().join("jq")).unwrap();
        }
        let s = t.stubs();
        // nix: `flake metadata [--refresh] --json <ref>` → meta/<ref を _ に置換>。無ければ失敗。
        exe(
            &s.join("nix"),
            r#"printf 'nix %s\n' "$*" >> "$STUB_DIR/calls.log"
ref="${*: -1}"
f="$STUB_DIR/meta/$(printf %s "$ref" | tr '/:+' '___')"
[[ -f $f ]] && { cat "$f"; exit 0; }
exit 1"#,
        );
        exe(
            &s.join("home-manager"),
            r#"printf 'home-manager %s\n' "$*" >> "$STUB_DIR/calls.log"
exit "${HM_RC:-0}""#,
        );
        exe(
            &s.join("systemctl"),
            r#"printf 'systemctl %s\n' "$*" >> "$STUB_DIR/calls.log"
case "$*" in
  *" cat "*) exit "${CAT_RC:-0}" ;;
  *is-active*) exit "${ACTIVE_RC:-0}" ;;
  *"show -p MainPID"*) printf '%s\n' "${MAINPID:-4242}" ;;
esac
exit 0"#,
        );
        t
    }
    fn stubs(&self) -> PathBuf {
        self.dir.path().join("stubs")
    }
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }
    fn meta(&self, flake: &str, json: &str) {
        let name: String = flake
            .chars()
            .map(|c| if "/:+".contains(c) { '_' } else { c })
            .collect();
        fs::write(self.stubs().join("meta").join(name), json).unwrap();
    }
    fn marker(&self, name: &str, value: &str) {
        fs::write(
            self.home().join(".config/dotfiles").join(name),
            format!("{value}\n"),
        )
        .unwrap();
    }
    fn checkout(&self) -> PathBuf {
        let d = self.dir.path().join("checkout");
        fs::create_dir_all(&d).unwrap();
        fs::canonicalize(d).unwrap()
    }
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut c = Command::new(under_test());
        c.args(args)
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs().display()))
            .env("STUB_DIR", self.stubs());
        for (k, v) in env {
            c.env(k, v);
        }
        c.output().unwrap()
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.stubs().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
    fn hm_call(&self) -> Option<String> {
        self.calls()
            .into_iter()
            .find(|l| l.starts_with("home-manager "))
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// 既定 ref(wrapper 無し)= pushed main: refresh → revision 表示 → switch → fcitx5 まで。
#[test]
fn default_apply_runs_the_whole_runbook() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"deadbeef"}"#);
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    let so = out(&o);
    assert!(so.contains(&format!("==> nix flake metadata --refresh {MAIN}\n")));
    assert!(so.contains("==> applying revision deadbeef\n"));
    // extra_opts が空でも末尾の空白は残る(bash の `${extra_opts[*]}`)。
    assert!(so.contains(&format!(
        "==> home-manager switch --flake {MAIN}#testhost -b backup \n"
    )));
    assert!(so.contains("==> systemctl --user daemon-reload\n"));
    assert!(so.contains("==> systemctl --user restart app-fcitx5@autostart.service\n"));
    assert!(so.contains("==> fcitx5 running (pid 4242) from "));
    assert!(so.ends_with("Done.\n"));
    assert_eq!(
        t.hm_call().unwrap(),
        format!("home-manager switch --flake {MAIN}#testhost -b backup")
    );
    let calls = t.calls();
    let pos = |needle: &str| calls.iter().position(|l| l.contains(needle)).unwrap();
    assert!(pos("home-manager") < pos("daemon-reload"));
    assert!(pos("daemon-reload") < pos("restart"));
    assert!(pos("restart") < pos("is-active"));
}

/// `hms <path>`、wrapper 無し: refresh せず warn-dirty を切って path 単体を適用。
#[test]
fn local_apply_without_wrapper() {
    let t = T::new();
    let d = t.checkout();
    let o = t.run(&[d.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        t.hm_call().unwrap(),
        format!(
            "home-manager switch --flake {}#testhost -b backup --option warn-dirty false",
            d.display()
        )
    );
    assert!(!t.calls().iter().any(|l| l.contains("--refresh")));
    assert!(!out(&o).contains("note: a nix"));
}

/// wrapper 登録ホスト + ローカルパス → wrapper 経由で `dotfiles` をそのパスに override。
#[test]
fn local_apply_on_wrapper_host_routes_through_wrapper() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    let d = t.checkout();
    let o = t.run(&[d.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        t.hm_call().unwrap(),
        format!(
            "home-manager switch --flake {WRAPPER}#testhost -b backup --option warn-dirty false --override-input dotfiles path:{} --no-write-lock-file",
            d.display()
        )
    );
    assert!(out(&o)
        .contains("==> note: a nix \"warning: not writing modified lock file\" below is expected"));
}

/// `--public-only` は wrapper ホストでも checkout 単体を適用する。
#[test]
fn public_only_skips_routing() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    let d = t.checkout();
    let o = t.run(&[d.to_str().unwrap(), "--public-only"], &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        t.hm_call().unwrap(),
        format!(
            "home-manager switch --flake {}#testhost -b backup --option warn-dirty false",
            d.display()
        )
    );
}

const WRAPPER_META: &str = r#"{"revision":"wrev","locks":{"nodes":{
    "root":{"inputs":{"dotfiles":"dotfiles"}},
    "dotfiles":{"locked":{"rev":"oldrev","type":"github"}}}}}"#;

/// wrapper の lock が古い rev を pin → pushed main の revision で override。
#[test]
fn wrapper_lock_is_overridden_with_pushed_main() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    t.meta(WRAPPER, WRAPPER_META);
    t.meta(MAIN, r#"{"revision":"newrev"}"#);
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    let so = out(&o);
    assert!(so.contains("==> applying revision wrev\n"));
    assert!(so.contains(
        "==> dotfiles revision newrev (overriding the wrapper's lock, which pins oldrev)\n"
    ));
    assert_eq!(
        t.hm_call().unwrap(),
        format!(
            "home-manager switch --flake {WRAPPER}#testhost -b backup --override-input dotfiles github:tarotene/dotfiles/newrev --no-write-lock-file"
        )
    );
}

#[test]
fn wrapper_lock_already_current() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    t.meta(WRAPPER, WRAPPER_META);
    t.meta(MAIN, r#"{"revision":"oldrev"}"#);
    let o = t.run(&[], &[]);
    assert!(out(&o)
        .contains("==> dotfiles revision oldrev (wrapper's lock already at this revision)\n"));
}

/// main が解決できない(offline)→ wrapper の lock をそのまま適用(override 無し)。
#[test]
fn wrapper_lock_applied_as_is_when_main_unresolvable() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    t.meta(WRAPPER, WRAPPER_META);
    let o = t.run(&[], &[]);
    assert!(o.status.success());
    assert!(err(&o).contains(&format!(
        "==> could not resolve {MAIN} (offline?); applying the wrapper's lock as-is (dotfiles oldrev)"
    )));
    assert_eq!(
        t.hm_call().unwrap(),
        format!("home-manager switch --flake {WRAPPER}#testhost -b backup")
    );
}

/// revision を解決できない(offline)→ 警告して switch は続行。
#[test]
fn unresolvable_revision_warns_and_continues() {
    let t = T::new();
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains(&format!(
        "==> could not resolve a revision for {MAIN} (offline?); continuing with whatever switch resolves"
    )));
    assert!(t.hm_call().is_some());
}

/// 降格 guard(#567): 前世代に private-hub マーカーがあるのに今回未登録 → 中止。
fn with_prev_generation_marker(t: &T) {
    let gen = t.dir.path().join("gen-1");
    fs::create_dir_all(gen.join("home-files/.config/dotfiles")).unwrap();
    fs::write(gen.join("home-files/.config/dotfiles/private-hub"), "x").unwrap();
    let profiles = t.home().join(".local/state/nix/profiles");
    fs::create_dir_all(&profiles).unwrap();
    symlink(&gen, profiles.join("home-manager")).unwrap();
}

#[test]
fn downgrade_guard_aborts() {
    let t = T::new();
    with_prev_generation_marker(&t);
    t.meta(MAIN, r#"{"revision":"r"}"#);
    let o = t.run(&[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o)
        .contains("Error: 直前の generation は private wrapper flake 経由で適用済みでしたが、"));
    assert!(err(&o).contains("printf '%s\\n' '<wrapper flake ref>'"));
    assert!(t.hm_call().is_none());
}

#[test]
fn downgrade_guard_allows_public_only_and_registered_wrapper() {
    let t = T::new();
    with_prev_generation_marker(&t);
    t.meta(MAIN, r#"{"revision":"r"}"#);
    let o = t.run(&["--public-only"], &[]);
    assert!(o.status.success(), "{}", err(&o));
    let t2 = T::new();
    with_prev_generation_marker(&t2);
    t2.marker("private-hub", WRAPPER);
    t2.meta(WRAPPER, r#"{"revision":"w"}"#);
    let o = t2.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
}

/// home-manager の失敗はその終了コードで終わり、後処理(systemctl)は走らない。
/// profile と current-home が食い違えば「今回も失敗」文言つきの警告を出す。
#[test]
fn switch_failure_propagates_and_warns_on_generation_mismatch() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"r"}"#);
    let a = t.dir.path().join("gen-a");
    let b = t.dir.path().join("gen-b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let profiles = t.home().join(".local/state/nix/profiles");
    fs::create_dir_all(&profiles).unwrap();
    symlink(&a, profiles.join("home-manager")).unwrap();
    let gc = t.home().join(".local/state/home-manager/gcroots");
    fs::create_dir_all(&gc).unwrap();
    symlink(&b, gc.join("current-home")).unwrap();
    let o = t.run(&[], &[("HM_RC", "7")]);
    assert_eq!(o.status.code(), Some(7));
    let e = err(&o);
    assert!(e.contains("Warning: home-manager generation/reality mismatch detected."));
    assert!(e.contains("A successful switch (this one) will resolve it."));
    assert!(e.contains("This switch also failed, so the mismatch remains"));
    assert!(!t.calls().iter().any(|l| l.starts_with("systemctl")));
}

#[test]
fn fcitx5_unit_absent_skips_restart() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"r"}"#);
    let o = t.run(&[], &[("CAT_RC", "1")]);
    assert!(o.status.success());
    assert!(out(&o)
        .contains("==> app-fcitx5@autostart.service not present; skipping fcitx5 restart.\n"));
    assert!(out(&o).ends_with("Done.\n"));
    assert!(!t.calls().iter().any(|l| l.contains("restart")));
}

#[test]
fn inactive_unit_after_restart_fails() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"r"}"#);
    let o = t.run(&[], &[("ACTIVE_RC", "3")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("Error: app-fcitx5@autostart.service is not active after the restart.")
    );
    assert!(t.calls().iter().any(|l| l.contains("status --no-pager")));
}

#[test]
fn active_unit_without_mainpid_fails() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"r"}"#);
    let o = t.run(&[], &[("MAINPID", "0")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("is active but has no MainPID."));
}

/// tailscale-prefs があれば switch 直後・daemon-reload 前に `apply` を呼ぶ。
#[test]
fn tailscale_prefs_runs_when_present() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"r"}"#);
    exe(
        &t.stubs().join("tailscale-prefs"),
        r#"printf 'tailscale-prefs %s\n' "$*" >> "$STUB_DIR/calls.log""#,
    );
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    let calls = t.calls();
    let ts = calls
        .iter()
        .position(|l| l == "tailscale-prefs apply")
        .unwrap();
    let hm = calls
        .iter()
        .position(|l| l.starts_with("home-manager"))
        .unwrap();
    let dr = calls
        .iter()
        .position(|l| l.contains("daemon-reload"))
        .unwrap();
    assert!(hm < ts && ts < dr);
}

#[test]
fn host_marker_wins_over_hostname_and_whitespace_is_stripped() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"r"}"#);
    fs::write(
        t.home().join(".config/dotfiles/host"),
        "  star ship \nignored\n",
    )
    .unwrap();
    let o = t.run(&[], &[]);
    assert!(o.status.success());
    assert_eq!(
        t.hm_call().unwrap(),
        format!("home-manager switch --flake {MAIN}#starship -b backup")
    );
}

#[test]
fn unknown_option_and_help() {
    let t = T::new();
    let o = t.run(&["--bogus"], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("Error: Unknown option: --bogus"));
    let o = t.run(&["--help"], &[]);
    assert!(o.status.success());
    assert!(out(&o).starts_with("Usage: hms [flake-ref] [--public-only]\n\nApply the home-manager configuration for this host.\n"));
    assert!(out(&o).contains(&format!("  hms          apply pushed main ({MAIN})\n")));
    assert!(!out(&o).contains("Routed through"));
    t.marker("private-hub", WRAPPER);
    let o = t.run(&["-h"], &[]);
    assert!(out(&o).contains(&format!("apply pushed main ({WRAPPER})")));
    assert!(out(&o).contains(&format!(
        "Routed through {WRAPPER} with dotfiles overridden to"
    )));
    assert!(t.hm_call().is_none());
}
