//! record-layer — 記録の層(L0〜L4)の宣言を読み、パスの層を引く(#1310、ADR-0000 record-layers)。
//!
//! 個人のリポジトリに追跡されている記録を、効いている期間(入れ替わる速さ)で L0〜L4 に分ける。
//! 宣言はリポジトリの `.config/record-layers.toml` で、「パス(glob)→ 層」の配列。語彙は
//! L0〜L4 の閉集合で、1 パス 1 層。宣言の無いリポジトリでは、何も発火しない(opt-in)。
//!
//! - この lib(宣言の読み込みと、パスの層の引き方): 単一ソース。
//! - [`hook`]: Write の拒否の判定(`hook-router` の規則が呼ぶ)。
//! - [`drift`]: 日次のずれ検出の純粋関数(`promotion-detect` が呼ぶ)。

pub mod drift;
pub mod hook;

use globset::{Glob, GlobBuilder, GlobMatcher};
use serde::Deserialize;
use std::fmt;

/// 宣言ファイルの、リポジトリのルートからの相対パス。
pub const DECLARATION_PATH: &str = ".config/record-layers.toml";

/// 記録の層。効いている期間(現在の事実として効く期間)で決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub enum Layer {
    /// 不変(追記のみ。月〜年)。Git。
    L0,
    /// 年〜月(月〜年)。Git。
    L1,
    /// 週。Git。
    L2,
    /// 日〜都度(1 週未満)。Git の外(Obsidian Vault が第一候補)。
    L3,
    /// 機械生成(生成のたび)。Git の外(vault の外)。
    L4,
}

impl Layer {
    pub const ALL: [Layer; 5] = [Layer::L0, Layer::L1, Layer::L2, Layer::L3, Layer::L4];

    pub fn name(self) -> &'static str {
        match self {
            Layer::L0 => "L0",
            Layer::L1 => "L1",
            Layer::L2 => "L2",
            Layer::L3 => "L3",
            Layer::L4 => "L4",
        }
    }

    /// 記録の種類を表す短い呼び名(拒否・検出の文面に使う)。
    pub fn title(self) -> &'static str {
        match self {
            Layer::L0 => "不変(追記のみ)",
            Layer::L1 => "年〜月",
            Layer::L2 => "週",
            Layer::L3 => "日〜都度(状態・済んだことの記録・観測の行・台帳の行)",
            Layer::L4 => "機械生成(外部の写し・キャッシュ・生成物・スナップショット)",
        }
    }

    /// この層の記録を Git に置いてよいか。L3・L4 は Git の外に置く。
    pub fn in_git(self) -> bool {
        matches!(self, Layer::L0 | Layer::L1 | Layer::L2)
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeclaration {
    #[serde(default)]
    path: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    glob: String,
    layer: Layer,
}

/// 宣言を読めなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationError {
    /// TOML として読めない、未知の層・キーがある。
    Syntax(String),
    /// `[[path]]` が 1 つも無い。すべてのパスが「宣言に無い」になるので、壊れた宣言として扱う。
    Empty,
    /// glob が書けない形(`/` や `./` で始まる、`..` を含む、構文の誤り)。
    BadGlob { glob: String, reason: String },
}

impl fmt::Display for DeclarationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeclarationError::Syntax(e) => write!(f, "宣言を読めません: {e}"),
            DeclarationError::Empty => write!(f, "[[path]] が 1 つもありません"),
            DeclarationError::BadGlob { glob, reason } => {
                write!(f, "glob `{glob}` を使えません: {reason}")
            }
        }
    }
}

impl std::error::Error for DeclarationError {}

/// 宣言の 1 行(コンパイル済みの glob と層)。
#[derive(Debug, Clone)]
pub struct Entry {
    pub glob: String,
    pub layer: Layer,
    matcher: GlobMatcher,
}

