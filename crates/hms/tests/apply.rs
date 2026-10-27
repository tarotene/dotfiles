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
        // 目標の世代の既定(`nix eval` が返し、`home-manager` が向ける先)。
        let default_target = t.generation("default", "default-rev");
        fs::write(s.join("target"), default_target.to_str().unwrap()).unwrap();
        // nix: `flake metadata [--refresh] --json <ref>` → meta/<ref を _ に置換>。無ければ失敗。
        exe(
            &s.join("nix"),
            r#"printf 'nix %s\n' "$*" >> "$STUB_DIR/calls.log"
trap 'printf "nix rc=%s\n" "$?" >> "$STUB_DIR/rc.log"' EXIT
[[ $1 == build ]] && exit "${BUILD_RC:-0}"
# eval: 今回の目標の世代(`activationPackage.outPath`)。EVAL_HOLD は評価の最中で止まり、
# `eval-go` が置かれるまで待つ(待ちには上限を付け、テストが先に失敗しても居残らない)。
if [[ $1 == eval ]]; then
  [[ -n ${EVAL_RC:-} ]] && { echo 'error: evaluation failed' >&2; exit "$EVAL_RC"; }
  if [[ -n ${EVAL_HOLD:-} ]]; then
    : > "$STUB_DIR/eval-started"
    for _ in $(seq 400); do [[ -f $STUB_DIR/eval-go ]] && break; sleep 0.05; done
  fi
  printf '%s' "${NIX_TARGET:-$(cat "$STUB_DIR/target")}"
  exit 0
fi
ref="${*: -1}"
f="$STUB_DIR/meta/$(printf %s "$ref" | tr '/:+' '___')"
[[ -f $f ]] && { cat "$f"; exit 0; }
exit 1"#,
        );
        exe(
            &s.join("home-manager"),
            // 成功した switch は profile と current-home を世代へ向ける(実物は `nix-env --set` と
            // activation script)。HM_LANDS で別の世代へ向けると別セッションの適用を、
            // HM_ONLY_PROFILE で activation 前の状態を再現する。LAND_HOLD は向けた直後で止まり、
            // `go` が置かれるまで待つ。
            // HM_HOLD: switch の最中(世代を向ける前)で止まり(`started` を置く)、`release` が
            // 置かれるまで待つ — 2 つ目の hms を重ねて、排他を確かめる。待ちには上限を付け、
            // テストが先に失敗しても stub が居残らないようにする。order.log は start/end の並び。
            r#"printf 'home-manager %s\n' "$*" >> "$STUB_DIR/calls.log"
printf 'start\n' >> "$STUB_DIR/order.log"
if [[ -n ${HM_HOLD:-} ]]; then
  : > "$STUB_DIR/started"
  for _ in $(seq 400); do [[ -f $STUB_DIR/release ]] && break; sleep 0.05; done
fi
if [[ ${HM_RC:-0} == 0 ]]; then
  gen="${HM_LANDS:-$(cat "$STUB_DIR/target")}"
  mkdir -p "$HOME/.local/state/nix/profiles" "$HOME/.local/state/home-manager/gcroots"
  ln -sfn "$gen" "$HOME/.local/state/nix/profiles/home-manager"
  [[ -n ${HM_ONLY_PROFILE:-} ]] || ln -sfn "$gen" "$HOME/.local/state/home-manager/gcroots/current-home"
  if [[ -n ${LAND_HOLD:-} ]]; then
    : > "$STUB_DIR/landed"
    for _ in $(seq 400); do [[ -f $STUB_DIR/go ]] && break; sleep 0.05; done
  fi
fi
[[ -n ${HM_BG:-} ]] && { nohup sleep 3 >/dev/null 2>&1 & }
printf 'end\n' >> "$STUB_DIR/order.log"
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
    /// 世代の実体(`home-files/.local/share/dotfiles/rev` つき)を作り、正準パスを返す。
    fn generation(&self, name: &str, rev: &str) -> PathBuf {
        let g = self.dir.path().join("gen").join(name);
        fs::create_dir_all(g.join("home-files/.local/share/dotfiles")).unwrap();
        fs::write(
            g.join("home-files/.local/share/dotfiles/rev"),
            format!("{rev}\n"),
        )
        .unwrap();
        fs::canonicalize(g).unwrap()
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
    /// `run` と同じ環境で子を起動したまま返す(排他のテストで 2 つ重ねる)。stderr は
    /// `stderr_to` のファイルへ流す — パイプだと読む側が居ない間に詰まりうる。
    fn spawn(&self, args: &[&str], env: &[(&str, &str)], stderr_to: &Path) -> std::process::Child {
        let mut c = Command::new(under_test());
        c.args(args)
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs().display()))
            .env("STUB_DIR", self.stubs())
            .stdout(std::process::Stdio::null())
            .stderr(fs::File::create(stderr_to).unwrap());
        for (k, v) in env {
            c.env(k, v);
        }
        c.spawn().unwrap()
    }
    fn state(&self) -> PathBuf {
        self.home().join(".local/state/hms")
    }
    /// `last-apply.json`(無ければ None)。
    fn record(&self) -> Option<serde_json::Value> {
        let text = fs::read_to_string(self.state().join("last-apply.json")).ok()?;
        Some(serde_json::from_str(&text).expect("the record is valid json"))
    }
    fn order(&self) -> Vec<String> {
        fs::read_to_string(self.stubs().join("order.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.stubs().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
    /// 失敗した assertion に付ける診断: hms の終了状態・stdout・stderr と、スタブが
    /// 受けた呼び出し・nix スタブの終了コード。断続的な失敗(スタブを起動できなかった
    /// のか、起動したが非ゼロで終わったのか)を assertion の出力から切り分けられる(#1036)。
    fn diag(&self, o: &Output) -> String {
        format!(
            "status: {}\nstdout: {}\nstderr: {}\ncalls:\n{}\nnix rc: {:?}",
            o.status,
            out(o),
            err(o),
            self.calls().join("\n"),
            fs::read_to_string(self.stubs().join("rc.log")).unwrap_or_default(),
        )
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
    assert!(err(&o).contains(&format!(
        "==> could not resolve a revision for {MAIN} (offline?); continuing with whatever switch resolves"
    )));
    assert!(t.hm_call().is_some());
}

const BLEEP: &str = "github:tarotene/bleep";
const PUBLIC_META: &str = r#"{"revision":"mrev","locks":{"nodes":{
    "root":{"inputs":{"bleep":"bleep"}},
    "bleep":{"locked":{"rev":"oldbleep","type":"github"}}}}}"#;

