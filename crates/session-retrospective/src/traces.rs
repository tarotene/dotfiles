//! その場で書いたコードの痕跡(ADR-1276)。
//!
//! PR を作らないセッション(メールの裁定・調査・机の作業)でも、scratchpad に使い捨てのスクリプトを
//! 書いたり、`python3 -c` や長い jq を打ったりする。それを「決定論化できないか」という目で振り返るために、
//! 機械で拾える痕跡を 3 つ数える。
//!
//! 1. このセッション専用の scratchpad にあるコード(コードの拡張子か shebang)。
//! 2. 形(`cmd-hash-log` が添える `shape`)を持つコマンドのハッシュが、セッション内で繰り返された。
//! 3. その場のスクリプトの形(`heredoc-interp` など強い形)が、合わせて何回か出た。
//!
//! しきい値と、コードの拡張子・形の強弱は `config/agents/codify-signals/vocabulary.json` が正本。
//! 素の「同じコマンドの反復」は数えない(過去 247 セッション中 106 で起きる。`cargo test` のやり直しなど)。
//! 本文は読まない・残さない。ここで扱うのはファイル名・ハッシュの先頭 8 文字・形の語だけ。
//!
//! 純関数(`scratch_code_files`・`parse_records`・`Traces::analyze`)と、環境を読む `collect` を分けてある。

use issue_inbox::codify::Vocabulary;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `cmd-hashes.jsonl` の 1 行のうち、痕跡の判定に使う欄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub hash: String,
    pub shape: Vec<String>,
}

/// セッションの痕跡。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Traces {
    /// scratchpad にあるコードのファイル名。
    pub scratch: Vec<String>,
    /// 形を持つコマンドで、セッション内に `shaped_repeat` 回以上出たもの `(hash8, 回数, 形)`。
    pub repeats: Vec<(String, usize, Vec<String>)>,
    /// 強い形ごとの回数。
    pub strong_shapes: BTreeMap<String, usize>,
}

/// scratchpad の直下と 1 段下から、コードのファイルの名前を集める。コードとは、語彙の拡張子を持つか、
/// 先頭が `#!` のもの(PR 本文や JSON など、コードでないファイルも scratchpad には置く)。
pub fn scratch_code_files(dir: &Path, exts: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    collect_code(dir, exts, 2, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_code(dir: &Path, exts: &[String], depth: usize, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if depth > 1 {
                collect_code(&path, exts, depth - 1, out);
            }
        } else if ft.is_file() && is_code(&path, exts) {
            if let Some(n) = path.file_name() {
                out.push(n.to_string_lossy().into_owned());
            }
        }
    }
}

fn is_code(path: &Path, exts: &[String]) -> bool {
    let by_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| x == e));
    if by_ext {
        return true;
    }
    use std::io::Read;
    let mut head = [0u8; 2];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && &head == b"#!"
}

