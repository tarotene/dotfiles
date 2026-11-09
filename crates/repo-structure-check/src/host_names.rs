//! ホスト名の不変条件: 機体固有の値は、その機体の `home/hosts/<host>.nix`(とその `<host>/` の中)だけが持つ。
//!
//! ホスト名は `home/hosts/*.nix` のファイル名(拡張子を除く)で、これが正本である。`home/` の下の Nix の
//! **文字列**に、ほかの機体のホスト名が単語として現れてはならない。module は、機体の名前を
//! `config.dotfiles.host.name`(`home/modules/host.nix` が宣言する option)で受け取る。
//!
//! - **自分の名前の宣言**: `home/hosts/<host>.nix` は `dotfiles.host.name = "<host>";` を持つ。ファイル名と
//!   宣言がずれると、`~/.config/dotfiles/host` のマーカー(`hms` が機体を決める元)が別の機体を指す。
//! - **マーカーの書き手**: `~/.config/dotfiles/host` を書く(`xdg.configFile."dotfiles/host"`)のは
//!   `home/modules/host.nix` だけ。`xdg.configFile.<名前>.text` は文字列を連結する型なので、書き手が 2 つあると、
//!   評価は通り、マーカーが `vega\n\nvega\n` のように壊れる。
//!
//! 検査は Nix の字句解析(コメントを読まず、文字列の中身と `${…}` の中を区別する)を通して行う。説明の
//! コメントに機体の名前を書くのは構わない(実値でなく、読み手への説明だから)。
//! 自分の機体のファイル(`home/hosts/<host>.nix` と `home/hosts/<host>/**`)の中の文字列は、自分の名前を
//! 持ってよい。

use std::fs;
use std::path::Path;

use crate::manifest::Violation;

/// 検査する Nix のソース 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixFile {
    /// リポジトリの根からの相対パス(`/` 区切り)。
    pub path: String,
    pub text: String,
}

/// マーカー(`~/.config/dotfiles/host`)を書く唯一の module。
pub const MARKER_WRITER: &str = "home/modules/host.nix";

/// マーカーの `xdg.configFile` の名前。
const MARKER_KEY: &str = "\"dotfiles/host\"";

/// 機体が自分の名前を宣言する option。
const DECLARATION: &str = "dotfiles.host.name";

fn violation(subject: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: subject.to_string(),
        detail: detail.into(),
    }
}

/// 文字列の静的な部分 1 つ(`${…}` の補間は含まない)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrLit {
    /// 部分の最初の文字がある行(1 から)。
    pub line: usize,
    pub text: String,
}

/// 字句解析の結果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// 文字列の静的な部分(補間の中にある文字列も含む)。
    pub strings: Vec<StrLit>,
    /// コメントを除いたソース。文字列の中身は残す。
    pub code: String,
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    line: usize,
    out: Scan,
}

impl Lexer {
    fn peek(&self, k: usize) -> Option<char> {
        self.chars.get(self.i + k).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.i).copied()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn push_segment(&mut self, seg: &mut String, seg_line: usize) {
        if !seg.is_empty() {
            self.out.strings.push(StrLit {
                line: seg_line,
                text: std::mem::take(seg),
            });
        }
    }

