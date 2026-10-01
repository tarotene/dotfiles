//! plan-precedent-gate — ExitPlanMode 直前に、先行例との対比(config/claude/
//! CLAUDE.md の「発明する前に先行例を確認する」節、config/claude/skills/
//! precedent-grounding/SKILL.md)の欠落を機械的に検査する hook
//! (旧 `config/claude/hooks/plan-precedent-gate.sh`、ADR-0024 Stage 4b #412)。
//!
//! 設計と根拠: docs/adr/0012-precedent-grounding-over-prompted-adversarial-
//! review.md、docs/claude/precedent-grounding.md。技術・仕組みの選択に関する
//! `軸:`/重い欄の検査は docs/adr/0035-selection-grounding.md、
//! docs/claude/selection-grounding.md を参照。
//!
//! 形式(節または免除行が存在するか、各 Dn に必要な要素があるか)だけを
//! LLM を呼ばずに検査する。内容(引用が本当に主張を支えているか)は
//! copilot-plan-review の lens A に残す。
//!
//!   - `## 先行例との対比` 節が無ければ、`先行例: 該当なし — <理由>` の免除行
//!     (ダッシュ種は — / – / - のいずれでも可)があるかを見る。どちらも無ければ deny。
//!   - 節がある場合、`- Dn:` 行が1件以上あるか・重複が無いかを見る。各 Dn は
//!     `先行例なし: <非空>`、または `先行例: <出典>` + `(取得 YYYY-MM-DD)` +
//!     `差分:(一致|異なる)` を満たすこと。
//!   - 各 Dn には `軸:(表現不可能|還元|検出のみ)` が必須。
//!   - `本命:`/`対抗馬:`/`外した候補:`(重い欄)のいずれかがあれば、`本命:` と
//!     `対抗馬:` が揃っていること、`既存手段:`(ADR-543)があること、`自前` なら
//!     `却下:`/`探索:` の理由があること。
//!   - allow は決して返さない。問題が無ければ何も決定しない(exit 0)。
//!
//! メッセージ文面はスキル文書(precedent-grounding / selection-grounding)と
//! 一致している必要があるため、bash 版の出力とバイト一致を
//! `tests/cmd/*.toml` で固定している。
//!
//! 正規表現の `[[:space:]]` は Unicode 空白(`\s`)で写した。bash 版は
//! grep/`[[ =~ ]]` を UTF-8 ロケールで実行したときの iswspace に依存していた。

use regex::Regex;
use std::sync::LazyLock;

static EXEMPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"先行例:\s*該当なし\s*[—–-]\s*\S").unwrap());
static DN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[-*]?\s*\*{0,2}D([0-9]+)\*{0,2}[:.)]").unwrap());
static CITATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(https?://[^\s)]+|#[0-9]+|[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+)").unwrap()
});
static DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"取得\s*[0-9]{4}-[0-9]{2}-[0-9]{2}").unwrap());
static DIFF_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"差分:\s*(一致|異なる)").unwrap());
static NONE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"先行例なし:\s*\S").unwrap());
static CITE_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("先行例:").unwrap());
static AXIS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"軸:\s*(表現不可能|還元|検出のみ)").unwrap());
static HONMEI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"本命:\s*\S").unwrap());
static TAIKOUBA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"対抗馬:\s*\S").unwrap());
static HEAVY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"本命:\s*\S|対抗馬:\s*\S|外した候補:\s*\S").unwrap());
static KIZON_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"既存手段:\s*\S").unwrap());
static KIZON_JIMAE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"既存手段:\s*.*自前").unwrap());
static KIZON_REASON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(却下|探索):\s*\S").unwrap());
static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#+\s*先行例との対比").unwrap());
static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#+\s").unwrap());

/// `grep -Eq` 相当: どこかの行が一致するか(grep は行単位で照合する)。
fn grep(re: &Regex, text: &str) -> bool {
    text.lines().any(|l| re.is_match(l))
}

