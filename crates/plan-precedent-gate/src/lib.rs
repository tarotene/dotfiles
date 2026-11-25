//! plan-precedent-gate — ExitPlanMode 直前に、先行例との対比(config/claude/
//! CLAUDE.md の「発明する前に先行例を確認する」節、config/agents/skills/
//! precedent-grounding/SKILL.md)の欠落を機械的に検査する hook
//! (旧 `config/claude/hooks/plan-precedent-gate.sh`、ADR-0024 Stage 4b #412)。
//!
//! 設計と根拠: docs/adr/0012-precedent-grounding-over-prompted-adversarial-
//! review.md、docs/agents/precedent-grounding.md。技術・仕組みの選択に関する
//! `軸:`/重い欄の検査は docs/adr/0035-selection-grounding.md、
//! docs/agents/technology-selection.md を参照。
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
//!   - 計画が画面の面(`.html`・`.css`・`.svelte`・`.astro`・`.tsx`・`.jsx`、`config/web/`、
//!     `page-types.toml`)に触れるように見えるとき、`構造:` の行を要求する(ADR-0000)。
//!     形は `構造: <パターン id> — 軸: <軸=値, …> — 退けた: <パターン id>(<理由>)`、画面を変えない
//!     計画は `構造: 該当なし — <理由>`。`## 先行例との対比` 節があればその中、無ければ計画全体から探す。
//!     **形だけを見る**(パターン id の語彙は `config/web/structures.toml` にあり、この hook は
//!     リポジトリを読まない。内容の妥当性は copilot-plan-review の lens A に残す)。
//!   - allow は決して返さない。問題が無ければ何も決定しない(exit 0)。
//!
//! メッセージ文面はスキル文書(precedent-grounding / technology-selection)と
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
// 画面の面に触れる計画の目印(ADR-0000)。ファイルの拡張子と、デザインの正本のパスだけ。意味を
// 機械で確定できないので、誤検知は `構造: 該当なし — <理由>` の 1 行で抜ける。
static SURFACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.(?:html|css|svelte|astro|tsx|jsx)\b|config/web/|page-types\.toml").unwrap()
});
static STRUCT_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("構造:").unwrap());
static STRUCT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"構造:\s*[a-z][a-z0-9-]*\s*[—–-]\s*軸:\s*\S.*退けた:\s*\S").unwrap()
});
static STRUCT_NA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"構造:\s*該当なし\s*[—–-]\s*\S").unwrap());
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

`軸:` は全 Dn に必須(precedent-grounding スキル §3、technology-selection
スキル参照)。出典は URL のほか #123 / owner/repo#123 / リポジトリ内パス
でも可。先行例から意図的に外れた場合は「差分: 異なる — <理由>」。技術・
仕組みの選択で外部依存の新設・置換・撤去、新しい道具・単位の実装言語の選択、
または戻せないもの(データの損失・外部契約・人手の作業)が生じるときは、
加えて「本命:」「対抗馬:」(揃えて書く)「外した候補:」
「既存手段:」(自前なら却下:/探索: 必須、ADR-543)も書く
(technology-selection スキル参照)。裁定は「コード 0 行の白紙から選ぶなら
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
                "D{id}: 「{}」は書き直しの作業量・歴史的経緯を理由にする語です。裁定は「コード 0 行の白紙から選ぶなら何を採るか」だけで下します。戻せないもの(データの損失・外部契約・人手の作業)があるときだけ「戻せない: データ|外部契約|人手 — <内容>」の行で書いてください(technology-selection スキル)",
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
            missing.push(
                "出典(URL・#N・owner/repo#N・リポジトリ内パスのいずれか。パスは scripts/x.py のようにスラッシュを含む形で、README.md や AGENTS.md のようなスラッシュの無い名前だけでは出典と認められません)",
            );
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

