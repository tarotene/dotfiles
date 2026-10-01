//! bash 版と Rust 版を同じ fixture・同じ gh スタブで動かし、stdout・終了
//! コード・ledger(generated_at 以外)がバイト単位で一致することを確かめる
//! (#414: `github-audit --json` / ledger を読む側がそのまま読めるように)。
//! bash 版 `scripts/github-audit` は依存側の移植が終わるまでリポジトリに残る。

mod common;

use common::Fx;
use std::fs;

fn ledger_without_timestamp(fx: &Fx) -> String {
    fs::read_to_string(fx.p("state/ledger.json"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with("\"generated_at\""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_same(fx: &Fx, args: &[&str], extra: &[(&str, &str)]) {
    let b = fx.bash(args, extra);
    let b_ledger = ledger_without_timestamp(fx);
    let r = fx.rust(args, extra);
    let r_ledger = ledger_without_timestamp(fx);
    assert_eq!(r.stdout, b.stdout, "stdout が違う: args={args:?}");
    assert_eq!(r.code, b.code, "終了コードが違う: args={args:?}");
    assert_eq!(r_ledger, b_ledger, "ledger が違う: args={args:?}");
    assert_eq!(r.stderr, b.stderr, "stderr が違う: args={args:?}");
}

#[test]
fn main_fixture_json_and_report() {
    let fx = Fx::new();
    assert_same(&fx, &["--json"], &[]);
    assert_same(&fx, &[], &[]);
    assert_same(&fx, &["--json", "rulesets"], &[]);
    assert_same(&fx, &["naming", "settings", "--json"], &[]);
    fx.write("config/overrides.tsv", "");
    assert_same(&fx, &["--json"], &[]);
}

#[test]
fn later_stages() {
    let fx = Fx::new();
    fx.overlay("snapshot");
    let snap = fx.p("state/app-snapshot.json");
    let snap = snap.to_str().unwrap();
    assert_same(
        &fx,
        &["--json", "releaser"],
        &[("GITHUB_AUDIT_APP_SNAPSHOT_FILE", snap)],
    );
    assert_same(&fx, &[], &[("GITHUB_AUDIT_APP_SNAPSHOT_FILE", snap)]);
    fs::remove_file(fx.p("state/app-snapshot.json")).unwrap();
    fx.overlay("releaser");
    assert_same(&fx, &["--json"], &[]);
    assert_same(&fx, &[], &[]);
    fx.overlay("archived");
    assert_same(&fx, &["--json"], &[]);
    assert_same(&fx, &[], &[]);
}

#[test]
fn usage_and_errors() {
    let fx = Fx::new();
    for args in [&["--help"][..], &["-x"], &["bogus"]] {
        let b = fx.bash(args, &[]);
        let r = fx.rust(args, &[]);
        assert_eq!(
            (r.code, &r.stdout, &r.stderr),
            (b.code, &b.stdout, &b.stderr),
            "{args:?}"
        );
    }
}

/// bash 版 selftest に無い、境界寄りの入力(CJK・nav-doc マーカーの崩れ・
/// ci.yml の書式揺れ・quality.json のキー順・JSON5・ruleset の重複/除外/
/// tag・REST の取得失敗や型違い・snapshot など)で両実装を突き合わせる。
#[test]
fn tricky_inputs() {
    let fx = Fx::new();
    fx.overlay("tricky");
    fx.write(
        "config/overrides.tsv",
        "# comment\n  \t\npj-x\tsettings\texempt\nweird\t*\texempt\r\nknown.site\tdocs\tskip\n",
    );
    fx.write("config/codename-registry.local.tsv", "regcode\nother\n");
    let now = ("GITHUB_AUDIT_LIFECYCLE_NOW", "2026-09-23T00:00:00Z");
    assert_same(&fx, &["--json"], &[now]);
    assert_same(&fx, &[], &[now]);
    assert_same(&fx, &["--json", "docs", "docs", "naming"], &[now]);
}
