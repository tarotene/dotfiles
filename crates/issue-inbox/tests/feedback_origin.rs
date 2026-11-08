//! feedback メモリを「今のセッションが更新した」と数えるときの帰属(`originSessionId`)。
//!
//! 更新時刻だけで帰属させると、同じ時間帯に走る別セッションが書いたメモリも拾う。メモリの
//! frontmatter に `originSessionId` があれば、今のセッションと違うものは除く。欄が無い古いメモリは
//! 従来どおり更新時刻だけで帰属させる。
//!
//! 環境変数(stamp と memory のルート)を差すので、プロセス全体に効く。このファイルには
//! テストを 1 つだけ置く。

use std::fs::{self, File};
use std::time::{Duration, SystemTime};

const MINE: &str = "11111111-1111-1111-1111-111111111111";
const OTHER: &str = "22222222-2222-2222-2222-222222222222";

fn memory(dir: &std::path::Path, name: &str, origin: Option<&str>, body: &str) -> String {
    let origin_line = origin.map_or(String::new(), |o| format!("  originSessionId: {o}\n"));
    let path = dir.join(format!("proj/memory/{name}.md"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!("---\nname: {name}\nmetadata:\n  type: feedback\n{origin_line}---\n{body}\n"),
    )
    .unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn memories_written_by_another_session_are_not_attributed_to_this_one() {
    let tmp = tempfile::tempdir().unwrap();
    let stamps = tmp.path().join("stamps");
    let projects = tmp.path().join("projects");
    fs::create_dir_all(&stamps).unwrap();
    std::env::set_var("WRAPUP_FEEDBACK_STAMP_DIR", &stamps);
    std::env::set_var("WRAPUP_FEEDBACK_MEMORY_DIR", &projects);

    // セッション開始の stamp を 1 時間前にして、あとに書いたメモリを「更新された」側にする。
    for id in [MINE, OTHER] {
        let f = File::create(issue_inbox::feedback_stamp_file(id)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }

    let mine = memory(&projects, "mine", Some(MINE), "no ref");
    let other = memory(&projects, "other", Some(OTHER), "no ref");
    let legacy = memory(&projects, "legacy", None, "no ref");
    let linked = memory(&projects, "linked", Some(MINE), "see #12");

    let mut updated = issue_inbox::updated_feedback_memories(MINE);
    updated.sort();
    let mut want = vec![mine.clone(), legacy.clone(), linked];
    want.sort();
    assert_eq!(updated, want, "別セッションのメモリ({other})は除く");

    let mut unlinked = issue_inbox::unlinked_feedback_memories(MINE);
    unlinked.sort();
    let mut want = vec![mine, legacy];
    want.sort();
    assert_eq!(unlinked, want);

    // 別のセッションから見ると、帰属は逆になる(自分の欄が一致するメモリだけ + 欄の無い古いもの)。
    let from_other = issue_inbox::updated_feedback_memories(OTHER);
    assert!(from_other.contains(&other));
    assert!(!from_other.iter().any(|p| p.ends_with("mine.md")));
}