/// 画面の面に触れる計画は、`構造:` の行を持つこと(ADR-0000)。形だけを見る。
fn check_structure(plan: &str, out: &mut Vec<String>) {
    if !SURFACE_RE.is_match(plan) {
        return;
    }
    let section = extract_precedent_section(plan);
    let scope = if section.is_empty() {
        plan.to_string()
    } else {
        section.join("\n")
    };
    if grep(&STRUCT_RE, &scope) || grep(&STRUCT_NA_RE, &scope) {
        return;
    }
    if grep(&STRUCT_LABEL_RE, &scope) {
        out.push("「構造:」の行の形が違います。`構造: <パターン id> — 軸: <軸=値, …> — 退けた: <パターン id>(<理由>)` の 1 行で書くか、画面を変えない計画なら `構造: 該当なし — <理由>` の 1 行にしてください(precedent-grounding スキル §3)".to_string());
    } else {
        out.push("画面の面(HTML・CSS・config/web/ など)に触れる計画に見えますが、「構造:」の行がありません。`## 先行例との対比` の中に、`構造: <パターン id> — 軸: <軸=値, …> — 退けた: <パターン id>(<理由>)`(パターン id と軸の語彙は config/web/structures.toml。ADR-0000)を 1 行で書くか、画面を変えない計画なら `構造: 該当なし — <理由>` の 1 行を書いてください(precedent-grounding スキル §3)".to_string());
    }
}

/// プラン本文を検査し、見つかった問題を 1 行 1 件で返す(無ければ空)。
pub fn judge_precedent(plan: &str) -> Vec<String> {
    let mut out = judge_sections(plan);
    check_structure(plan, &mut out);
    out
}

/// `## 先行例との対比` 節(または免除行)と、各 Dn の形式の検査。
fn judge_sections(plan: &str) -> Vec<String> {
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

/// `$(cat …)` と同じく末尾の改行を落とす。
pub fn trim_newlines(mut s: String) -> String {
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

/// 例文に埋める取得日(`YYYY-MM-DD`)。テストは `PLAN_PRECEDENT_GATE_TODAY` で
/// 固定する — ゴールデンに実行日が入り込む経路そのものを無くすため(#752、
/// due-remind の `DUE_REMIND_TODAY` と同形)。本番は外部 `date` に委ねる。
pub fn today() -> String {
    std::env::var("PLAN_PRECEDENT_GATE_TODAY")
        .ok()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| process_control::date("%Y-%m-%d"))
}

fn gate_dir() -> std::path::PathBuf {
    std::env::var_os("CLAUDE_PLAN_PRECEDENT_GATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".claude/plan-precedent-gate")
        })
}

