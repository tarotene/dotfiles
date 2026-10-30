//! `docs/agents/codify-signals.md` の生成部が、語彙(`vocabulary.json`)から作る事例集と一致することを検査する。
//! 語彙を直したのに表を貼り替え忘れると、読む人が古い表を見る。文書は `include_str!` せず、テスト時に読む
//! (nix のビルドの入力に docs を足さないため)。

use issue_inbox::codify::Vocabulary;
use std::path::PathBuf;

const BEGIN: &str = "<!-- BEGIN GENERATED: session-retrospective --retro-casebook -->\n";
const END: &str = "<!-- END GENERATED -->";

#[test]
fn the_generated_section_matches_the_vocabulary() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/agents/codify-signals.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} が読めない: {e}", path.display()));
    let start = doc.find(BEGIN).expect("生成部の開始の印が無い") + BEGIN.len();
    let end = start + doc[start..].find(END).expect("生成部の終了の印が無い");
    assert_eq!(
        &doc[start..end],
        Vocabulary::get().casebook_markdown(),
        "docs/agents/codify-signals.md の生成部が語彙とずれている。`cargo run -q -p session-retrospective -- --retro-casebook` の出力に貼り替える"
    );
}