fn bleep_main(t: &T, rev: &str) {
    t.meta(
        BLEEP,
        &format!(r#"{{"locked":{{"rev":"{rev}","type":"github"}}}}"#),
    );
}

/// bleep main が lock の pin より先 + 事前 build 成功 → `--override-input bleep` で適用。
#[test]
fn bleep_main_is_overridden_after_a_passing_prebuild() {
    let t = T::new();
    t.meta(MAIN, PUBLIC_META);
    bleep_main(&t, "newbleep");
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", t.diag(&o));
    assert!(out(&o).contains(
        "==> bleep revision newbleep (overriding the lock pin oldbleep; canary passed)\n"
    ));
    let calls = t.calls();
    let build = calls.iter().find(|l| l.starts_with("nix build ")).unwrap();
    assert!(build.contains("--override-input bleep github:tarotene/bleep/newbleep"));
    assert_eq!(
        t.hm_call().unwrap(),
        format!(
            "home-manager switch --flake {MAIN}#testhost -b backup --override-input bleep github:tarotene/bleep/newbleep --no-write-lock-file"
        )
    );
}

/// 事前 build が落ちる(canary 不通過)→ 警告して lock の pin のまま適用。
#[test]
fn bleep_main_falls_back_to_the_pin_when_prebuild_fails() {
    let t = T::new();
    t.meta(MAIN, PUBLIC_META);
    bleep_main(&t, "newbleep");
    let o = t.run(&[], &[("BUILD_RC", "1")]);
    assert!(o.status.success(), "{}", t.diag(&o));
    assert!(err(&o).contains("Warning: bleep main newbleep failed to build or failed the deployed-hook canary; applying the locked pin oldbleep instead."));
    assert_eq!(
        t.hm_call().unwrap(),
        format!("home-manager switch --flake {MAIN}#testhost -b backup")
    );
}

/// lock が既に main と同じ → build も override も無い。
#[test]
fn bleep_lock_already_current_does_not_build() {
    let t = T::new();
    t.meta(MAIN, PUBLIC_META);
    bleep_main(&t, "oldbleep");
    let o = t.run(&[], &[]);
    assert!(out(&o).contains("==> bleep revision oldbleep (lock already at this revision)\n"));
    assert!(!t.calls().iter().any(|l| l.starts_with("nix build ")));
}

/// wrapper 経由は入力名が `dotfiles/bleep`。
#[test]
fn bleep_override_goes_through_the_wrapper_input() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    t.meta(
        WRAPPER,
        r#"{"revision":"wrev","locks":{"nodes":{
            "root":{"inputs":{"dotfiles":"dotfiles"}},
            "dotfiles":{"inputs":{"bleep":"bleep"},"locked":{"rev":"oldrev","type":"github"}},
            "bleep":{"locked":{"rev":"oldbleep","type":"github"}}}}}"#,
    );
    t.meta(MAIN, r#"{"revision":"oldrev"}"#);
    bleep_main(&t, "newbleep");
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", t.diag(&o));
    let hm = t.hm_call().unwrap();
    assert!(
        hm.contains("--override-input dotfiles/bleep github:tarotene/bleep/newbleep"),
        "{hm}\n{}",
        t.diag(&o)
    );
}

