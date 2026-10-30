//! 仕様(Quint)の軌跡を、実装の畳み込み(`worklog::mail::fold`)に通して一致を確かめる。
//!
//! `config/agents/mail-lifecycle/spec/lifecycle.qnt` を `quint run --out-itf` で走らせた軌跡
//! (ITF)の各状態には、機体ごとのノートの行の写し(`views`)と、仕様が畳み込んだ結果(`fo`)がある。
//! 同じ行を実装の `fold` に通し、項目ごとの状態と食い違いの数が仕様と同じであることを確かめる。
//! 仕様の畳み込みが語彙に従うこと(`s6`・`cov<i>`)は nix の検査が、実装が仕様と同じ畳み込みをする
//! ことはこのテストが担う(docs/agents/mail-lifecycle.md)。
//!
//! 軌跡は nix の check `mail-lifecycle-itf` が作る。置き場を環境変数 `MAIL_LIFECYCLE_ITF_DIR` で渡す。
//! 無ければ skip する(軌跡を作れない機体の `cargo test` を止めない)。あるのに軌跡が 0 件なら失敗する。

use serde_json::Value;
use std::path::PathBuf;
use worklog::mail::{fold, Ev, ItemId, MailLine, Tag};

fn int(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v["#bigint"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or_else(|| panic!("整数でない: {v}"))
}

/// ITF の連想配列 `{"#map": [[k, v], ...]}`。
fn map(v: &Value) -> Vec<(&Value, &Value)> {
    v["#map"]
        .as_array()
        .unwrap_or_else(|| panic!("#map でない: {v}"))
        .iter()
        .map(|p| (&p[0], &p[1]))
        .collect()
}

/// 仕様のキー k(スレッドと、それを開いた受信)。スレッドは 0,1 と 2,3 の 2 組で、受信の順はキーの順。
fn key_parts(k: i64) -> (String, String, String) {
    let thread = format!("t{}", if k >= 2 { 1 } else { 0 });
    let receipt = format!("m{k}");
    (format!("gmail:{thread}/{receipt}"), thread, receipt)
}

fn to_event(line: &Value) -> Ev {
    let (_, thread, receipt) = key_parts(int(&line["k"]));
    Ev {
        day: 20_000 + int(&line["d"]),
        time: None,
        text: String::new(),
        line: MailLine {
            event: line["ev"].as_str().expect("ev").to_string(),
            summary: "x".into(),
            tag: Tag {
                channel: "gmail".into(),
                item: Some(ItemId { thread, receipt }),
                due: None,
                to: None,
            },
        },
    }
}

#[test]
fn the_implementation_folds_the_specs_trace_lines_the_same_way() {
    let Ok(dir) = std::env::var("MAIL_LIFECYCLE_ITF_DIR") else {
        eprintln!("MAIL_LIFECYCLE_ITF_DIR が無いので skip する(nix の check mail-lifecycle-itf が軌跡を作る)");
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{dir} を読めない: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "{dir} に軌跡(*.itf.json)が無い");

    let (mut states, mut nonempty, mut merged) = (0usize, 0usize, 0usize);
    for f in &files {
        let trace: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap())
            .unwrap_or_else(|e| panic!("{} が JSON でない: {e}", f.display()));
        for (i, st) in trace["states"]
            .as_array()
            .expect("states")
            .iter()
            .enumerate()
        {
            let s = &st["s"];
            let fos = map(&s["fo"]);
            for (m, lines) in map(&s["views"]) {
                let m = int(m);
                let evs: Vec<Ev> = lines
                    .as_array()
                    .expect("views の値は行の列")
                    .iter()
                    .map(to_event)
                    .collect();
                let folded = fold(&evs);
                let (_, fo) = fos
                    .iter()
                    .find(|(k, _)| int(k) == m)
                    .unwrap_or_else(|| panic!("fo に機体 {m} が無い"));
                for (k, acc) in map(&fo["a"]) {
                    let k = int(k);
                    let (key, _, _) = key_parts(k);
                    let want = acc["st"].as_str().expect("st");
                    let got = folded.items.get(&key).map_or("none", |i| i.state.as_str());
                    let disc = folded.discrepancies.iter().filter(|d| d.key == key).count() as i64;
                    let ctx = || format!("{} の {i} 状態目、機体 {m}、項目 {k}", f.display());
                    assert_eq!(got, want, "状態が仕様と違う: {}", ctx());
                    assert_eq!(
                        disc,
                        int(&acc["disc"]),
                        "食い違いの数が仕様と違う: {}",
                        ctx()
                    );
                    if want == "superseded"
                        && evs
                            .iter()
                            .filter(|l| l.line.tag.key().as_deref() == Some(&key))
                            .count()
                            > 0
                    {
                        merged += 1;
                    }
                }
                states += 1;
                if !evs.is_empty() {
                    nonempty += 1;
                }
            }
        }
    }
    // 空の軌跡だけを通して「一致した」と言わない。
    assert!(nonempty > 0, "行のある状態が 1 つも無い({states} 状態)");
    eprintln!(
        "軌跡 {} 本、{states} 機体状態(行あり {nonempty}、superseded の項目 {merged})で一致",
        files.len()
    );
}
