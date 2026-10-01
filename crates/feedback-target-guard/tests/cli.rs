//! bash 版 `feedback-target-guard.sh --selftest` 全 16 ケース(`--body-file`
//! の 4 ケース #675 を含む)を実バイナリに対して固定する(#415)。
//! HOME は使い捨てのフィクスチャに向ける。全ケースは bash 版で緑を確認済み。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_feedback-target-guard");

struct Fx {
    _d: tempfile::TempDir,
    root: PathBuf,
}

fn fx() -> Fx {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().to_path_buf();
    let h = root.join("home");
    std::fs::create_dir_all(h.join(".claude/skills/precedent-grounding")).unwrap();
    std::fs::write(h.join(".claude/skills/precedent-grounding/SKILL.md"), "").unwrap();
    std::fs::create_dir_all(h.join(".claude/hooks")).unwrap();
    std::fs::write(h.join(".claude/hooks/foo-gate.sh"), "").unwrap();
    std::fs::create_dir_all(h.join(".agents")).unwrap();
    std::fs::write(h.join(".agents/AGENTS.md"), "# 見出しテスト\n\nある節\n").unwrap();
    Fx { _d: d, root }
}

/// hook として呼ぶ。deny の理由文(通れば None)。
fn run(home: Option<&Path>, cmd: &str) -> Option<String> {
    let input = serde_json::json!({"tool_name":"Bash","tool_input":{"command":cmd}}).to_string();
    let mut c = Command::new(BIN);
    c.stdin(Stdio::piped()).stdout(Stdio::piped());
    match home {
        Some(h) => c.env("HOME", h),
        None => c.env_remove("HOME"),
    };
    let mut ch = c.spawn().unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let o = ch.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    if o.stdout.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    Some(
        v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

fn pass(f: &Fx, name: &str, cmd: &str) {
    let out = run(Some(&f.root.join("home")), cmd);
    assert!(out.is_none(), "{name}: expected pass, denied: {out:?}");
}

fn deny(f: &Fx, name: &str, cmd: &str, needle: &str) {
    let out = run(Some(&f.root.join("home")), cmd)
        .unwrap_or_else(|| panic!("{name}: expected deny, but passed"));
    assert!(out.contains(needle), "{name}: [{needle}] not in {out}");
}

#[test]
fn st01_to_04_label_and_skill() {
    let f = fx();
    pass(
        &f,
        "1 feedback ラベルなしは対象外",
        "gh issue create --label bug --body 'Target: skill/precedent-grounding'",
    );
    deny(
        &f,
        "2 feedback + Target なし",
        "gh issue create --label feedback --body '本文だけ'",
        "Target: 行がありません",
    );
    pass(
        &f,
        "3 実在する skill/",
        "gh issue create --label feedback --body 'Target: skill/precedent-grounding'",
    );
    deny(
        &f,
        "4 実在しない skill/",
        "gh issue create --label feedback --body 'Target: skill/nonexistent'",
        "実在しません",
    );
}

#[test]
fn st05_to_08_hook_and_agents_md() {
    let f = fx();
    pass(
        &f,
        "5 実在する hook/",
        "gh issue create --label feedback --body 'Target: hook/foo-gate'",
    );
    pass(
        &f,
        "6 実在する agents-md/",
        "gh issue create --label feedback --body 'Target: agents-md/見出しテスト'",
    );
    deny(
        &f,
        "7 実在しない agents-md/",
        "gh issue create --label feedback --body 'Target: agents-md/存在しない節'",
        "実在しません",
    );
    pass(
        &f,
        "8 カンマ区切りラベルの中に feedback",
        "gh issue create --label bug,feedback --body 'Target: hook/foo-gate'",
    );
}

#[test]
fn st09_to_12_non_targets_and_undecidable() {
    let f = fx();
    pass(
        &f,
        "9 gh issue edit は対象外",
        "gh issue edit 1 --label feedback --body 'Target: skill/precedent-grounding'",
    );
    pass(
        &f,
        "10 コマンド位置外の綴り",
        "echo 'gh issue create --label feedback --body x'",
    );
    pass(
        &f,
        "11 本文が $() は判定不能で通す",
        "gh issue create --label feedback --body \"$(echo x)\"",
    );
    pass(
        &f,
        "12 heredoc 本体に Target: がある",
        "gh issue create --label feedback --body \"$(cat <<'EOF'\nTarget: skill/precedent-grounding\nEOF\n)\"",
    );
}

#[test]
fn st13_to_16_body_file() {
    let f = fx();
    let p = |n: &str, s: &str| {
        let path = f.root.join(n);
        std::fs::write(&path, s).unwrap();
        path.display().to_string()
    };
    let with = p(
        "with-target.md",
        "Target: skill/precedent-grounding\n\n本文\n",
    );
    let none = p("no-target.md", "本文だけ\n");
    let bad = p("bad-target.md", "Target: skill/nonexistent\n");
    let base = "gh issue create -R tarotene/dotfiles --label feedback";
    pass(
        &f,
        "13 --body-file に Target: がある",
        &format!("{base} --body-file {with}"),
    );
    deny(
        &f,
        "14 --body-file に Target: が無い",
        &format!("{base} --body-file {none}"),
        "Target: 行がありません",
    );
    deny(
        &f,
        "15 --body-file の Target: が実在しない",
        &format!("{base} --body-file {bad}"),
        "実在しません",
    );
    pass(
        &f,
        "16 --body-file=<path> 形式",
        &format!("{base} --body-file={with}"),
    );
    // -F も同じ(本文フラグの別名)
    pass(&f, "-F", &format!("{base} -F {with}"));
}

#[test]
fn deny_reason_text_is_verbatim() {
    let f = fx();
    let out = run(
        Some(&f.root.join("home")),
        "gh issue create --label feedback --body x",
    )
    .unwrap();
    assert_eq!(
        out,
        "feedback ラベル付き Issue の起票に問題があります: Target: 行がありません。

本文に次のいずれかの形式で1行追加してください(実在照合されます):

  Target: skill/<name>          — config/claude/skills/<name>/SKILL.md
  Target: agents-md/<節見出し>   — ~/.agents/AGENTS.md・~/.claude/CLAUDE.md の実在する見出し
  Target: hook/<name>            — config/claude/hooks/<name>* の実在するファイル

(共有 AGENTS.md「ユーザーからのフィードバックは不可視なローカルメモに
閉じ込めない」、ADR-543「既存手段の前倒し接地と、決定論への昇格導線」参照)"
    );
}

#[test]
fn target_forms_and_claude_md_fallback() {
    let f = fx();
    let h = f.root.join("home");
    std::fs::write(h.join(".claude/CLAUDE.md"), "## CLAUDE 側の節\n").unwrap();
    pass(
        &f,
        "CLAUDE.md の見出しも照合",
        "gh issue create --label feedback --body 'Target: agents-md/CLAUDE 側の節'",
    );
    // 種別なし・空の名前・未知の種別は実在しない扱い
    for t in ["skill", "skill/", "other/x", "/x"] {
        deny(
            &f,
            t,
            &format!("gh issue create --label feedback --body 'Target: {t}'"),
            "実在しません",
        );
    }
    // hook は接頭辞一致(`${name}*`)。サブディレクトリ付きも辿る
    std::fs::create_dir_all(h.join(".claude/hooks/sub")).unwrap();
    std::fs::write(h.join(".claude/hooks/sub/bar.rs"), "").unwrap();
    pass(
        &f,
        "hook 接頭辞",
        "gh issue create --label feedback --body 'Target: hook/foo'",
    );
    pass(
        &f,
        "hook サブディレクトリ",
        "gh issue create --label feedback --body 'Target: hook/sub/ba'",
    );
    deny(
        &f,
        "hook 不在",
        "gh issue create --label feedback --body 'Target: hook/nope'",
        "実在しません",
    );
    // 最初の Target: 行だけを見る
    deny(
        &f,
        "最初の Target:",
        "gh issue create --label feedback --body 'Target: skill/nonexistent\nTarget: skill/precedent-grounding'",
        "実在しません",
    );
}

#[test]
fn degraded_inputs_and_missing_home() {
    for stdin in [
        "",
        "{x",
        "{}",
        r#"{"tool_name":"Read"}"#,
        r#"{"tool_name":"Bash","tool_input":{}}"#,
    ] {
        let mut ch = Command::new(BIN)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        ch.stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let o = ch.wait_with_output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{stdin}");
        assert!(o.stdout.is_empty(), "{stdin}");
    }
    // HOME が無ければ実在照合は常に偽(Target: 自体が無ければ従来どおり)
    assert!(run(
        None,
        "gh issue create --label feedback --body 'Target: skill/x'"
    )
    .unwrap()
    .contains("実在しません"));
}