    /// コードを読む。`interpolation` のときは、対応する `}` の手前で戻る(`}` は読まない)。
    fn code(&mut self, interpolation: bool) {
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            match c {
                '#' => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                '/' if self.peek(1) == Some('*') => {
                    self.bump();
                    self.bump();
                    while self.peek(0).is_some()
                        && !(self.peek(0) == Some('*') && self.peek(1) == Some('/'))
                    {
                        self.bump();
                    }
                    self.bump();
                    self.bump();
                }
                '"' => {
                    self.out.code.push('"');
                    self.bump();
                    self.double_quoted();
                    self.out.code.push('"');
                }
                '\'' if self.peek(1) == Some('\'') => {
                    self.out.code.push_str("''");
                    self.bump();
                    self.bump();
                    self.indented();
                    self.out.code.push_str("''");
                }
                '{' => {
                    depth += 1;
                    self.out.code.push('{');
                    self.bump();
                }
                '}' => {
                    if interpolation && depth == 0 {
                        return;
                    }
                    depth = depth.saturating_sub(1);
                    self.out.code.push('}');
                    self.bump();
                }
                _ => {
                    self.out.code.push(c);
                    self.bump();
                }
            }
        }
    }

    /// `${` を読んだあとで、補間の中のコードを読み、閉じの `}` を読み捨てる。
    fn interpolation(&mut self) {
        self.out.code.push_str("${");
        self.code(true);
        self.bump();
        self.out.code.push('}');
    }

    /// `"` の次から、閉じの `"` まで。
    fn double_quoted(&mut self) {
        let mut seg = String::new();
        let mut seg_line = self.line;
        while let Some(c) = self.peek(0) {
            match c {
                '"' => {
                    self.bump();
                    break;
                }
                '\\' => {
                    self.bump();
                    if let Some(e) = self.bump() {
                        if seg.is_empty() {
                            seg_line = self.line;
                        }
                        seg.push(e);
                        self.out.code.push(e);
                    }
                }
                '$' if self.peek(1) == Some('{') => {
                    self.push_segment(&mut seg, seg_line);
                    self.bump();
                    self.bump();
                    self.interpolation();
                    seg_line = self.line;
                }
                _ => {
                    if seg.is_empty() {
                        seg_line = self.line;
                    }
                    seg.push(c);
                    self.out.code.push(c);
                    self.bump();
                }
            }
        }
        self.push_segment(&mut seg, seg_line);
    }

    /// `''` の次から、閉じの `''` まで。`'''`・`''$`・`''\x` はエスケープ。
    fn indented(&mut self) {
        let mut seg = String::new();
        let mut seg_line = self.line;
        loop {
            let (a, b, c) = (self.peek(0), self.peek(1), self.peek(2));
            match (a, b, c) {
                (None, _, _) => break,
                (Some('\''), Some('\''), Some('\'')) => {
                    if seg.is_empty() {
                        seg_line = self.line;
                    }
                    seg.push_str("''");
                    self.out.code.push_str("''");
                    self.bump();
                    self.bump();
                    self.bump();
                }
                (Some('\''), Some('\''), Some('$')) => {
                    if seg.is_empty() {
                        seg_line = self.line;
                    }
                    seg.push('$');
                    self.out.code.push('$');
                    self.bump();
                    self.bump();
                    self.bump();
                }
                (Some('\''), Some('\''), Some('\\')) => {
                    self.bump();
                    self.bump();
                    self.bump();
                    if let Some(e) = self.bump() {
                        if seg.is_empty() {
                            seg_line = self.line;
                        }
                        seg.push(e);
                        self.out.code.push(e);
                    }
                }
                (Some('\''), Some('\''), _) => {
                    self.bump();
                    self.bump();
                    break;
                }
                (Some('$'), Some('{'), _) => {
                    self.push_segment(&mut seg, seg_line);
                    self.bump();
                    self.bump();
                    self.interpolation();
                    seg_line = self.line;
                }
                (Some(ch), _, _) => {
                    if seg.is_empty() {
                        seg_line = self.line;
                    }
                    seg.push(ch);
                    self.out.code.push(ch);
                    self.bump();
                }
            }
        }
        self.push_segment(&mut seg, seg_line);
    }
}

/// Nix のソースを字句解析する。文字列の静的な部分と、コメントを除いたソースを返す。
pub fn lex(nix: &str) -> Scan {
    let mut lx = Lexer {
        chars: nix.chars().collect(),
        i: 0,
        line: 1,
        out: Scan::default(),
    };
    lx.code(false);
    lx.out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `text` の中で `word` が単語として(前後が単語の文字でなく)最初に現れるバイト位置。
pub fn find_word(text: &str, word: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(pos) = text[from..].find(word) {
        let start = from + pos;
        let end = start + word.len();
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after_ok = text[end..].chars().next().is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            return Some(start);
        }
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// `text` に `word` が単語として現れるか。
pub fn contains_word(text: &str, word: &str) -> bool {
    find_word(text, word).is_some()
}

/// `home/hosts/<stem>.nix` のファイル名(拡張子を除く)の一覧。ホスト名の正本。
pub fn host_names(files: &[NixFile]) -> Vec<String> {
    let mut out: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let rest = f.path.strip_prefix("home/hosts/")?;
            let stem = rest.strip_suffix(".nix")?;
            (!stem.contains('/')).then(|| stem.to_string())
        })
        .collect();
    out.sort();
    out
}