/// そのまま貼れば書式検査を通る完全な例文ブロック(末尾改行なし)。取得日は
/// 実行時の日付(`date +%Y-%m-%d`)を埋める — 固定プレースホルダだと DATE_RE に
/// 一致せず、丸写しした瞬間に再 deny してしまうため。
pub fn example_block(today: &str) -> String {
    format!(
        "そのまま構造を写し、山括弧の中身だけ事実に置き換えれば書式検査は通ります:

## 先行例との対比

- D1: <採った設計判断を1文で>
  先行例: <著者/組織, タイトル> https://example.com/doc (取得 {today})
  差分: 一致
  軸: 表現不可能 | 還元 | 検出のみ — <1句>
- D2: <採った設計判断を1文で>
  先行例なし: <どこを・何のキーワードで・一次/二次のどちらまで探したか>
  軸: 表現不可能 | 還元 | 検出のみ — <1句>
- D3: <外部依存の新設・置換・撤去を伴う設計判断を1文で>
  本命: 憧れ駆動 | なし — <理由>
  対抗馬: <候補> (<本命と共通の評価軸>)
  既存手段: <path> — 採用: <ツール名/URL> | 拡張: <既存パス> | 自前 — 却下: <候補> (<理由>) | 探索: <どこを・何のキーワードで>
  先行例: <著者/組織, タイトル> https://example.com/doc (取得 {today})
  差分: 一致
  軸: 表現不可能 | 還元 | 検出のみ — <1句>

`軸:` は全 Dn に必須(precedent-grounding スキル §3、selection-grounding
スキル参照)。出典は URL のほか #123 / owner/repo#123 / リポジトリ内パス
でも可。先行例から意図的に外れた場合は「差分: 異なる — <理由>」。技術・
仕組みの選択で外部依存の新設・置換・撤去、または撤収コストが導入コストを
上回るときは、加えて「本命:」「対抗馬:」(揃えて書く)「外した候補:」
「既存手段:」(自前なら却下:/探索: 必須、ADR-543)も書く
(selection-grounding スキル参照)。設計判断を含まないプランなら、
節の代わりに次の1行だけ:

先行例: 該当なし — <理由(例: typo 修正で設計判断を含まない)>"
    )
}

/// `## 先行例との対比` 節(見出し行込み、次の見出しの手前まで)。無ければ空。
pub fn extract_precedent_section(plan: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut insec = false;
    for line in plan.lines() {
        if SECTION_RE.is_match(line) {
            insec = true;
            out.push(line);
            continue;
        }
        if insec && HEADING_RE.is_match(line) {
            break;
        }
        if insec {
            out.push(line);
        }
    }
    out
}

/// 1 つの Dn ブロック(Dn 行と後続行)を検査し、問題を 1 行 1 件で返す。
fn check_dn_block(id: &str, block: &str, out: &mut Vec<String>) {
    let mut missing: Vec<&str> = Vec::new();
    if grep(&NONE_RE, block) {
        // 先行例なし: の枝
    } else if grep(&CITE_LABEL_RE, block) {
        if !grep(&CITATION_RE, block) {
            missing.push("出典(URL・#N・owner/repo#N・リポジトリ内パスのいずれか)");
        }
        if !grep(&DATE_RE, block) {
            missing.push("取得日(「(取得 YYYY-MM-DD)」の形)");
        }
        if !grep(&DIFF_RE, block) {
            missing.push("差分:(一致|異なる)");
        }
    } else {
        out.push(format!(
            "D{id} には「先行例:」または「先行例なし:」の記載がありません"
        ));
        return;
    }

    if !grep(&AXIS_RE, block) {
        missing.push("軸:(表現不可能|還元|検出のみ)");
    }
    if !missing.is_empty() {
        out.push(format!(
            "D{id}: 先行例の記載に不足があります — {}",
            missing.join("、")
        ));
    }

    let has_heavy = grep(&HEAVY_RE, block);
    if has_heavy {
        if !grep(&HONMEI_RE, block) {
            out.push(format!(
                "D{id}: 重い欄(本命/対抗馬/外した候補)の一部だけがあります — 本命: が欠落しています"
            ));
        }
        if !grep(&TAIKOUBA_RE, block) {
            out.push(format!(
                "D{id}: 重い欄(本命/対抗馬/外した候補)の一部だけがあります — 対抗馬: が欠落しています"
            ));
        }
        if grep(&KIZON_RE, block) {
            if grep(&KIZON_JIMAE_RE, block) && !grep(&KIZON_REASON_RE, block) {
                out.push(format!(
                    "D{id}: 既存手段: が「自前」なのに却下:/探索: の理由がありません"
                ));
            }
        } else {
            out.push(format!(
                "D{id}: 既存手段:(採用|拡張|自前)の記載がありません(ADR-543)"
            ));
        }
    }
}