impl Entry {
    /// この glob が `rel`(リポジトリのルートからの相対パス)に当たるか。
    pub fn matches(&self, rel: &str) -> bool {
        self.matcher.is_match(rel)
    }
}

/// 読み込み済みの宣言。
#[derive(Debug, Clone)]
pub struct Declaration {
    entries: Vec<Entry>,
}

/// パスの層の引き結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// どの glob にも当たらない(宣言の漏れ)。
    Undeclared,
    /// ちょうど 1 つの glob に当たる。
    One(Layer),
    /// 2 つ以上の glob に当たる(宣言の重なり)。当たった glob を宣言順に持つ。
    Overlap(Vec<String>),
}

impl Declaration {
    /// 宣言の TOML を読む。
    pub fn parse(text: &str) -> Result<Declaration, DeclarationError> {
        let raw: RawDeclaration =
            toml::from_str(text).map_err(|e| DeclarationError::Syntax(e.to_string()))?;
        if raw.path.is_empty() {
            return Err(DeclarationError::Empty);
        }
        let mut entries = Vec::with_capacity(raw.path.len());
        for e in raw.path {
            entries.push(compile(e)?);
        }
        Ok(Declaration { entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// リポジトリのルートからの相対パス(`/` 区切り)の層を引く。
    pub fn lookup(&self, rel: &str) -> Lookup {
        let hits: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.matcher.is_match(rel))
            .collect();
        match hits.as_slice() {
            [] => Lookup::Undeclared,
            [one] => Lookup::One(one.layer),
            many => Lookup::Overlap(many.iter().map(|e| e.glob.clone()).collect()),
        }
    }
}

fn compile(e: RawEntry) -> Result<Entry, DeclarationError> {
    let bad = |reason: &str| DeclarationError::BadGlob {
        glob: e.glob.clone(),
        reason: reason.to_string(),
    };
    if e.glob.is_empty() {
        return Err(bad("空です"));
    }
    if e.glob.starts_with('/') || e.glob.starts_with("./") {
        return Err(bad(
            "リポジトリのルートからの相対パスで書く(`/` や `./` で始めない)",
        ));
    }
    if e.glob.split('/').any(|c| c == "..") {
        return Err(bad("`..` を含められない"));
    }
    // `*` は `/` を越えない。ディレクトリの下は `dir/**` と明示する。
    let glob: Glob = GlobBuilder::new(&e.glob)
        .literal_separator(true)
        .build()
        .map_err(|err| bad(&err.to_string()))?;
    Ok(Entry {
        matcher: glob.compile_matcher(),
        glob: e.glob,
        layer: e.layer,
    })
}

/// 宣言の欠陥 1 件(追跡されているファイルに対して引いた結果)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Defect {
    /// どの glob にも当たらないファイル。
    Uncovered(String),
    /// 2 つ以上の glob に当たるファイル(当たった glob を持つ)。
    Overlapping(String, Vec<String>),
}

/// 追跡されているファイル(`git ls-files`)の各々に、当たる glob がちょうど 1 つあるかを検査する。
/// 重なりと漏れは、glob 同士の包含を調べず、実在のファイルへの当たり方に落として見つける。
pub fn check_files<'a>(
    decl: &Declaration,
    files: impl IntoIterator<Item = &'a str>,
) -> Vec<Defect> {
    let mut out = Vec::new();
    for f in files {
        match decl.lookup(f) {
            Lookup::One(_) => {}
            Lookup::Undeclared => out.push(Defect::Uncovered(f.to_string())),
            Lookup::Overlap(globs) => out.push(Defect::Overlapping(f.to_string(), globs)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(text: &str) -> Declaration {
        Declaration::parse(text).unwrap()
    }

    const BASIC: &str = r#"
[[path]]
glob = "docs/adr/**"
layer = "L0"

[[path]]
glob = "docs/*.md"
layer = "L1"

[[path]]
glob = "state/**"
layer = "L3"
"#;

    #[test]
    fn a_path_is_resolved_to_exactly_one_layer() {
        let d = decl(BASIC);
        assert_eq!(d.lookup("docs/adr/001-x.md"), Lookup::One(Layer::L0));
        assert_eq!(d.lookup("docs/README.md"), Lookup::One(Layer::L1));
        assert_eq!(d.lookup("state/a/b.json"), Lookup::One(Layer::L3));
    }

    #[test]
    fn a_star_does_not_cross_a_directory_boundary() {
        let d = decl(BASIC);
        assert_eq!(d.lookup("docs/sub/x.md"), Lookup::Undeclared);
    }

    #[test]
    fn an_unmatched_path_is_undeclared() {
        assert_eq!(decl(BASIC).lookup("src/main.rs"), Lookup::Undeclared);
    }

    #[test]
    fn overlapping_globs_are_reported_with_both_globs() {
        let d = decl(
            r#"
[[path]]
glob = "docs/**"
layer = "L1"
[[path]]
glob = "docs/adr/**"
layer = "L0"
"#,
        );
        assert_eq!(
            d.lookup("docs/adr/1.md"),
            Lookup::Overlap(vec!["docs/**".into(), "docs/adr/**".into()])
        );
        assert_eq!(d.lookup("docs/other.md"), Lookup::One(Layer::L1));
    }

    #[test]
    fn dotfiles_are_matched_by_a_star() {
        let d = decl("[[path]]\nglob = \".config/*\"\nlayer = \"L1\"\n");
        assert_eq!(
            d.lookup(".config/record-layers.toml"),
            Lookup::One(Layer::L1)
        );
    }

    #[test]
    fn the_layer_vocabulary_is_closed() {
        let err = Declaration::parse("[[path]]\nglob = \"a\"\nlayer = \"L5\"\n").unwrap_err();
        assert!(matches!(err, DeclarationError::Syntax(_)), "{err}");
        let err = Declaration::parse("[[path]]\nglob = \"a\"\nlayer = \"l1\"\n").unwrap_err();
        assert!(matches!(err, DeclarationError::Syntax(_)), "{err}");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = Declaration::parse("[[path]]\nglob = \"a\"\nlayer = \"L1\"\nnote = \"x\"\n")
            .unwrap_err();
        assert!(matches!(err, DeclarationError::Syntax(_)), "{err}");
        let err = Declaration::parse("[meta]\nx = 1\n").unwrap_err();
        assert!(matches!(err, DeclarationError::Syntax(_)), "{err}");
    }

    #[test]
    fn an_empty_declaration_is_broken() {
        assert_eq!(Declaration::parse("").unwrap_err(), DeclarationError::Empty);
    }

    #[test]
    fn globs_must_be_repo_relative_and_well_formed() {
        for glob in ["/abs/**", "./a", "a/../b", "", "a/[b"] {
            let text = format!("[[path]]\nglob = \"{glob}\"\nlayer = \"L1\"\n");
            let err = Declaration::parse(&text).unwrap_err();
            assert!(
                matches!(err, DeclarationError::BadGlob { .. }),
                "{glob:?} -> {err}"
            );
        }
    }

    #[test]
    fn check_files_finds_uncovered_and_overlapping_files() {
        let d = decl(
            r#"
[[path]]
glob = "a/**"
layer = "L1"
[[path]]
glob = "a/b/**"
layer = "L0"
"#,
        );
        let defects = check_files(&d, ["a/x", "a/b/y", "c/z"]);
        assert_eq!(
            defects,
            vec![
                Defect::Overlapping("a/b/y".into(), vec!["a/**".into(), "a/b/**".into()]),
                Defect::Uncovered("c/z".into()),
            ]
        );
    }

    #[test]
    fn only_l0_to_l2_belong_in_git() {
        let in_git: Vec<Layer> = Layer::ALL.into_iter().filter(|l| l.in_git()).collect();
        assert_eq!(in_git, vec![Layer::L0, Layer::L1, Layer::L2]);
    }
}
