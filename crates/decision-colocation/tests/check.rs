//! `scripts/decision-colocation-check --selftest` の全 16 ケース(bash 版の
//! 番号どおり)を、実際の一時 git repo に対する `run_check` と CLI の終了コードで
//! 検査する(移植の characterization、docs/rust-migration.md)。

mod common;

use common::Repo;
use decision_colocation::check::run_check;
use std::process::Command;

const SH: &str = "#!/usr/bin/env bash\ntrue\n";

fn check(r: &Repo) -> Vec<String> {
    run_check(&r.path(), &r.base_sha)
}

fn adr(title: &str, section: &[&str]) -> String {
    let mut s = format!("# {title}\n\n## 執行点\n");
    for l in section {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// 既存 ADR 末尾に Amendment を足した全文。
fn amended(heading: &str, body: &[&str]) -> String {
    let mut s = String::from("# ADR-0001 — base\n\n## Context\n\n");
    s.push_str(heading);
    s.push('\n');
    for l in body {
        s.push('\n');
        s.push_str(l);
    }
    s.push('\n');
    s
}

#[test]
fn cases_1_to_16() {
    let r = Repo::new();

    // 1: 既存ファイル編集のみは非該当で通過
    r.write(
        "docs/adr/0001-base.md",
        "# ADR-0001 — base\n\n## Context\n\n(typo fix)\n",
    );
    r.commit("typo");
    assert_eq!(check(&r), Vec::<String>::new(), "1");

    // 2: 新規ADR + 新規非mdファイルは合格
    r.reset_to_base();
    r.write(
        "docs/adr/0100-with-exec.md",
        &adr(
            "ADR-0100 — 執行点あり",
            &["- config/claude/hooks/new-gate.sh"],
        ),
    );
    r.write("config/claude/hooks/new-gate.sh", SH);
    r.commit("add adr with exec");
    assert_eq!(check(&r), Vec::<String>::new(), "2");

    // 3: 執行点節なしは不合格
    r.reset_to_base();
    r.write(
        "docs/adr/0101-no-section.md",
        "# ADR-0101 — 執行点なし\n\n## Context\n",
    );
    r.commit("add adr without section");
    let v = check(&r);
    assert_eq!(v.len(), 1, "3: {v:?}");
    assert!(v[0].contains("'## 執行点' 節がありません"), "3: {v:?}");

    // 4: 執行点が全てmd/docsは不合格
    r.reset_to_base();
    r.write(
        "docs/adr/0102-md-only.md",
        &adr("ADR-0102 — mdだけ", &["- docs/claude/foo.md"]),
    );
    r.write("docs/claude/foo.md", "placeholder\n");
    r.commit("add adr with md-only exec");
    assert!(!check(&r).is_empty(), "4");

    // 5: 執行点が実在しないパスは不合格
    r.reset_to_base();
    r.write(
        "docs/adr/0103-broken-ref.md",
        &adr("ADR-0103 — 存在しないパス", &["- scripts/does-not-exist"]),
    );
    r.commit("add adr with broken ref");
    let v = check(&r);
    assert_eq!(v.len(), 1, "5: {v:?}");
    assert!(
        v[0].contains("'scripts/does-not-exist' が実在しません"),
        "5: {v:?}"
    );

    // 6: 実在するが無変更のパスのみは不合格(D10)
    r.reset_to_base();
    r.write(
        "docs/adr/0104-reuse-only.md",
        &adr(
            "ADR-0104 — 既存機構の無変更併記",
            &["- config/claude/hooks/existing-gate.sh"],
        ),
    );
    r.commit("add adr reusing unchanged file");
    let v = check(&r);
    assert_eq!(v.len(), 1, "6: {v:?}");
    assert!(
        v[0].contains("既存機構の無変更併記だけでは合格しません"),
        "6: {v:?}"
    );

    // 7: 執行点が既存パスでも今回変更されていれば合格
    r.reset_to_base();
    r.write(
        "docs/adr/0105-modify-existing.md",
        &adr(
            "ADR-0105 — 変更された既存パスは合格",
            &["- config/claude/hooks/existing-gate.sh"],
        ),
    );
    r.write(
        "config/claude/hooks/existing-gate.sh",
        "#!/usr/bin/env bash\ntrue\necho changed\n",
    );
    r.commit("add adr + modify existing exec point");
    assert_eq!(check(&r), Vec::<String>::new(), "7");

    // 8: 非mdの同梱なしは不合格(config/claude/skills 新規)
    r.reset_to_base();
    r.write("config/claude/skills/new-skill.md", "skill body\n");
    r.commit("add new skill doc only");
    let v = check(&r);
    assert_eq!(v.len(), 1, "8: {v:?}");
    assert!(
        v[0].contains("docs/claude/ または config/claude/skills/"),
        "8: {v:?}"
    );

    // 9: 非mdの同梱ありは合格
    r.reset_to_base();
    r.write("config/claude/skills/new-skill.md", "skill body\n");
    r.write("config/claude/hooks/new-skill-gate.sh", SH);
    r.commit("add new skill doc + hook");
    assert_eq!(check(&r), Vec::<String>::new(), "9");

    // 10: docs/claude 新規のみは不合格
    r.reset_to_base();
    r.write("docs/claude/new-design.md", "design rationale\n");
    r.commit("add design doc only");
    assert!(!check(&r).is_empty(), "10");

    // 11: Amendment + 新規非mdファイルは合格
    r.reset_to_base();
    r.write(
        "docs/adr/0001-base.md",
        &amended(
            "## Amendment (2026-09-23 — 執行点あり)",
            &["### 執行点", "- config/claude/hooks/amendment-gate.sh"],
        ),
    );
    r.write("config/claude/hooks/amendment-gate.sh", SH);
    r.commit("amend with exec");
    assert_eq!(check(&r), Vec::<String>::new(), "11");

    // 12: Amendment に執行点なしは不合格
    r.reset_to_base();
    r.write(
        "docs/adr/0001-base.md",
        &amended(
            "## Amendment (2026-09-23 — 執行点なし)",
            &["新しい決定の説明のみ。"],
        ),
    );
    r.commit("amend without exec");
    let v = check(&r);
    assert_eq!(v.len(), 1, "12: {v:?}");
    assert!(v[0].contains("に '### 執行点' がありません"), "12: {v:?}");

    // 13: ADR-387 相当(既存機構の無変更宣言のみ)は不合格
    r.reset_to_base();
    r.write(
        "docs/adr/0001-base.md",
        &amended(
            "## Amendment (2026-09-23 — 既存機構の再利用のみ)",
            &["### 執行点", "- config/claude/hooks/existing-gate.sh"],
        ),
    );
    r.commit("amend reusing unchanged existing file");
    assert!(!check(&r).is_empty(), "13");

    // 14・15(#668): 1 行 2 パス(カンマ区切り)は不合格で、原因を名指しする
    r.reset_to_base();
    r.write(
        "docs/adr/0106-multi-path.md",
        &adr(
            "ADR-0106 — 1 行に 2 パス",
            &["- `config/claude/hooks/new-a.sh`, `config/claude/hooks/new-b.sh`"],
        ),
    );
    r.write("config/claude/hooks/new-a.sh", SH);
    r.write("config/claude/hooks/new-b.sh", SH);
    r.commit("add adr with two paths on one line");
    let v = check(&r);
    assert!(!v.is_empty(), "14");
    // 15: メッセージは「1 行 1 パス」で、「実在しない」と言わない
    let msg = v.join("\n");
    assert!(msg.contains("1 行 1 パス"), "15: {msg}");
    assert!(!msg.contains("実在しません"), "15: {msg}");
    assert!(
        msg.contains("  - `config/claude/hooks/new-a.sh`, `config/claude/hooks/new-b.sh`"),
        "15: {msg}"
    );

    // 16: --base に不正な ref は fail-open で通過
    assert_eq!(
        run_check(&r.path(), "not-a-real-ref"),
        Vec::<String>::new(),
        "16"
    );
}

/// bash 版の selftest には無いが、同じ分岐の追加検算。
#[test]
fn extra_branches() {
    let r = Repo::new();

    // Amendment が複数あれば全部検査し、違反を全部返す(最初で打ち切らない)。
    r.write(
        "docs/adr/0001-base.md",
        "# ADR-0001 — base\n\n## Context\n\n\
         ## Amendment (a)\n\nno section\n\n\
         ## Amendment (b)\n\nalso none\n",
    );
    r.commit("two bad amendments");
    assert_eq!(check(&r).len(), 2);

    // 既存の Amendment 見出しは、本文だけ直しても新規扱いしない。
    r.reset_to_base();
    r.write(
        "docs/adr/0001-base.md",
        "# ADR-0001 — base\n\n## Context\n\n## Amendment (old)\n\nx\n",
    );
    r.commit("land amendment");
    let landed = common::git(&r.path(), &["rev-parse", "HEAD"]);
    r.write(
        "docs/adr/0001-base.md",
        "# ADR-0001 — base\n\n## Context\n\n## Amendment (old)\n\nx (typo fix)\n",
    );
    r.commit("typo in amendment");
    assert_eq!(run_check(&r.path(), &landed), Vec::<String>::new());

    // `## 執行点` が空節のとき。
    r.reset_to_base();
    r.write("docs/adr/0107-empty.md", "# ADR\n\n## 執行点\n\n## 次\n");
    r.commit("empty section");
    let v = check(&r);
    assert!(v[0].contains("節がありません"), "{v:?}");

    // 節はあるが箇条書きが無い。
    r.reset_to_base();
    r.write("docs/adr/0108-prose.md", "# ADR\n\n## 執行点\n\nなし\n");
    r.commit("prose section");
    let v = check(&r);
    assert!(v[0].contains("にパスが列挙されていません"), "{v:?}");
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_decision-colocation-check"))
}

#[test]
fn cli_exit_codes_and_output() {
    let r = Repo::new();
    r.write("docs/adr/0101-no-section.md", "# ADR\n\n## Context\n");
    r.commit("bad");

    let out = bin()
        .current_dir(r.path())
        .args(["--base", &r.base_sha])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert!(String::from_utf8_lossy(&out.stderr)
        .starts_with("decision-colocation-check: docs/adr/0101-no-section.md は新規 ADR です"));

    r.reset_to_base();
    r.write(
        "docs/adr/0001-base.md",
        "# ADR-0001 — base\n\n## Context\n\nfix\n",
    );
    r.commit("ok");
    let out = bin()
        .current_dir(r.path())
        .args(["--base", &r.base_sha])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "decision-colocation-check: OK\n"
    );

    // サブディレクトリから呼んでも repo 先頭を基準にする。
    let out = bin()
        .current_dir(r.path().join("docs"))
        .args(["--base", &r.base_sha])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn cli_usage_errors_exit_2() {
    let r = Repo::new();
    for args in [&[][..], &["--base"], &["--base", ""], &["--bogus"], &["x"]] {
        let out = bin().current_dir(r.path()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).starts_with("usage: decision-colocation-check")
        );
    }
    let out = bin().arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("usage:"));
}
