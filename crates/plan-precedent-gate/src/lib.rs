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
//!   - コストを理由にした語(書き直しコスト・移行コスト・撤収コスト・battle-tested
//!     等、閉じた語彙)が Dn ブロックにあれば deny(新 ADR sunk-cost-exclusion、
//!     バッククォートで囲んだ語は数えない)。コストを理由にしてよいのは
//!     `戻せない:(データ|外部契約|人手) — <内容>` の行だけ。
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
// 3 値の選択としての「自前」だけに一致させる(#714)。書式は
// `既存手段: <path> — 採用: … | 拡張: … | 自前 — 却下: …` で、選択は `— ` の直後に
// 来る。採用側の説明文に含まれる語(例: 「自前 derivation は書かない」)は選択ではない。
static KIZON_JIMAE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"既存手段:\s*(?:[^—\n]*—\s*)?自前(?:\s*—|\s*$)").unwrap());
static KIZON_REASON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(却下|探索):\s*\S").unwrap());
static MODOSENAI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"戻せない:\s*(データ|外部契約|人手)").unwrap());
static MODOSENAI_ANY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"戻せない:\s*\S").unwrap());
// 技術選定の裁定に持ち込まない、作業量・歴史的経緯を理由にする語の閉語彙。
// 既存の裁定(ADR-0035 D4 / 543 / 568 / 625 / 0033 / 0002)が実際に使った表現から採った。
// 英語の語は入れない(先行例の書誌に含まれる語、例えば論文名の `sunk cost`、を誤検知するため)。
static COST_WORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"書き直しコスト|書き直す手間|移行コスト|移行の手間|撤収コスト|波及コスト|乗り換えコスト|battle-tested|歴史的経緯",
    )
    .unwrap()
});
static BACKTICK_SPAN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]*`").unwrap());
// 節名は 3 軸で同点のため据え置く(ADR-0035 D4。2026-10-08 に白紙から裁定し直した、#750)。
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
  戻せない: データ | 外部契約 | 人手 — <内容。戻せないものが無ければこの行ごと省く>
  軸: 表現不可能 | 還元 | 検出のみ — <1句>

`軸:` は全 Dn に必須(precedent-grounding スキル §3、selection-grounding
スキル参照)。出典は URL のほか #123 / owner/repo#123 / リポジトリ内パス
でも可。先行例から意図的に外れた場合は「差分: 異なる — <理由>」。技術・
仕組みの選択で外部依存の新設・置換・撤去、新しい道具・単位の実装言語の選択、
または戻せないもの(データの損失・外部契約・人手の作業)が生じるときは、
加えて「本命:」「対抗馬:」(揃えて書く)「外した候補:」
「既存手段:」(自前なら却下:/探索: 必須、ADR-543)も書く
(selection-grounding スキル参照)。裁定は「コード 0 行の白紙から選ぶなら
何を採るか」だけで下し、書き直しの作業量と過去の経緯は理由にしない。
戻せないものがあるときだけ「戻せない: データ | 外部契約 | 人手 — <内容>」の
1 行で書く(これ以外の書き方でコストを理由にすると deny)。設計判断を含まない
プランなら、
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

/// 作業量・歴史的経緯を理由にした語(閉語彙)を Dn ブロックから拾う。コストを
/// 理由にしてよいのは `戻せない:(データ|外部契約|人手)` の行だけ。バッククォートで
/// 囲んだ語は引用なので数えない。`戻せない:` の値が閉語彙の外なら、それも指摘する。
fn check_cost_reasons(id: &str, block: &str, out: &mut Vec<String>) {
    for line in block.lines() {
        if MODOSENAI_RE.is_match(line) {
            continue;
        }
        if MODOSENAI_ANY_RE.is_match(line) {
            out.push(format!(
                "D{id}: 「戻せない:」の値は データ|外部契約|人手 のいずれかで始めてください"
            ));
            continue;
        }
        let stripped = BACKTICK_SPAN_RE.replace_all(line, "");
        if let Some(m) = COST_WORD_RE.find(&stripped) {
            out.push(format!(
                "D{id}: 「{}」は書き直しの作業量・歴史的経緯を理由にする語です。裁定は「コード 0 行の白紙から選ぶなら何を採るか」だけで下します。戻せないもの(データの損失・外部契約・人手の作業)があるときだけ「戻せない: データ|外部契約|人手 — <内容>」の行で書いてください(selection-grounding スキル)",
                m.as_str()
            ));
        }
    }
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

    check_cost_reasons(id, block, out);

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

    fn plan_with(kizon: &str) -> String {
        format!(
            "## 先行例との対比\n- D1: x\n  本命: なし — 比較した\n  対抗馬: y (同じ軸)\n  {kizon}\n  先行例なし: 探した範囲\n  軸: 還元 — 1句\n"
        )
    }

    #[test]
    fn jimae_word_inside_adopt_description_is_not_a_choice() {
        let p = plan_with(
            "既存手段: home/modules/nixgl.nix — 採用: nixpkgs musescore + 既存 nixGLWrap(自前 derivation は書かない)",
        );
        assert!(judge_precedent(&p).is_empty(), "{:?}", judge_precedent(&p));
    }

    #[test]
    fn jimae_choice_without_reason_still_fails() {
        let p = plan_with("既存手段: home/modules/x.nix — 自前");
        assert!(judge_precedent(&p)
            .iter()
            .any(|m| m.contains("「自前」なのに却下:/探索: の理由がありません")));
        let p = plan_with("既存手段: 自前 — 理由なし");
        assert!(judge_precedent(&p)
            .iter()
            .any(|m| m.contains("「自前」なのに却下:/探索: の理由がありません")));
    }

    #[test]
    fn jimae_choice_with_reason_passes() {
        let p = plan_with("既存手段: home/modules/x.nix — 自前 — 却下: foo (重い)");
        assert!(judge_precedent(&p).is_empty(), "{:?}", judge_precedent(&p));
    }

    fn plan_with_line(line: &str) -> String {
        format!(
            "## 先行例との対比\n- D1: x\n  先行例なし: 探した範囲\n  {line}\n  軸: 還元 — 1句\n"
        )
    }

    #[test]
    fn cost_word_is_denied() {
        let p = plan_with_line("差分: 移行コストが高いので現状を維持する");
        assert!(judge_precedent(&p)
            .iter()
            .any(|m| m.contains("「移行コスト」") && m.contains("戻せない:")));
    }

    #[test]
    fn cost_word_inside_backticks_is_a_quotation() {
        let p = plan_with_line("説明: 既存の `撤収コスト` という語を取り除く");
        assert!(judge_precedent(&p).is_empty(), "{:?}", judge_precedent(&p));
    }

    #[test]
    fn modosenai_line_may_mention_cost() {
        let p = plan_with_line("戻せない: データ — 移行コストではなく mozc の学習履歴が失われる");
        assert!(judge_precedent(&p).is_empty(), "{:?}", judge_precedent(&p));
    }

    #[test]
    fn modosenai_with_open_value_is_denied() {
        let p = plan_with_line("戻せない: 手間がかかる");
        assert!(judge_precedent(&p)
            .iter()
            .any(|m| m.contains("「戻せない:」の値は")));
    }

    #[test]
    fn english_sunk_cost_in_a_citation_is_not_flagged() {
        let p = "## 先行例との対比\n- D1: x\n  先行例: Arkes & Blumer, The psychology of sunk cost, https://doi.org/10.1016/0749-5978(85)90049-4 (取得 2026-10-08)\n  差分: 一致\n  軸: 還元 — 1句\n";
        assert!(judge_precedent(p).is_empty(), "{:?}", judge_precedent(p));
    }
}
