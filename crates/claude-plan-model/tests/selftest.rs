//! `scripts/claude-plan-model --selftest` の全ケース(41 アサーション)を、
//! 実バイナリと一時 settings.json で再現する統合テスト(#414、
//! docs/rust-migration.md の段 1-2)。
//!
//! テスト対象は既定で cargo bin(Rust 版)。`CLAUDE_PLAN_MODEL_ORACLE` に bash 版の
//! パスを入れると同じケースを bash 版に向けて走らせる(移植前の緑確認用)。
//! カタログは `CLAUDE_PLAN_MODEL_CATALOG` で差し替える(bash 版の継ぎ目と同じ)ので、
//! どの claude が入っているかに依存しない。ラベルは bash selftest の `1a` 等と同じ。
//!
//! 末尾の `catalog_*` は bash 版にあった継ぎ目の外側(実バイナリからのカタログ読み)
//! を、PATH 上の偽 claude で固定する追加ケース。

use std::path::Path;
use std::process::{Command, Stdio};

const CATALOG: &str = r#"latest_per_family:{fable:"claude-fable-5-1",opus:"claude-opus-5",sonnet:"claude-sonnet-5",haiku:"claude-haiku-4-5"}"#;

const M_FABLE_SONNET: &str = r#"{"fb":["claude-opus-5"],"p":"claude-fable-5-1","x":null}"#;
const M_OPUS_SONNET: &str = r#"{"fb":null,"p":"claude-opus-5","x":null}"#;
const M_FABLE_OPUS: &str =
    r#"{"fb":["claude-sonnet-5"],"p":"claude-fable-5-1","x":"claude-opus-5"}"#;

fn target() -> Command {
    match std::env::var_os("CLAUDE_PLAN_MODEL_ORACLE") {
        Some(script) => {
            let mut c = Command::new("bash");
            c.arg(script);
            c
        }
        None => Command::new(env!("CARGO_BIN_EXE_claude-plan-model")),
    }
}

struct Run {
    rc: i32,
    /// stdout + stderr(bash の `2>&1` と同じ扱い)
    out: String,
}

fn run(file: &Path, args: &[&str]) -> Run {
    let o = target()
        .args(args)
        .env("CLAUDE_PLAN_MODEL_SETTINGS", file)
        .env("CLAUDE_PLAN_MODEL_CATALOG", CATALOG)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Run {
        rc: o.status.code().unwrap_or(-1),
        out: format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    }
}

/// `jq -cS '{p, x, fb}'` 相当(キーは辞書順、欠けは null)。
fn state(file: &Path) -> String {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let pick = |p: &str| v.pointer(p).cloned().unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&serde_json::json!({
        "fb": pick("/fallbackModel"),
        "p": pick("/env/ANTHROPIC_DEFAULT_OPUS_MODEL"),
        "x": pick("/env/ANTHROPIC_DEFAULT_SONNET_MODEL"),
    }))
    .unwrap()
}

fn seed(file: &Path, json: &str) {
    std::fs::write(file, format!("{json}\n")).unwrap();
}

fn cat(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap()
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("settings.json");
    (dir, f)
}