/// `hms .` / `hms <path>` は checkout の pin をそのまま検証する — bleep を触らない。
#[test]
fn local_apply_does_not_touch_bleep() {
    let t = T::new();
    bleep_main(&t, "newbleep");
    let d = t.checkout();
    let o = t.run(&[d.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", t.diag(&o));
    assert!(!t
        .calls()
        .iter()
        .any(|l| l.contains("prefetch") || l.starts_with("nix build ")));
    assert!(!t.hm_call().unwrap().contains("bleep"));
}

/// bleep main を解決できない(offline)→ 警告して lock の pin のまま。
#[test]
fn bleep_unresolvable_applies_the_pin() {
    let t = T::new();
    t.meta(MAIN, PUBLIC_META);
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", t.diag(&o));
    assert!(err(&o).contains(
        "==> could not resolve github:tarotene/bleep (offline?); applying the locked bleep pin as-is (bleep oldbleep)"
    ));
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
    assert!(o.status.success(), "{}", t.diag(&o));
    let t2 = T::new();
    with_prev_generation_marker(&t2);
    t2.marker("private-hub", WRAPPER);
    t2.meta(WRAPPER, r#"{"revision":"w"}"#);
    let o = t2.run(&[], &[]);
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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
    assert!(o.status.success(), "{}", t.diag(&o));
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

// ---------------------------------------------------------------------------
// 適用後の世代の照合(#1025)。switch の成功応答だけでは、今回の世代が有効とは言えない。
// 別セッションの適用は、スタブの `home-manager` をロックなしで直接呼んで再現する
// (ロックを持たない古い hms や手で実行した switch と同じ — hms 同士なら排他が先に効く)。
// ---------------------------------------------------------------------------

impl T {
    /// `run` と同じ環境で hms を起動したまま返す。出力はファイルへ流す(パイプだと
    /// 読む側が居ない間に詰まりうる)。
    fn start_hms(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Child {
        let mut c = Command::new(under_test());
        c.args(args)
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs().display()))
            .env("STUB_DIR", self.stubs())
            .stdout(fs::File::create(self.dir.path().join("hms.out")).unwrap())
            .stderr(fs::File::create(self.dir.path().join("hms.err")).unwrap());
        for (k, v) in env {
            c.env(k, v);
        }
        c.spawn().unwrap()
    }
    fn started_out(&self) -> String {
        fs::read_to_string(self.dir.path().join("hms.out")).unwrap_or_default()
    }
    fn started_err(&self) -> String {
        fs::read_to_string(self.dir.path().join("hms.err")).unwrap_or_default()
    }
    /// 別セッションの適用: hms を通さず、スタブの `home-manager switch` が世代 `to` を有効にする。
    fn other_session_lands(&self, to: &Path) {
        let st = Command::new(self.stubs().join("home-manager"))
            .args(["switch", "--flake", "other#testhost"])
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs().display()))
            .env("STUB_DIR", self.stubs())
            .env("HM_LANDS", to)
            .status()
            .unwrap();
        assert!(st.success());
    }
    fn profile_target(&self) -> PathBuf {
        fs::canonicalize(self.home().join(".local/state/nix/profiles/home-manager")).unwrap()
    }
}

/// 目標の世代を switch の前に `nix eval` で求め、switch と同じ attribute・同じ追加引数を渡す。
/// switch の後に profile と current-home が目標を指すと確かめて、初めて続きへ進む。
#[test]
fn the_target_generation_is_evaluated_before_the_switch_and_verified_after() {
    let t = T::new();
    t.marker("private-hub", WRAPPER);
    let d = t.checkout();
    let o = t.run(&[d.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", err(&o));
    let calls = t.calls();
    let pos = |p: &str| calls.iter().position(|l| l.starts_with(p)).unwrap();
    assert!(pos("nix eval") < pos("home-manager "));
    // switch の `--flake <ref>#<host>` と同じ ref・host・追加引数で評価している。
    assert_eq!(
        calls[pos("nix eval")],
        format!(
            "nix eval --raw {WRAPPER}#homeConfigurations.testhost.activationPackage.outPath --option warn-dirty false --override-input dotfiles path:{} --no-write-lock-file",
            d.display()
        )
    );
    let target = fs::read_to_string(t.stubs().join("target")).unwrap();
    assert!(out(&o).contains(&format!(
        "世代を照合しました: profile と current-home が今回の世代を指しています ({target})"
    )));
    assert!(!err(&o).contains("待っている間に別の適用が世代を切り替えました"));
    assert!(out(&o).ends_with("Done.\n"));
}

/// 目標の世代を求められないなら、何も変えずに止まる(switch も同じ評価をする)。
#[test]
fn an_unevaluable_target_stops_before_the_switch() {
    let t = T::new();
    let o = t.run(&[], &[("EVAL_RC", "1")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("error: evaluation failed"));
    assert!(err(&o).contains("今回の適用の目標の世代を求められませんでした"));
    assert!(t.hm_call().is_none());
}

/// 再現(#1025): 今回の switch が成功を返した直後に、別セッションの適用が世代を切り替える。
/// home-manager の終了コードは 0 のままでも、hms は成功と言わず、適用元・実際の世代・
/// 次の操作を示して非 0 で終える。環境は別セッションの世代のまま(今回の変更は無効)。
#[test]
fn a_competing_apply_after_our_switch_is_detected_not_reported_as_done() {
    let t = T::new();
    let d = t.checkout();
    t.meta(d.to_str().unwrap(), r#"{"dirtyRevision":"dirty123"}"#);
    let theirs = t.generation("theirs", "their-rev");
    let ours = fs::read_to_string(t.stubs().join("target")).unwrap();

    let mut ours_apply = t.start_hms(&[d.to_str().unwrap()], &[("LAND_HOLD", "1")]);
    wait_until("our switch to take effect", || {
        t.stubs().join("landed").exists()
    });
    t.other_session_lands(&theirs);
    fs::write(t.stubs().join("go"), "").unwrap();
    let st = ours_apply.wait().unwrap();

    assert_eq!(st.code(), Some(1), "{}", t.started_err());
    let e = t.started_err();
    assert!(e.contains("switch は成功しましたが、今回の世代が有効になっていません"));
    // 適用元(worktree と rev)・目標・実際の世代・次の操作。
    assert!(e.contains(&format!(
        "適用元: {}(worktree {}、rev dirty123)",
        d.display(),
        d.display()
    )));
    assert!(e.contains(&format!("今回の目標の世代: {ours}")));
    assert!(e.contains(&format!(
        "実際の世代 profile: {}(rev their-rev)",
        theirs.display()
    )));
    assert!(e.contains(&format!("`hms {}` を", d.display())));
    // 成功の表示も、後処理(daemon-reload・fcitx5)も行わない。
    assert!(!t.started_out().contains("Done."));
    assert!(!t.started_out().contains("世代を照合しました"));
    assert!(!t.calls().iter().any(|l| l.starts_with("systemctl")));
    assert_eq!(t.profile_target(), theirs);
}

/// profile は今回の世代でも current-home が追いついていないなら、同じく成功にしない
/// (別の適用の activation が進行中、または今回の activation が反映されていない)。
#[test]
fn a_current_home_that_did_not_follow_is_detected() {
    let t = T::new();
    let stale = t.generation("stale", "stale-rev");
    let gc = t.home().join(".local/state/home-manager/gcroots");
    fs::create_dir_all(&gc).unwrap();
    symlink(&stale, gc.join("current-home")).unwrap();
    let o = t.run(&[], &[("HM_ONLY_PROFILE", "1")]);
    assert_eq!(o.status.code(), Some(1));
    let e = err(&o);
    assert!(e.contains("current-home が追いついていません"));
    assert!(e.contains(&format!(
        "実際の世代 current-home: {}(rev stale-rev)",
        stale.display()
    )));
    assert!(!out(&o).ends_with("Done.\n"));
}

/// 排他だけでは足りない場合(#1025 の完了条件): 待っていた古い変更が、後から新しい変更を戻す。
/// 今回の世代は有効なので成功だが、待っている間に別の適用が切り替えた世代を置き換えることを、
/// switch の前(止められる間)と、終わった後の両方で示す。
#[test]
fn replacing_a_generation_that_changed_during_the_wait_is_announced() {
    let t = T::new();
    let before = t.generation("before", "before-rev");
    let newer = t.generation("newer", "newer-rev");
    let profiles = t.home().join(".local/state/nix/profiles");
    fs::create_dir_all(&profiles).unwrap();
    symlink(&before, profiles.join("home-manager")).unwrap();

    let mut older = t.start_hms(&[], &[("EVAL_HOLD", "1")]);
    wait_until("our evaluation to start", || {
        t.stubs().join("eval-started").exists()
    });
    // 待っている間に、別セッションの(新しい)変更が適用される。
    t.other_session_lands(&newer);
    fs::write(t.stubs().join("eval-go"), "").unwrap();
    let st = older.wait().unwrap();

    assert!(st.success(), "{}", t.started_err());
    let e = t.started_err();
    assert!(e.contains(
        "待っている間に別の適用が世代を切り替えました。今回の switch はそれを置き換えます。"
    ));
    assert!(e.contains(&format!(
        "開始時の世代: {}(rev before-rev)",
        before.display()
    )));
    assert!(e.contains(&format!(
        "いまの世代(置き換えられる): {}(rev newer-rev)",
        newer.display()
    )));
    assert!(t.started_out().contains(&format!(
        "切り替えた世代 {}(rev newer-rev) を、今回の世代で置き換えました",
        newer.display()
    )));
    // 今回の世代は有効になっている。
    assert_eq!(
        t.profile_target(),
        PathBuf::from(fs::read_to_string(t.stubs().join("target")).unwrap())
    );
}

// ---------------------------------------------------------------------------
// 適用ロックと適用の記録(#1025。D2: 経路の記録、D3: 同時に 1 つだけ)
// ---------------------------------------------------------------------------

/// `~/.local/state/nix/profiles/home-manager` が指す世代と、その世代が置いた
/// `~/.local/share/dotfiles/rev` を用意する(switch 後の読み戻しの再現)。世代の実体の
/// 絶対パスを返す。
fn fake_generation(t: &T, rev: &str) -> PathBuf {
    let generation = t.dir.path().join("generation");
    fs::create_dir_all(&generation).unwrap();
    let profiles = t.home().join(".local/state/nix/profiles");
    fs::create_dir_all(&profiles).unwrap();
    symlink(&generation, profiles.join("home-manager")).unwrap();
    let share = t.home().join(".local/share/dotfiles");
    fs::create_dir_all(&share).unwrap();
    fs::write(share.join("rev"), format!("{rev}\n")).unwrap();
    let generation = fs::canonicalize(generation).unwrap();
    // 成功した switch は profile を目標へ向け直す。目標もこの世代に揃えて、照合も記録も
    // 同じ世代を見るようにする。
    fs::write(t.stubs().join("target"), generation.to_str().unwrap()).unwrap();
    generation
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out waiting for: {what}");
}

fn leftover_temp_files(t: &T) -> Vec<String> {
    fs::read_dir(t.state())
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains(".tmp."))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_default_apply_records_how_the_generation_was_made() {
    let t = T::new();
    t.meta(MAIN, r#"{"revision":"deadbeef"}"#);
    let generation = fake_generation(&t, "abc123");
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    let r = t.record().expect("a successful switch writes a record");
    assert_eq!(r["mode"], "default");
    assert_eq!(r["rev"], "abc123");
    assert_eq!(r["generation"], generation.to_str().unwrap());
    assert!(r["applied_at"].as_u64().unwrap() >= before);
    assert!(leftover_temp_files(&t).is_empty());
}

/// `args` で適用し、記録の `mode` が `explicit` になることを確かめる。
fn assert_recorded_as_explicit(name: &str, args: impl FnOnce(&T) -> Vec<String>) {
    let t = T::new();
    fake_generation(&t, "abc123");
    let args = args(&t);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = t.run(&argv, &[]);
    assert!(o.status.success(), "{name}: {}", err(&o));
    assert_eq!(t.record().expect(name)["mode"], "explicit", "{name}");
}

/// 引数で ref を渡した適用はすべて検証用の世代(`explicit`)。ローカルパスかどうかでは
/// 決めない — リモートのブランチ ref も main ではない。`--public-only` も同じ。
#[test]
fn every_explicit_ref_is_recorded_as_explicit() {
    assert_recorded_as_explicit("a local checkout", |t| {
        vec![t.checkout().to_str().unwrap().to_string()]
    });
    assert_recorded_as_explicit("a remote branch", |_| {
        vec!["github:tarotene/dotfiles/some-branch".to_string()]
    });
    assert_recorded_as_explicit("--public-only", |_| vec!["--public-only".to_string()]);
}

#[test]
fn a_failed_switch_does_not_write_or_touch_the_record() {
    let t = T::new();
    let o = t.run(&[], &[("HM_RC", "3")]);
    assert_eq!(o.status.code(), Some(3));
    assert!(t.record().is_none());

    let previous = r#"{"mode":"default","rev":"old"}"#;
    fs::create_dir_all(t.state()).unwrap();
    fs::write(t.state().join("last-apply.json"), previous).unwrap();
    let o = t.run(&[], &[("HM_RC", "3")]);
    assert_eq!(o.status.code(), Some(3));
    assert_eq!(
        fs::read_to_string(t.state().join("last-apply.json")).unwrap(),
        previous
    );
}

/// 照合(#1025)が食い違ったとき、profile は別の適用の世代を指している。その世代を
/// この hms の経路で作ったと記録しない(既存の記録にも触らない)。
#[test]
fn a_generation_that_fails_verification_is_not_recorded() {
    let t = T::new();
    let theirs = t.generation("theirs", "their-rev");
    let o = t.run(&[], &[("HM_LANDS", theirs.to_str().unwrap())]);
    assert_eq!(o.status.code(), Some(1), "{}", t.diag(&o));
    assert!(err(&o).contains("今回の世代が有効になっていません"));
    assert!(t.record().is_none());

    let previous = r#"{"mode":"default","rev":"old"}"#;
    fs::create_dir_all(t.state()).unwrap();
    fs::write(t.state().join("last-apply.json"), previous).unwrap();
    let o = t.run(&[], &[("HM_LANDS", theirs.to_str().unwrap())]);
    assert_eq!(o.status.code(), Some(1), "{}", t.diag(&o));
    assert_eq!(
        fs::read_to_string(t.state().join("last-apply.json")).unwrap(),
        previous
    );
}

/// 後処理(ここでは fcitx5 の検証)が失敗しても、switch は成功しているので世代は適用済み。
/// 記録が無いと、hms が作った世代を hms 以外が切り替えたと誤認する。
#[test]
fn a_failure_after_the_switch_still_records_the_generation() {
    let t = T::new();
    fake_generation(&t, "abc123");
    let o = t.run(&[], &[("ACTIVE_RC", "1")]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        t.record().expect("the switch had succeeded")["mode"],
        "default"
    );
}

#[test]
fn an_unwritable_record_only_warns() {
    let t = T::new();
    // rename の先が(空の)ディレクトリだと失敗する。
    fs::create_dir_all(t.state().join("last-apply.json")).unwrap();
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("Warning: 適用の記録"));
    assert!(leftover_temp_files(&t).is_empty());
}

/// ロックを取れない(状態ディレクトリを作れない)ときは、ロックなしで進まず止まる。
#[test]
fn an_unobtainable_lock_stops_the_apply() {
    let t = T::new();
    fs::create_dir_all(t.home().join(".local/state")).unwrap();
    fs::write(t.state(), "not a directory").unwrap();
    let o = t.run(&[], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("適用ロック"));
    assert!(t.hm_call().is_none());
}

#[test]
fn help_version_and_bad_options_take_no_lock_and_leave_no_state() {
    let t = T::new();
    for args in [&["--help"][..], &["--version"], &["--bogus"]] {
        t.run(args, &[]);
    }
    assert!(!t.state().exists());
}

/// 2 つ目の hms は、先の switch が終わるまで待つ(#1025)。待っている間に重ならず、
/// start/end が入れ子にならない。
#[test]
fn a_second_hms_waits_for_the_first_to_finish() {
    let t = T::new();
    let first_err = t.dir.path().join("first.err");
    let second_err = t.dir.path().join("second.err");

    let mut first = t.spawn(&[], &[("HM_HOLD", "1")], &first_err);
    wait_until("the first hms to reach its switch", || {
        t.stubs().join("started").exists()
    });

    let mut second = t.spawn(&[], &[], &second_err);
    wait_until("the second hms to announce that it is waiting", || {
        fs::read_to_string(&second_err)
            .unwrap_or_default()
            .contains("別の hms が適用中です")
    });
    // 待っている間、2 つ目は switch に入っていない。
    assert_eq!(t.order(), ["start"]);

    fs::write(t.stubs().join("release"), "").unwrap();
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    assert_eq!(t.order(), ["start", "end", "start", "end"]);
    assert!(!fs::read_to_string(&first_err)
        .unwrap()
        .contains("別の hms が適用中です"));
}

/// ロックは hms 自身だけが持つ。子(nix・home-manager)が hms より長生きしても、
/// 次の hms を待たせない(ロックの fd を子に継承させない)。
#[test]
fn a_child_that_outlives_hms_does_not_hold_the_lock() {
    let t = T::new();
    let o = t.run(&[], &[("HM_BG", "1")]);
    assert!(o.status.success(), "{}", err(&o));
    let o = t.run(&[], &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(!err(&o).contains("別の hms が適用中です"));
}