/// プラン本文を検査し、見つかった問題を 1 行 1 件で返す(無ければ空)。
pub fn judge_precedent(plan: &str) -> Vec<String> {
    let section = extract_precedent_section(plan);
    let mut out = Vec::new();
    if section.is_empty() {
        if !grep(&EXEMPT_RE, plan) {
            out.push("`## 先行例との対比` 節が見つかりません。非自明な設計判断ごとに Dn 行で先行例と対比するか、設計判断が無いなら `先行例: 該当なし — <理由>` の1行を書いてください(precedent-grounding スキル参照)。".to_string());
        }
        return out;
    }

    // bash 版の連想配列と同じ挙動: 重複した Dn は警告を出したうえで、ブロックを
    // 後の出現で置き換える(検査順は初出順)。
    let mut ids: Vec<String> = Vec::new();
    let mut blocks: std::collections::HashMap<String, String> = Default::default();
    let mut current: Option<String> = None;
    for line in &section {
        if let Some(c) = DN_RE.captures(line) {
            let id = c[1].to_string();
            if blocks.contains_key(&id) {
                out.push(format!("D{id} が複数回出現しています(重複)"));
            } else {
                ids.push(id.clone());
            }
            blocks.insert(id.clone(), (*line).to_string());
            current = Some(id);
        } else if let Some(id) = &current {
            let b = blocks.get_mut(id).expect("current id has a block");
            b.push('\n');
            b.push_str(line);
        }
    }

    if ids.is_empty() {
        out.push(
            "`## 先行例との対比` 節に `- D1:` のような設計判断の行が1件もありません。".to_string(),
        );
        return out;
    }
    for id in &ids {
        check_dn_block(id, &blocks[id], &mut out);
    }
    out
}

/// hook モードの deny 理由(見つかった問題 + 例文ブロック)。
pub fn deny_message(problems: &[String], today: &str) -> String {
    format!(
        "先行例との対比の検査で問題が見つかりました。precedent-grounding スキルの手順に従って計画を修正してください。\n\n{}\n\n{}",
        problems.join("\n"),
        example_block(today)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_block_passes_itself() {
        assert!(judge_precedent(&example_block("2026-10-01")).is_empty());
    }

    #[test]
    fn section_stops_at_next_heading() {
        let s = extract_precedent_section("前文\n## 先行例との対比\n- D1: x\n## 次の節\n無関係\n");
        assert_eq!(s, vec!["## 先行例との対比", "- D1: x"]);
    }

    #[test]
    fn duplicate_uses_last_block() {
        let p = "## 先行例との対比\n- D1: a\n  先行例なし: x\n  軸: 還元\n- D1: b\n";
        assert_eq!(
            judge_precedent(p),
            vec![
                "D1 が複数回出現しています(重複)".to_string(),
                "D1 には「先行例:」または「先行例なし:」の記載がありません".to_string()
            ]
        );
    }
}