// 1) 未初期化は既定モードを seed し、持っていない前のモードを主張しない。
#[test]
fn t1_uninit_seeds_default() {
    let (_d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    let r = run(&f, &[""]);
    assert_eq!(state(&f), M_FABLE_SONNET, "1a uninit seeds fable/sonnet");
    assert_eq!(r.rc, 0, "1b uninit exit 0");
    assert!(
        r.out.contains("[uninitialised] => [fable/sonnet]"),
        "1c uninit is not reported as opus: {}",
        r.out
    );
}

// 2) 引数なしのトグルは 3 モードを巡って元に戻る。
#[test]
fn t2_cycle() {
    let (_d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    run(&f, &[""]);
    run(&f, &[""]);
    assert_eq!(state(&f), M_OPUS_SONNET, "2a cycle 1 -> opus/sonnet");
    run(&f, &[""]);
    assert_eq!(state(&f), M_FABLE_OPUS, "2b cycle 2 -> fable/opus");
    run(&f, &[""]);
    assert_eq!(state(&f), M_FABLE_SONNET, "2c cycle 3 -> fable/sonnet");
}

// 3) fable/opus を抜けると実行側の上書きは残らず消える。
#[test]
fn t3_explicit_modes_and_sonnet_key_removal() {
    let (_d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    run(&f, &["fable/opus"]);
    assert_eq!(state(&f), M_FABLE_OPUS, "3a explicit fable/opus");
    run(&f, &["opus/sonnet"]);
    assert_eq!(state(&f), M_OPUS_SONNET, "3b SONNET key deleted on exit");
    let v: serde_json::Value = serde_json::from_str(&cat(&f)).unwrap();
    assert!(
        v["env"].get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none(),
        "3c SONNET key really absent"
    );
    run(&f, &["fable-opus"]);
    assert_eq!(state(&f), M_FABLE_OPUS, "3d dash spelling accepted");
    run(&f, &["fable"]);
    assert_eq!(state(&f), M_FABLE_SONNET, "3e legacy name \"fable\"");
    run(&f, &["opus"]);
    assert_eq!(state(&f), M_OPUS_SONNET, "3f legacy name \"opus\"");
}

// 4) 定常状態はファイルに全く触れない(mtime すら)。
#[test]
fn t4_steady_state_untouched() {
    let (_d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    run(&f, &["opus/sonnet"]);
    let stat = |f: &Path| {
        let m = std::fs::metadata(f).unwrap();
        (m.len(), m.modified().unwrap())
    };
    let before = stat(&f);
    // mtime の粒度より後に走らせるため少し待つ
    std::thread::sleep(std::time::Duration::from_millis(20));
    let r = run(&f, &["opus/sonnet"]);
    assert_eq!(before, stat(&f), "4a steady state leaves mtime alone");
    assert_eq!(r.rc, 0, "4b steady state exit 0");
    assert!(
        r.out.contains("mode unchanged: [opus/sonnet]"),
        "4c steady state says unchanged: {}",
        r.out
    );
}

// 5) .model の素のエイリアスは inert ではなく over-broad: 書く前に拒否する。
#[test]
fn t5_alias_model_refused() {
    let (_d, f) = fixture();
    seed(
        &f,
        r#"{"model":"opus[1m]","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-opus-5"}}"#,
    );
    let before = cat(&f);
    let r = run(&f, &[""]);
    assert_eq!(r.rc, 1, "5a alias .model exits 1");
    assert_eq!(before, cat(&f), "5b alias .model leaves the file untouched");
    assert!(
        r.out.contains("WHOLE session"),
        "5c alias .model is diagnosed, not called inert: {}",
        r.out
    );
    let r = run(&f, &["--force", ""]);
    assert_eq!(state(&f), M_FABLE_OPUS, "5d --force applies anyway");
    assert_eq!(r.rc, 0, "5e --force exit 0");
}

// 6) 具体モデル ID、または .model 無しは本当に inert: これも書く前に拒否する。
#[test]
fn t6_flat_model_refused() {
    let (_d, f) = fixture();
    seed(
        &f,
        r#"{"model":"claude-opus-5[1m]","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-fable-5-1"}}"#,
    );
    let before = cat(&f);
    let r = run(&f, &[""]);
    assert_eq!(r.rc, 1, "6a flat .model exits 1");
    assert_eq!(before, cat(&f), "6b flat .model leaves the file untouched");
    seed(
        &f,
        r#"{"env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-fable-5-1"}}"#,
    );
    let before = cat(&f);
    let r = run(&f, &[""]);
    assert_eq!(r.rc, 1, "6c absent .model exits 1");
    assert_eq!(
        before,
        cat(&f),
        "6d absent .model leaves the file untouched"
    );
}

// 7) このコマンドが書いていない値は、どちら側でも決して奪わない。
#[test]
fn t7_foreign_values_never_stolen() {
    let (_d, f) = fixture();
    seed(
        &f,
        r#"{"model":"opusplan","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-mythos-preview"}}"#,
    );
    let before = cat(&f);
    let r = run(&f, &[""]);
    assert_eq!(r.rc, 1, "7a unknown plan value exits 1");
    assert_eq!(before, cat(&f), "7b unknown plan value untouched");
    seed(
        &f,
        r#"{"model":"opusplan","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-fable-5-1","ANTHROPIC_DEFAULT_SONNET_MODEL":"claude-sonnet-5"}}"#,
    );
    let before = cat(&f);
    let r = run(&f, &[""]);
    assert_eq!(r.rc, 1, "7c foreign sonnet pin exits 1");
    assert_eq!(before, cat(&f), "7d foreign sonnet pin untouched");
    let r = run(&f, &["sync"]);
    assert_eq!(before, cat(&f), "7e sync leaves it untouched too");
    assert_eq!(r.rc, 0, "7f sync still exits 0");
}

// 8) sync はモードを保ち、腐った ID を引き直す。.model が何であっても。
#[test]
fn t8_sync_preserves_mode() {
    let (_d, f) = fixture();
    seed(
        &f,
        r#"{"model":"claude-opus-5[1m]","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-fable-5"}}"#,
    );
    let r = run(&f, &["sync"]);
    assert_eq!(
        state(&f),
        M_FABLE_SONNET,
        "8a sync refreshes a rotten fable pin"
    );
    assert_eq!(r.rc, 0, "8b sync ignores .model");
    seed(
        &f,
        r#"{"model":"opusplan","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-fable-5","ANTHROPIC_DEFAULT_SONNET_MODEL":"claude-opus-4-1"}}"#,
    );
    run(&f, &["sync"]);
    assert_eq!(
        state(&f),
        M_FABLE_OPUS,
        "8c sync preserves fable/opus and refreshes both"
    );
    seed(
        &f,
        r#"{"model":"opusplan","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-opus-4-1"}}"#,
    );
    run(&f, &["sync"]);
    assert_eq!(state(&f), M_OPUS_SONNET, "8d sync preserves opus/sonnet");
    seed(&f, r#"{"model":"opusplan"}"#);
    run(&f, &["sync"]);
    assert_eq!(state(&f), M_FABLE_SONNET, "8e sync seeds the default mode");
}

// 9) 無関係なキーは残り、status は決して書き換えず失敗もしない。
#[test]
fn t9_unrelated_keys_and_status() {
    let (_d, f) = fixture();
    seed(
        &f,
        r#"{"model":"opusplan","permissions":{"allow":["Bash(ls)"]},"env":{"FOO":"bar"}}"#,
    );
    run(&f, &["fable/opus"]);
    let got: serde_json::Value = serde_json::from_str(&cat(&f)).unwrap();
    let want: serde_json::Value = serde_json::from_str(
        r#"{"env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-fable-5-1","ANTHROPIC_DEFAULT_SONNET_MODEL":"claude-opus-5","FOO":"bar"},"fallbackModel":["claude-sonnet-5"],"model":"opusplan","permissions":{"allow":["Bash(ls)"]}}"#,
    )
    .unwrap();
    assert_eq!(want, got, "9a unrelated keys preserved");
    let before = cat(&f);
    let r = run(&f, &["status"]);
    assert_eq!(r.rc, 0, "9b status exits 0");
    assert_eq!(before, cat(&f), "9c status does not mutate");
    seed(&f, r#"{"model":"claude-opus-5[1m]"}"#);
    let before = cat(&f);
    let r = run(&f, &["status"]);
    assert_eq!(r.rc, 0, "9d status exits 0 even when inert");
    assert_eq!(before, cat(&f), "9e status does not mutate when inert");
}

// 10) 途中のどこにも一時ファイルが残らない(上の全ケースを 1 つの dir で通す)。
#[test]
fn t10_no_leftover_temp_files() {
    let (d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    for a in [
        &[""][..],
        &[""],
        &[""],
        &["opus/sonnet"],
        &["sync"],
        &["fable/opus"],
    ] {
        run(&f, a);
    }
    let left: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("settings.json.hm."))
        .collect();
    assert!(left.is_empty(), "10a no leftover temp files: {left:?}");
}

// --- 追加: bash 版の継ぎ目の外側 ------------------------------------------

#[test]
fn key_order_of_settings_is_preserved() {
    // jq は入力のキー順を保つ。辞書順に並べ直さない(利用者の settings 全体を壊さない)。
    let (_d, f) = fixture();
    seed(
        &f,
        r#"{"zeta":1,"model":"opusplan","env":{"Z":"1","A":"2"},"alpha":[1,2]}"#,
    );
    run(&f, &["fable/sonnet"]);
    let text = cat(&f);
    let pos = |k: &str| text.find(k).unwrap();
    assert!(pos("\"zeta\"") < pos("\"model\""));
    assert!(pos("\"model\"") < pos("\"env\""));
    assert!(pos("\"env\"") < pos("\"alpha\""));
    assert!(pos("\"alpha\"") < pos("\"fallbackModel\""));
    assert!(pos("\"Z\"") < pos("\"A\"") && pos("\"A\"") < pos("ANTHROPIC_DEFAULT_OPUS_MODEL"));
}

#[test]
fn catalog_is_read_from_installed_claude() {
    use std::os::unix::fs::PermissionsExt;
    let (d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    let bindir = d.path().join("bin");
    std::fs::create_dir_all(&bindir).unwrap();
    let fake = bindir.join("claude");
    // 空カタログが先に出現しても、次の非空カタログを拾う
    std::fs::write(
        &fake,
        format!("#!/bin/sh\n# latest_per_family:{{}}\n# {CATALOG}\nexit 0\n"),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = target()
        .arg("sync")
        .env("CLAUDE_PLAN_MODEL_SETTINGS", &f)
        .env_remove("CLAUDE_PLAN_MODEL_CATALOG")
        .env("PATH", &bindir)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(state(&f), M_FABLE_SONNET);
}

#[test]
fn missing_catalog_degrades_without_touching_settings() {
    let (d, f) = fixture();
    seed(&f, r#"{"model":"opusplan"}"#);
    let before = cat(&f);
    let empty = d.path().join("nobin");
    std::fs::create_dir_all(&empty).unwrap();
    let go = |arg: &str| {
        target()
            .arg(arg)
            .env("CLAUDE_PLAN_MODEL_SETTINGS", &f)
            .env_remove("CLAUDE_PLAN_MODEL_CATALOG")
            .env("PATH", &empty)
            .output()
            .unwrap()
    };
    // sync は activation を壊さない(exit 0)、人間の操作は 1
    assert_eq!(go("sync").status.code(), Some(0));
    assert_eq!(go("fable").status.code(), Some(1));
    assert_eq!(before, cat(&f));
}

#[test]
fn argument_errors() {
    let (_d, f) = fixture();
    let r = run(&f, &["--bogus"]);
    assert_eq!(r.rc, 1);
    assert!(r.out.contains("unknown option: --bogus"));
    let r = run(&f, &["nope"]);
    assert_eq!(r.rc, 1);
    assert!(r.out.contains("unknown argument: nope"));
    let r = run(&f, &["status", "sync"]);
    assert_eq!(r.rc, 1);
    assert!(r.out.contains("unexpected argument: sync"));
    let r = run(&f, &["--help"]);
    assert_eq!(r.rc, 0);
    assert!(r.out.starts_with("Usage: claude-plan-model"));
}
