//! naming ドメイン(ADR-0014 + ADR-0020 閉語彙 + ADR-0026 coined 分割・
//! lifecycle 軸)。
//!
//! どのクラスを宣言すべきかはこの監査では決めない(人間の判断、
//! github-audit-triage の blind re-derivation で提示する)。

use crate::model::{Detail, Finding, Verdict};
use regex::Regex;
use std::sync::OnceLock;

/// ADR-0020: この時刻以前に作られたリポジトリは ADR-0014 の緩い字面パターン
/// だけで判定する(ADR-0007 の「遡って改名しない」と同じ考え方)。codename
/// registry の包含検査はこの cutoff に関係なく全リポジトリに掛かる。
pub const NAMING_STRICT_CUTOFF: &str = "2026-09-19T23:59:59Z";

/// ADR-0020 の閉語彙と ADR-0026 の lifecycle 語彙(実行全体で 1 回読む)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NamingVocab {
    /// descriptive-species.tsv
    pub species: Vec<String>,
    /// codename-registry.tsv + .local.tsv
    pub registry: Vec<String>,
    /// site-domains.tsv + .local.tsv
    pub site_domains: Vec<String>,
    /// lifecycle-species.tsv(並びは `sort -u` 済みの順で、最初に一致した語を採る)
    pub lifecycle_species: Vec<String>,
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("valid regex"))
}

fn pattern_for(class: &str) -> Option<&'static Regex> {
    static PJ: OnceLock<Regex> = OnceLock::new();
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    static DESC: OnceLock<Regex> = OnceLock::new();
    static SITE: OnceLock<Regex> = OnceLock::new();
    Some(match class {
        "naming-pj" => re(&PJ, r"^pj-[a-z0-9]+(-[a-z0-9]+)*$"),
        // ADR-0026: naming-coined は naming-codename と同じ字面(単一トークン)
        // だが閉語彙検査は無い — 意味を持つ著者固有の造語は固定キャストから
        // 採番する性質のものではない。既存 naming-codename のどれが実は coined
        // かの再分類はここでは判定できない(機械的に区別する語彙が無いことが
        // そもそもの分割理由)— triage の人間裁定に委ねる(#278)。
        "naming-codename" | "naming-coined" => re(&TOKEN, r"^[a-z0-9]+$"),
        "naming-descriptive" => re(&DESC, r"^[a-z0-9]+(-[a-z0-9]+)+$"),
        "naming-site" => re(&SITE, r"^[a-z0-9]+(\.[a-z0-9]+)+$"),
        _ => return None,
    })
}

/// bash: `judge_naming`。`topics` は `repositoryTopics` の名前、`created_at` は
/// ISO 8601(空なら cutoff 前扱い)、`description` は lifecycle-study 候補の
/// 照合に使う。
pub fn judge_naming(
    repo: &str,
    topics: &[String],
    created_at: &str,
    vocab: &NamingVocab,
    description: &str,
    is_archived: bool,
) -> Finding {
    let naming_topics: Vec<&String> = topics.iter().filter(|t| t.starts_with("naming-")).collect();
    let mut missing: Vec<String> = Vec::new();
    let strict = created_at > NAMING_STRICT_CUTOFF;

    let mut class = String::new();
    match naming_topics.len() {
        0 => missing.push("class-undeclared".into()),
        1 => {
            class = naming_topics[0].clone();
            match class.as_str() {
                "naming-codename" => {
                    if !vocab.registry.iter().any(|r| r == repo) {
                        missing.push("codename-not-registered".into());
                    }
                }
                "naming-descriptive" => {
                    if strict {
                        let species = repo.rsplit('-').next().unwrap_or(repo);
                        if !vocab.species.iter().any(|s| s == species) {
                            missing.push(format!("species-unrecognized:{species}"));
                        }
                    }
                }
                "naming-site" => {
                    if strict && !vocab.site_domains.iter().any(|d| d == repo) {
                        missing.push("domain-unrecognized".into());
                    }
                }
                "naming-pj" | "naming-coined" => {}
                other => missing.push(format!("class-unknown:{other}")),
            }
            if let Some(p) = pattern_for(&class) {
                if !p.is_match(repo) {
                    missing.push(format!("pattern-mismatch:{class}"));
                }
            }
        }
        _ => missing.push("class-ambiguous".into()),
    }

    // ADR-0026 §2: lifecycle 軸は naming-* クラスと直交。候補の提示だけで
    // verdict/missing には影響しない(rulesets の review_layer と同じ形の
    // 情報フィールド)。
    let has_topic = |t: &str| topics.iter().any(|x| x == t);
    let has_timeboxed = has_topic("lifecycle-timeboxed");
    let has_study = has_topic("lifecycle-study");
    let mut lifecycle_candidates: Vec<String> = Vec::new();
    // archived か否かを問わない(ADR-0026 は進行中と完了済みの両方を含む)。
    if !has_study && !description.is_empty() {
        // `tr '[:upper:]' '[:lower:]'` はバイト単位(ASCII のみ)。
        let desc_lower = description.to_ascii_lowercase();
        if let Some(tok) = vocab
            .lifecycle_species
            .iter()
            .find(|tok| desc_lower.contains(tok.as_str()))
        {
            lifecycle_candidates.push(format!("lifecycle-study-candidate:{tok}"));
        }
    }
    // 進行中の naming-pj で topic 未宣言 → 付与の候補。
    if class == "naming-pj" && !is_archived && !has_timeboxed {
        lifecycle_candidates.push("lifecycle-timeboxed-candidate".into());
    }
    // topic を持つが archived → 外す候補(archived は観測できる最も強い完了の信号)。
    if has_timeboxed && is_archived {
        lifecycle_candidates.push("lifecycle-timeboxed-removal-candidate".into());
    }

    Finding {
        verdict: if missing.is_empty() {
            Verdict::Ok
        } else {
            Verdict::Drifted
        },
        missing,
        detail: Detail::Naming {
            class,
            lifecycle_candidates,
        },
    }
}
