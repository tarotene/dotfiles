//! 投稿の関所の端から端まで(設定ファイル・記録・環境変数を含む)。環境変数を書き換えるので、
//! 1 つのテスト関数の中で順に確かめる(統合テストは別プロセスなので、ほかのテストには影響しない)。

use agent_session::{Agent, Outcome};
use ja_polish::{blocks, gate, records};
use serde_json::json;

const JP: &str = "翻訳調の文章を直すことができる。";

fn raw_commit() -> String {
    json!({"tool_name": "Bash", "tool_input": {"command": format!("git commit -m '{JP}'")}})
        .to_string()
}

fn is_deny(o: Option<Outcome>) -> bool {
    matches!(o, Some(Outcome::Deny(_)))
}

#[test]
fn gate_follows_config_records_and_escape_hatch() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
    std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
    std::env::remove_var("SKIP_JA_POLISH_GATE");

    // 1. 設定ファイルが無い機体(ja-polish を有効にしていない)では、何も検査しない。
    assert!(gate::hook(Agent::Claude, &raw_commit()).is_none());
    assert!(gate::hook(Agent::Claude, "not json").is_none());

    // 2. 設定ファイルがあり、記録が無い: 拒否。
    let cfg_dir = dir.path().join("config/dotfiles");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("ja-polish.toml"), "model = \"m\"\n").unwrap();
    assert!(is_deny(gate::hook(Agent::Claude, &raw_commit())));
    // Codex も同じ入力の形。
    assert!(is_deny(gate::hook(Agent::Codex, &raw_commit())));

    // 3. 逃げ道の環境変数。
    std::env::set_var("SKIP_JA_POLISH_GATE", "1");
    assert!(gate::hook(Agent::Claude, &raw_commit()).is_none());
    std::env::remove_var("SKIP_JA_POLISH_GATE");
    assert!(is_deny(gate::hook(Agent::Claude, &raw_commit())));

    // 4. 記録に段落があれば通る(書き直した段落も、素通しにした段落も同じ)。
    let path = records::records_path().unwrap();
    records::append(
        &path,
        &[records::Entry::skipped(blocks::hash(JP), "m", "テスト")],
    )
    .unwrap();
    assert!(gate::hook(Agent::Claude, &raw_commit()).is_none());

    // 5. 対象でないツールと、Bash の対象でないコマンドは見ない。
    let read = json!({"tool_name": "Read", "tool_input": {"file_path": "/x"}}).to_string();
    assert!(gate::hook(Agent::Claude, &read).is_none());
    let ls = json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}).to_string();
    assert!(gate::hook(Agent::Claude, &ls).is_none());

    // 6. MCP(Gmail の下書き)。
    let other = "別の日本語の文章を、これから下書きにする。";
    let mcp = json!({
        "tool_name": "mcp__claude_ai_Gmail__create_draft",
        "tool_input": {"to": ["a@example.com"], "body": other}
    })
    .to_string();
    assert!(is_deny(gate::hook(Agent::Claude, &mcp)));

    // 7. ja-polish が動けない間(直近 5 件の記録がすべて素通し)は、通していない段落でも拒否しない。
    //    往復を求めても、素通しが増えるだけだからである(API キーが無い機体の症状)。
    let entries: Vec<records::Entry> = (0..records::UNAVAILABLE_RUN)
        .map(|i| records::Entry::skipped(format!("unavailable{i}"), "m", "No key found"))
        .collect();
    records::append(&path, &entries).unwrap();
    assert!(gate::hook(Agent::Claude, &mcp).is_none());
    //    書き直しが 1 件入れば、並びが切れて、関所は元に戻る(直った)。
    records::append(
        &path,
        &[records::Entry::polished(
            blocks::hash("直った後の段落です。"),
            "m",
        )],
    )
    .unwrap();
    assert!(is_deny(gate::hook(Agent::Claude, &mcp)));
}