/// このファイルを持つ機体。`home/hosts/<host>.nix` と `home/hosts/<host>/**` はその機体のもの。
fn owner<'a>(path: &str, hosts: &'a [String]) -> Option<&'a str> {
    hosts
        .iter()
        .find(|h| {
            path == format!("home/hosts/{h}.nix") || path.starts_with(&format!("home/hosts/{h}/"))
        })
        .map(String::as_str)
}

/// `dotfiles.host.name = "<name>";` の `<name>`。無ければ None。
fn declared_name(code: &str) -> Option<String> {
    let at = code.find(DECLARATION)?;
    let rest = code[at + DECLARATION.len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// ホスト名の不変条件を検査する。
pub fn check_host_names(files: &[NixFile]) -> Vec<Violation> {
    let hosts = host_names(files);
    let mut out = Vec::new();

    for f in files {
        let scan = lex(&f.text);
        let mine = owner(&f.path, &hosts);

        for lit in &scan.strings {
            for h in hosts.iter().filter(|h| Some(h.as_str()) != mine) {
                if let Some(pos) = find_word(&lit.text, h) {
                    // 文字列の先頭の行に、単語の手前の改行の数を足す(複数行の文字列でも、単語がある行を示す)。
                    let line = lit.line + lit.text[..pos].matches('\n').count();
                    out.push(violation(
                        &f.path,
                        format!(
                            "{line} 行目の文字列にホスト名 `{h}` がある。機体固有の値は `home/hosts/{h}.nix` が `dotfiles.host.name` などの option で与え、module は `config.dotfiles.host.name` で受け取る(説明は `<host>` のように書く)",
                        ),
                    ));
                }
            }
        }

        if f.path != MARKER_WRITER && scan.code.contains(MARKER_KEY) {
            out.push(violation(
                &f.path,
                format!(
                    "{MARKER_KEY} を書いている。`~/.config/dotfiles/host` のマーカーを書くのは {MARKER_WRITER} だけ(`dotfiles.host.name` を宣言する)。ほかで書くと、文字列が連結されてマーカーが壊れる"
                ),
            ));
        }

        let own_file = hosts
            .iter()
            .find(|h| f.path == format!("home/hosts/{h}.nix"));
        if let Some(h) = own_file {
            match declared_name(&scan.code) {
                Some(name) if name == *h => {}
                Some(name) => out.push(violation(
                    &f.path,
                    format!("`{DECLARATION}` が `{name}` で、ファイル名のホスト名 `{h}` と違う"),
                )),
                None => out.push(violation(
                    &f.path,
                    format!("`{DECLARATION} = \"{h}\";` が無い。機体は自分の名前を宣言する"),
                )),
            }
        }
    }

    match files.iter().find(|f| f.path == MARKER_WRITER) {
        Some(f) if lex(&f.text).code.contains(MARKER_KEY) => {}
        Some(f) => out.push(violation(
            &f.path,
            format!("{MARKER_KEY} を書いていない。マーカーを書くのはこの module の仕事"),
        )),
        None => out.push(violation(
            MARKER_WRITER,
            "ファイルが無い。マーカー(`~/.config/dotfiles/host`)を書く module が要る",
        )),
    }

    out
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<NixFile>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{} を読めない: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `home/lib/import-dir.nix` が一覧に含めない規則と同じ。追跡外の隠しファイルは読まない。
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            collect(&p, root, out)?;
        } else if p.is_file() && name.ends_with(".nix") {
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let text =
                fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))?;
            out.push(NixFile { path: rel, text });
        }
    }
    Ok(())
}