/// hook 本体(`main` が標準入力を読んだあとの処理)。`main` と、ルータ(`crates/hook-router`)が
/// 同じ関数を呼ぶ。返すのは deny だけ(allow は決して返さない)。
///
/// 判定はプランの本文への正規表現だけで、`gh` もネットワークも呼ばない。プロセス全体の状態は変えない
/// (作業ディレクトリへは移らず、`HookInput::effective_cwd` を相対パスの解決にだけ使う)。外の読み取りは、
/// プランのファイルと、例文の取得日のための `date` だけ。
pub fn hook(raw: &str) -> Option<agent_session::Outcome> {
    if agent_session::escape::PLAN_PRECEDENT_GATE
        .active(Some(&gate_dir()), None)
        .is_some()
    {
        return None;
    }
    // 不正 JSON でも bash 版(jq が空を返す)と同じく最新プランへ縮退する。
    let input = agent_session::HookInput::parse(raw).unwrap_or_default();
    let cwd = input.effective_cwd();
    let plan = agent_session::plan::plan_text_at(&input, cwd.as_deref())?;
    let problems = judge_precedent(&trim_newlines(plan));
    if problems.is_empty() {
        return None;
    }
    agent_session::Outcome::deny(deny_message(&problems, &today()))
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
            "既存手段: home/lib/nixgl.nix — 採用: nixpkgs musescore + 既存 nixGLWrap(自前 derivation は書かない)",
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
    fn a_slashless_file_name_is_not_a_citation_and_the_message_says_so() {
        // F43: `README.md` のようなスラッシュの無い名前だけの出典は、認められない。拒否文に、通る形(スラッシュを含むパス)を書く。
        let p = "## 先行例との対比\n- D1: x\n  先行例: このリポジトリの README.md (取得 2026-11-20)\n  差分: 一致\n  軸: 還元 — 1句\n";
        let m = judge_precedent(p);
        assert!(
            m.iter()
                .any(|x| x.contains("scripts/x.py") && x.contains("スラッシュ")),
            "{m:?}"
        );
        // スラッシュを含む形は通る。
        let ok = p.replace("README.md", "docs/README.md");
        assert!(
            judge_precedent(&ok).is_empty(),
            "{:?}",
            judge_precedent(&ok)
        );
    }

    fn screen_plan(structure_line: &str) -> String {
        format!(
            "## 先行例との対比\n- D1: x\n  先行例なし: 探した範囲\n  {structure_line}\n  軸: 還元 — 1句\n\n## 変更\n- `config/web/structures.toml` を足す\n"
        )
    }

    #[test]
    fn a_plan_touching_a_screen_needs_a_structure_line() {
        let m = judge_precedent(&screen_plan("説明: 画面の話"));
        assert!(
            m.iter().any(|x| x.contains("「構造:」の行がありません")),
            "{m:?}"
        );
        // 書けば通る。
        let ok = screen_plan("構造: step — 軸: 独立性=独立, 反復=毎日 — 退けた: single-column(1 件あたりの情報が多い)");
        assert!(
            judge_precedent(&ok).is_empty(),
            "{:?}",
            judge_precedent(&ok)
        );
        // 画面を変えない計画は、免除の 1 行で抜ける(ダッシュ種は 3 つとも)。
        for dash in ["—", "–", "-"] {
            let na = screen_plan(&format!("構造: 該当なし {dash} ドキュメントだけを直す"));
            assert!(
                judge_precedent(&na).is_empty(),
                "{dash}: {:?}",
                judge_precedent(&na)
            );
        }
    }

    #[test]
    fn a_malformed_structure_line_is_reported_as_such() {
        for bad in [
            "構造: step",
            "構造: step — 軸: 独立性=独立",
            "構造: Step — 軸: x — 退けた: y",
            "構造: 該当なし",
        ] {
            let m = judge_precedent(&screen_plan(bad));
            assert!(
                m.iter().any(|x| x.contains("「構造:」の行の形が違います")),
                "{bad}: {m:?}"
            );
        }
    }

    #[test]
    fn surfaces_are_recognised_by_extension_and_the_web_directory() {
        for hit in [
            "template.html",
            "page.css",
            "App.svelte",
            "x.astro",
            "view.tsx",
            "view.jsx",
            "config/web/",
            "config/web/page-types.toml",
            "page-types.toml",
        ] {
            let plan = format!("先行例: 該当なし — 修正\n- {hit} を直す\n");
            assert!(
                judge_precedent(&plan)
                    .iter()
                    .any(|x| x.contains("「構造:」の行がありません")),
                "{hit}"
            );
        }
        for miss in [
            "README.md",
            "crates/x/src/lib.rs",
            "docs/adr/1.md",
            "csshelper",
            "https://example.com/doc.htmlx",
        ] {
            let plan = format!("先行例: 該当なし — 修正\n- {miss} を直す\n");
            assert!(
                judge_precedent(&plan).is_empty(),
                "{miss}: {:?}",
                judge_precedent(&plan)
            );
        }
    }

    #[test]
    fn without_a_section_the_whole_plan_is_searched_for_the_line() {
        let plan = "先行例: 該当なし — 修正\n構造: 該当なし — 画面を変えない\n- template.html のコメントだけ直す\n";
        assert!(
            judge_precedent(plan).is_empty(),
            "{:?}",
            judge_precedent(plan)
        );
        // 節があるのに、節の外にだけ書いても通らない。
        let outside = "## 先行例との対比\n- D1: x\n  先行例なし: y\n  軸: 還元 — 1句\n\n## 他\n構造: 該当なし — 理由\n- template.html を直す\n";
        assert!(
            judge_precedent(outside)
                .iter()
                .any(|x| x.contains("「構造:」の行がありません")),
            "{:?}",
            judge_precedent(outside)
        );
    }

    #[test]
    fn a_plan_that_does_not_touch_a_screen_is_not_asked_for_a_structure() {
        let plan = "## 先行例との対比\n- D1: x\n  先行例なし: y\n  軸: 還元 — 1句\n";
        assert!(judge_precedent(plan).is_empty());
    }

    #[test]
    fn english_sunk_cost_in_a_citation_is_not_flagged() {
        let p = "## 先行例との対比\n- D1: x\n  先行例: Arkes & Blumer, The psychology of sunk cost, https://doi.org/10.1016/0749-5978(85)90049-4 (取得 2026-10-08)\n  差分: 一致\n  軸: 還元 — 1句\n";
        assert!(judge_precedent(p).is_empty(), "{:?}", judge_precedent(p));
    }
}