/// `cmd-hashes.jsonl` のテキストから、このセッションの行を取り出す。sid の部分一致で先に絞ってから
/// JSON を読む(ファイルは全セッション分で、数 MB まで育つ)。`shape` の欄が無い行は形なし。
pub fn parse_records(log: &str, sid: &str) -> Vec<Record> {
    log.lines()
        .filter(|l| l.contains(sid))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["session_id"] == sid)
        .filter_map(|v| {
            Some(Record {
                hash: v["hash"].as_str()?.to_string(),
                shape: v["shape"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

impl Traces {
    pub fn analyze(scratch: Vec<String>, records: &[Record], vocab: &Vocabulary) -> Traces {
        let strong = |id: &str| vocab.shapes.iter().any(|s| s.strong && s.id == id);
        let mut by_hash: BTreeMap<&str, (usize, Vec<String>)> = BTreeMap::new();
        let mut strong_shapes: BTreeMap<String, usize> = BTreeMap::new();
        for r in records.iter().filter(|r| !r.shape.is_empty()) {
            by_hash.entry(&r.hash).or_default().0 += 1;
            by_hash.entry(&r.hash).or_default().1 = r.shape.clone();
            for s in r.shape.iter().filter(|s| strong(s)) {
                *strong_shapes.entry(s.clone()).or_default() += 1;
            }
        }
        let min = vocab.fire.shaped_repeat as usize;
        let repeats = by_hash
            .into_iter()
            .filter(|(_, (n, _))| *n >= min)
            .map(|(h, (n, shapes))| (h.chars().take(8).collect(), n, shapes))
            .collect();
        Traces {
            scratch,
            repeats,
            strong_shapes,
        }
    }

    /// しきい値を超えているか: scratchpad のコードが `scratch_code_files` 個以上、形を持つコマンドの反復が
    /// 1 つ以上、または強い形が合わせて `strong_shapes_total` 回以上。
    pub fn fires(&self, vocab: &Vocabulary) -> bool {
        self.scratch.len() >= vocab.fire.scratch_code_files as usize
            || !self.repeats.is_empty()
            || self.strong_shapes.values().sum::<usize>() >= vocab.fire.strong_shapes_total as usize
    }

    /// 行の `evidence` に引くべき出来事 `(id, 説明)`。しきい値を超えていなければ空(1 回の
    /// `python3 -c` だけでは、振り返りを求めない)。
    pub fn events(&self, vocab: &Vocabulary) -> Vec<(String, String)> {
        if !self.fires(vocab) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for f in &self.scratch {
            out.push((
                format!("adhoc:scratch:{f}"),
                format!("code written in this session's scratchpad: {f}"),
            ));
        }
        for (h, n, shapes) in &self.repeats {
            out.push((
                format!("adhoc:repeat:{h}"),
                format!(
                    "the same ad-hoc command ({}) ran {n} times",
                    shapes.join(", ")
                ),
            ));
        }
        for (s, n) in &self.strong_shapes {
            out.push((
                format!("adhoc:shape:{s}"),
                format!("{n} command(s) of shape {s} (a script written on the spot)"),
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 環境を読む部分

/// このセッション専用の scratchpad の候補(`<root>/<プロジェクトの slug>/<session_id>/scratchpad`)。
/// root は `$CLAUDE_CODE_TMPDIR` と `/tmp/claude-<uid>`。slug は Claude Code が決めるので導かず、
/// root の直下を走査して session_id の一致で選ぶ(UUID なので、他のセッションとは重ならない)。
/// 全セッションで共有の `$TMPDIR` は見ない。
pub fn scratch_dirs(sid: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let uid = std::env::var_os("HOME")
        .and_then(|h| std::fs::metadata(h).ok())
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m));
    if let Some(t) = issue_inbox::env_nonempty("CLAUDE_CODE_TMPDIR") {
        let t = PathBuf::from(t);
        if let Some(uid) = uid {
            roots.push(t.join(format!("claude-{uid}")));
        }
        roots.push(t);
    }
    if let Some(uid) = uid {
        roots.push(PathBuf::from(format!("/tmp/claude-{uid}")));
    }
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for project in rd.flatten() {
            let d = project.path().join(sid).join("scratchpad");
            if d.is_dir() && !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

/// このセッションの痕跡を、環境から集める。
pub fn collect(sid: &str) -> Traces {
    let vocab = Vocabulary::get();
    let mut scratch = Vec::new();
    for d in scratch_dirs(sid) {
        scratch.extend(scratch_code_files(&d, &vocab.scratch_code_ext));
    }
    scratch.sort();
    scratch.dedup();
    let log = cmd_hash_log::resolve_log_path(
        std::env::var_os("CMD_HASH_LOG_PATH").as_deref(),
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .and_then(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_default();
    Traces::analyze(scratch, &parse_records(&log, sid), vocab)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(hash: &str, shape: &[&str]) -> Record {
        Record {
            hash: hash.into(),
            shape: shape.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn v() -> &'static Vocabulary {
        Vocabulary::get()
    }

    #[test]
    fn a_plain_repeat_does_not_fire() {
        // cargo test のやり直し: 同じハッシュが何回でも、形が無ければ痕跡ではない。
        let recs: Vec<Record> = (0..5).map(|_| rec("aaaaaaaaaaaaaaaa", &[])).collect();
        let t = Traces::analyze(vec![], &recs, v());
        assert!(!t.fires(v()));
        assert!(t.events(v()).is_empty());
    }

    #[test]
    fn one_inline_interpreter_does_not_fire_but_three_do() {
        let one = vec![rec("1111111111111111", &["inline-interp"])];
        assert!(!Traces::analyze(vec![], &one, v()).fires(v()));
        let three = vec![
            rec("1111111111111111", &["inline-interp"]),
            rec("2222222222222222", &["inline-interp"]),
            rec("3333333333333333", &["heredoc-interp"]),
        ];
        let t = Traces::analyze(vec![], &three, v());
        assert!(t.fires(v()));
        let ids: Vec<String> = t.events(v()).into_iter().map(|e| e.0).collect();
        assert_eq!(
            ids,
            ["adhoc:shape:heredoc-interp", "adhoc:shape:inline-interp"]
        );
    }

    #[test]
    fn a_shaped_command_repeated_fires_even_if_the_shape_is_weak() {
        let recs = vec![
            rec("4444444444444444", &["pipeline"]),
            rec("4444444444444444", &["pipeline"]),
        ];
        let t = Traces::analyze(vec![], &recs, v());
        assert!(t.fires(v()));
        let ids: Vec<String> = t.events(v()).into_iter().map(|e| e.0).collect();
        // 弱い形(pipeline)は反復としてだけ数え、形の出来事にはしない。
        assert_eq!(ids, ["adhoc:repeat:44444444"]);
    }

    #[test]
    fn a_weak_shape_once_does_not_fire() {
        let recs = vec![rec("5555555555555555", &["pipeline"])];
        assert!(!Traces::analyze(vec![], &recs, v()).fires(v()));
    }

    #[test]
    fn scratch_code_fires_with_one_file() {
        let t = Traces::analyze(vec!["x.py".into()], &[], v());
        assert!(t.fires(v()));
        assert_eq!(t.events(v())[0].0, "adhoc:scratch:x.py");
    }

    #[test]
    fn only_code_files_in_the_scratchpad_count() {
        let d = tempfile::tempdir().unwrap();
        let exts: Vec<String> = v().scratch_code_ext.clone();
        std::fs::write(d.path().join("body.md"), "# pr body").unwrap();
        std::fs::write(d.path().join("findings.json"), "{}").unwrap();
        assert!(scratch_code_files(d.path(), &exts).is_empty());
        std::fs::write(d.path().join("x.py"), "print(1)").unwrap();
        std::fs::write(d.path().join("noext"), "#!/bin/sh\nls\n").unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub").join("y.sh"), "ls").unwrap();
        assert_eq!(
            scratch_code_files(d.path(), &exts),
            ["noext", "x.py", "y.sh"]
        );
    }

    #[test]
    fn records_of_other_sessions_and_old_records_are_handled() {
        let log = concat!(
            r#"{"hash":"aaaaaaaaaaaaaaaa","repo_key":"/r","session_id":"s1","ts_unix":1}"#,
            "\n",
            r#"{"hash":"bbbbbbbbbbbbbbbb","repo_key":"/r","session_id":"s2","shape":["pipeline"],"ts_unix":2}"#,
            "\n",
            r#"{"hash":"cccccccccccccccc","repo_key":"/r","session_id":"s1","shape":["inline-interp"],"ts_unix":3}"#,
            "\n",
            "not json s1\n",
        );
        let recs = parse_records(log, "s1");
        assert_eq!(
            recs,
            [
                rec("aaaaaaaaaaaaaaaa", &[]),
                rec("cccccccccccccccc", &["inline-interp"])
            ]
        );
    }

    #[test]
    fn events_never_carry_command_text() {
        let recs = vec![
            rec("6666666666666666", &["inline-interp"]),
            rec("6666666666666666", &["inline-interp"]),
        ];
        for (id, desc) in Traces::analyze(vec![], &recs, v()).events(v()) {
            assert!(id.len() < 40 && desc.len() < 120, "{id} {desc}");
        }
    }
}