/// `home/**/*.nix` を全部読む。
pub fn read_files(root: &Path) -> Result<Vec<NixFile>, String> {
    let mut out = Vec::new();
    collect(&root.join("home"), root, &mut out)?;
    Ok(out)
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    Ok(check_host_names(&read_files(root)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nix(path: &str, text: &str) -> NixFile {
        NixFile {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    /// 正しい最小のツリー: 2 つの機体と、マーカーを書く module。
    fn ok_tree() -> Vec<NixFile> {
        vec![
            nix("home/hosts/vega.nix", "{ dotfiles.host.name = \"vega\"; }\n"),
            nix("home/hosts/arcturus.nix", "{ dotfiles.host.name = \"arcturus\"; }\n"),
            nix(
                MARKER_WRITER,
                "{ config, ... }: { xdg.configFile.\"dotfiles/host\".text = \"${config.dotfiles.host.name}\\n\"; }\n",
            ),
            nix("home/modules/a.nix", "{ x = \"hello\"; }\n"),
        ]
    }

    fn details(v: &[Violation]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_clean_tree_has_no_violations() {
        assert_eq!(details(&check_host_names(&ok_tree())), Vec::<String>::new());
    }

    // ---- 字句解析 ----

    fn strings(src: &str) -> Vec<String> {
        lex(src).strings.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn comments_are_not_read() {
        let s = "# vega\nx = 1; /* vega\narcturus */ y = 2;\n";
        assert!(strings(s).is_empty());
        assert!(!lex(s).code.contains("vega"));
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        assert_eq!(strings("x = \"a # vega\";\n# tail\n"), ["a # vega"]);
        assert_eq!(strings("x = ''a # vega\nb'';\n"), ["a # vega\nb"]);
    }

    #[test]
    fn interpolation_splits_the_static_parts() {
        assert_eq!(strings("x = \"ve${a}ga\";"), ["ve", "ga"]);
        assert_eq!(strings("x = ''ve${a}ga'';"), ["ve", "ga"]);
        // 補間の中の文字列も、文字列として読む。
        assert_eq!(strings("x = \"a${ \"vega\" }b\";"), ["a", "vega", "b"]);
        // 補間の中の波括弧と、入れ子。
        assert_eq!(strings("x = \"${ { a = \"in\"; }.a }out\";"), ["in", "out"]);
    }

    #[test]
    fn escapes_in_double_quoted_strings() {
        // `\"` は文字列を閉じない。`\${` は補間でない。
        assert_eq!(
            strings(r#"x = "a\"vega\"b"; y = "c";"#),
            ["a\"vega\"b", "c"]
        );
        assert_eq!(strings(r#"x = "\${vega}";"#), ["${vega}"]);
    }

    #[test]
    fn escapes_in_indented_strings() {
        // `'''` は `''`、`''$` は `$`(その直後の `{` は補間でない)、`''\n` は改行。
        assert_eq!(strings("x = ''a'''b'';"), ["a''b"]);
        assert_eq!(strings("x = ''a''${vega}b'';"), ["a${vega}b"]);
        assert_eq!(strings("x = ''a''\\nb'';"), ["anb"]);
    }

    #[test]
    fn a_reported_line_is_the_line_of_the_word_in_a_multiline_string() {
        let t = vec![
            nix(
                "home/hosts/vega.nix",
                "{ dotfiles.host.name = \"vega\"; }\n",
            ),
            nix(
                "home/hosts/arcturus.nix",
                "{ dotfiles.host.name = \"arcturus\"; }\n",
            ),
            nix(
                MARKER_WRITER,
                "{ xdg.configFile.\"dotfiles/host\".text = \"x\"; }\n",
            ),
            nix(
                "home/modules/b.nix",
                "a = 1;\nb = \'\'\n  x\n  vega\n\'\';\n",
            ),
        ];
        let v = details(&check_host_names(&t));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].starts_with("home/modules/b.nix: 4 行目"), "{v:?}");
    }

    // ---- 単語の判定 ----

    #[test]
    fn host_names_match_whole_words_only() {
        assert!(contains_word("home/hosts/vega.nix", "vega"));
        assert!(contains_word("vega's kernel", "vega"));
        assert!(contains_word("vega", "vega"));
        assert!(contains_word("x vega-only", "vega"));
        assert!(!contains_word("vegan", "vega"));
        assert!(!contains_word("megavega", "vega"));
        assert!(!contains_word("vega2", "vega"));
        assert!(!contains_word("my_vega", "vega"));
        assert!(contains_word("nope vegan and vega", "vega"));
    }

    // ---- 検査 ----

    #[test]
    fn a_host_name_in_a_module_string_is_reported() {
        let mut t = ok_tree();
        t.push(nix("home/modules/b.nix", "{ ConditionHost = \"vega\"; }\n"));
        let v = details(&check_host_names(&t));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("home/modules/b.nix: 1 行目の文字列にホスト名 `vega`"),
            "{v:?}"
        );
    }

    #[test]
    fn a_host_name_in_a_description_string_is_reported() {
        let mut t = ok_tree();
        t.push(nix(
            "home/modules/b.nix",
            "{\n  description = ''\n    set in `home/hosts/arcturus.nix`\n  '';\n}\n",
        ));
        let v = details(&check_host_names(&t));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].contains("3 行目") && v[0].contains("`arcturus`"),
            "{v:?}"
        );
    }

    #[test]
    fn a_host_name_in_a_comment_is_allowed() {
        let mut t = ok_tree();
        t.push(nix(
            "home/modules/b.nix",
            "# measured on vega\n{ x = 1; } # arcturus\n",
        ));
        assert_eq!(details(&check_host_names(&t)), Vec::<String>::new());
    }

    #[test]
    fn a_host_may_name_itself_but_not_another_host() {
        let mut t = ok_tree();
        t.push(nix(
            "home/hosts/vega/obsidian.nix",
            "{ x = \"vega only\"; }\n",
        ));
        assert_eq!(details(&check_host_names(&t)), Vec::<String>::new());
        t.push(nix(
            "home/hosts/vega/leak.nix",
            "{ x = \"like arcturus\"; }\n",
        ));
        let v = details(&check_host_names(&t));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("home/hosts/vega/leak.nix:") && v[0].contains("`arcturus`"),
            "{v:?}"
        );
    }

    #[test]
    fn a_host_file_must_declare_its_own_name() {
        let mut t = ok_tree();
        t[0] = nix("home/hosts/vega.nix", "{ }\n");
        let v = details(&check_host_names(&t));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].contains("home/hosts/vega.nix") && v[0].contains("が無い"),
            "{v:?}"
        );
    }

    #[test]
    fn a_declaration_that_differs_from_the_file_name_is_reported() {
        let mut t = ok_tree();
        t[0] = nix(
            "home/hosts/vega.nix",
            "{ dotfiles.host.name = \"arcturus\"; }\n",
        );
        let v = details(&check_host_names(&t));
        // ファイル名と違う宣言。`arcturus` を vega の文字列が持つ違反も、同じ原因で出る。
        assert!(
            v.iter()
                .any(|m| m.contains("ファイル名のホスト名 `vega` と違う")),
            "{v:?}"
        );
    }

    #[test]
    fn a_declaration_in_a_comment_does_not_count() {
        let mut t = ok_tree();
        t[0] = nix(
            "home/hosts/vega.nix",
            "# dotfiles.host.name = \"vega\";\n{ }\n",
        );
        let v = details(&check_host_names(&t));
        assert!(v.iter().any(|m| m.contains("が無い")), "{v:?}");
    }

    #[test]
    fn the_marker_is_written_only_by_the_marker_module() {
        let mut t = ok_tree();
        t.push(nix(
            "home/hosts/leftover.nix",
            "{ dotfiles.host.name = \"leftover\"; xdg.configFile.\"dotfiles/host\".text = \"x\\n\"; }\n",
        ));
        let v = details(&check_host_names(&t));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("home/hosts/leftover.nix:") && v[0].contains("マーカーを書くのは"),
            "{v:?}"
        );
    }

    #[test]
    fn a_missing_marker_module_is_reported() {
        let mut t = ok_tree();
        t.retain(|f| f.path != MARKER_WRITER);
        let v = details(&check_host_names(&t));
        assert!(v.iter().any(|m| m.contains("ファイルが無い")), "{v:?}");
        let mut t = ok_tree();
        t[2] = nix(MARKER_WRITER, "{ }\n");
        let v = details(&check_host_names(&t));
        assert!(v.iter().any(|m| m.contains("書いていない")), "{v:?}");
    }

    #[test]
    fn host_names_come_from_the_file_names_in_hosts() {
        assert_eq!(host_names(&ok_tree()), ["arcturus", "vega"]);
        // サブディレクトリの中の .nix と、home/hosts の外は、ホスト名にならない。
        let t = vec![
            nix("home/hosts/vega/x.nix", ""),
            nix("home/modules/vega.nix", ""),
        ];
        assert!(host_names(&t).is_empty());
    }
}
