//! composite action の `action.yml` のメタデータの `description:`(トップ・`inputs.*`・
//! `outputs.*`)に式の構文(`${{`)を書くと、GitHub はそれを式として評価する。
//! `cancelled()` のように action のメタデータの文脈で使えない関数を含むと、action の
//! 読み込み自体が失敗する(#1011。#1009 の `stack-stage` の `Failed to load .../action.yml`)。
//! 手元にも push 前にも読み込みを検査する手段が無いので、`cargo test` で落とす。
//!
//! `outputs.*.value` や `run:` / `env:` の式は正当なので見ない。式を例として書きたい
//! ときは、式の構文を使わない言い回し(「`!cancelled()` かつ …」)にする。
//!
//! 依存ゼロの crate なので YAML パーサは使わず、行の字下げで `description:` の値(同じ行と、
//! それより深く字下げした続きの行)を取る。`run:` などのブロックスカラの中は読まない
//! (シェルの本文に `description:` と `${{` が並ぶのは別の話)。

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// `description:` の値(および続きの行)に `${{` を含む箇所の (行番号, 行)。
fn expressions_in_descriptions(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let key = line.trim_start_matches([' ', '-']);
        let key_indent = indent(line);
        // 値を読まない(スクリプト本文の)ブロックスカラ。
        let is_script_key = ["run:", "script:"].iter().any(|k| key.starts_with(k));
        let is_description = key.starts_with("description:");
        if !is_script_key && !is_description {
            i += 1;
            continue;
        }
        // この key の値の範囲: 同じ行の残りと、key より深く字下げした続きの行。
        let mut end = i + 1;
        while end < lines.len() && (lines[end].trim().is_empty() || indent(lines[end]) > key_indent)
        {
            end += 1;
        }
        if is_description {
            for (n, l) in lines.iter().enumerate().take(end).skip(i) {
                if l.contains("${{") {
                    found.push((n + 1, (*l).to_string()));
                }
            }
        }
        i = end;
    }
    found
}

/// リポジトリ内の composite action(dotfiles 自身と、言語別テンプレートが播く分)。
fn action_files(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.join(".github/actions")];
    if let Ok(skills) = std::fs::read_dir(root.join("config/agents/skills")) {
        for skill in skills.flatten() {
            dirs.push(skill.path().join("templates/.github/actions"));
        }
    }
    let mut files = Vec::new();
    for dir in dirs {
        let Ok(actions) = std::fs::read_dir(&dir) else {
            continue;
        };
        for action in actions.flatten() {
            for name in ["action.yml", "action.yaml"] {
                let f = action.path().join(name);
                if f.is_file() {
                    files.push(f);
                }
            }
        }
    }
    files.sort();
    files
}

#[test]
fn no_action_yml_description_contains_an_expression() {
    let root = repo_root();
    let files = action_files(&root);
    assert!(
        files.len() >= 7,
        "action.yml を拾えていない(.github/actions/* と templates の action): {files:?}"
    );
    let mut bad = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (n, line) in expressions_in_descriptions(&text) {
            bad.push(format!("{}:{n}: {}", f.display(), line.trim()));
        }
    }
    assert!(
        bad.is_empty(),
        "action.yml の description に `${{{{` があります(GitHub が式として評価し、action の読み込みが失敗しうる。\
         式を例に書くときは、式の構文を使わない言い回しにする):\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_check_finds_expressions_in_every_description_form() {
    // 失敗した実例(#1009)と同じ、トップの block scalar の中の式。
    let top = "name: x\ndescription: >-\n  使い方: if: ${{ !cancelled() && foo }}\nruns:\n  using: composite\n";
    assert_eq!(expressions_in_descriptions(top).len(), 1);
    // inputs / outputs の 1 行の description。
    let nested = "inputs:\n  a:\n    description: \"see ${{ github.token }}\"\n    required: true\noutputs:\n  b:\n    description: x ${{ y }}\n    value: ${{ steps.s.outputs.b }}\n";
    assert_eq!(
        expressions_in_descriptions(nested)
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>(),
        vec![3, 7]
    );
}

#[test]
fn the_check_ignores_legitimate_expressions() {
    let ok = "\
name: x
description: >-
  式を使わない言い回し(`!cancelled()` かつ ...)
inputs:
  a:
    description: plain
    default: ${{ github.token }}
outputs:
  b:
    description: plain
    value: ${{ steps.s.outputs.b }}
runs:
  using: composite
  steps:
    - shell: bash
      env:
        T: ${{ github.token }}
      run: |
        description: ${{ not read }}
        echo hi
";
    assert!(expressions_in_descriptions(ok).is_empty());
}
