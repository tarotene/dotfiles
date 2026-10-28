use super::*;
use crate::Catalog;

/// 手登録だけのカタログ（`fetch-aa` が自動登録を入れる前の `catalog.toml` の写し）。実カタログには
/// 自動登録のモデル・測定値が入っていくので、これらの試験は、この固定の土台と試験用の応答だけを
/// 前提にする（#1188）。手登録の行の取得日・持ち越しなど、土台の中身にも依存している。
const CATALOG_TOML: &str = include_str!("../../tests/fixtures/catalog-hand-registered.toml");

/// 応答の 1 行(AA の形の最小限)。`tb` は Terminal-Bench 4.0 の値で、None なら判定に使うベンチに値が無い。
fn row(id: u32, name: &str, creator: &str, release: &str, tb: Option<f64>) -> String {
    let tb = tb.map_or("null".to_owned(), |v| v.to_string());
    format!(
        r#"{{"id":"00000000-0000-0000-0000-{id:012}","name":"{name}","slug":"{slug}",
        "release_date":"{release}","model_creator":{{"name":"{creator}"}},
        "median_output_tokens_per_second":50.0,"median_time_to_first_token_seconds":2.0,
        "evaluations":{{"terminalbench_v4_0":{tb},"hle":null,"unknown_bench":0.9}}}}"#,
        slug = crate::aa::slugify(name)
    )
}

fn resp(rows: &[String]) -> String {
    format!(
        r#"{{"status":200,"prompt_options":{{"parallel_queries":1,"prompt_length":1000}},"data":[{}]}}"#,
        rows.join(",")
    )
}

/// 手登録の Claude Fable 5.1(low)の行。カタログの `aa_ids` と同じ ID。
const FABLE_LOW: &str = "05776db7-f5c0-40f7-b824-079160f8cfa8";

fn fable_low() -> String {
    row(
        0,
        "Claude Fable 5.1 (Low)",
        "Anthropic",
        "2026-09-01",
        Some(0.5),
    )
    .replace("00000000-0000-0000-0000-000000000000", FABLE_LOW)
}

fn day1() -> Vec<String> {
    vec![
        fable_low(),
        row(1, "Acme One (High)", "Acme Labs", "2026-09-10", Some(0.4)),
        row(2, "Acme One (Low)", "Acme Labs", "2026-09-10", Some(0.3)),
        row(3, "Acme One", "Acme Labs", "2026-09-10", Some(0.35)),
        row(
            4,
            "Acme Two (Dec '24)",
            "Acme Labs",
            "2026-08-01",
            Some(0.2),
        ),
        row(5, "Zeta 3", "Zeta AI", "2026-07-01", Some(0.1)),
        // 範囲の外(古い)・判定に使うベンチに値が無い行は載せない。
        row(6, "Acme Old", "Acme Labs", "2026-06-30", Some(0.1)),
        row(7, "Acme Blank", "Acme Labs", "2026-09-20", None),
    ]
}

fn run_with(text: &str, rows: &[String], date: &str) -> (Report, String) {
    run(text, &resp(rows), date).unwrap()
}

fn model<'r>(r: &'r Report, id: &str) -> &'r Model {
    r.models.iter().find(|m| m.id == id).unwrap()
}

#[test]
fn effort_is_read_only_from_a_closed_vocabulary_of_parenthesised_leading_words() {
    let cases = [
        ("GPT-X (High)", ("GPT-X", "high")),
        ("GPT-X (Low, Default Fallback)", ("GPT-X", "low")),
        ("Foo (xhigh)", ("Foo", "xhigh")),
        ("Foo (Reasoning)", ("Foo", "reasoning")),
        ("Foo (Non-reasoning)", ("Foo", "non-reasoning")),
        ("Foo (Minimal)", ("Foo", "minimal")),
        ("Foo (Max)", ("Foo", "max")),
        ("Foo (Medium)", ("Foo", "medium")),
        // 語彙に無い括弧は名前の一部で、エフォートは default。
        ("Foo (Dec '24)", ("Foo (Dec '24)", "default")),
        ("Foo (Preview)", ("Foo (Preview)", "default")),
        ("Foo", ("Foo", "default")),
        // 先頭語でなければ読まない。
        (
            "Foo (Adaptive Reasoning, High Effort)",
            ("Foo (Adaptive Reasoning, High Effort)", "default"),
        ),
        // 括弧だけの名前は基本名が空になるので、そのまま。
        ("(High)", ("(High)", "default")),
    ];
    for (name, (base, effort)) in cases {
        assert_eq!(split_effort(name), (base.to_owned(), effort), "{name}");
    }
}

#[test]
fn slugs_are_ascii_lowercase_with_single_dashes() {
    assert_eq!(slugify("GPT-6.1 Sol"), "gpt-6-1-sol");
    assert_eq!(slugify("  Foo  (Bar) "), "foo-bar");
    assert_eq!(slugify("日本語"), "model");
}

#[test]
fn registers_every_provider_model_and_effort_in_range_and_nothing_else() {
    let (r, text) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let c = Catalog::parse(&text).unwrap();
    let ids: Vec<_> = r.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["acme-one", "acme-two-dec-24", "zeta-3"]);
    let one = model(&r, "acme-one");
    assert_eq!(
        one.efforts,
        ["low", "high", "default"],
        "段階の順、default は最後"
    );
    assert_eq!(one.provider, "acme-labs");
    assert_eq!(one.name, "Acme One");
    assert_eq!(model(&r, "acme-two-dec-24").efforts, ["default"]);
    let providers: Vec<_> = r.providers.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(providers, ["acme-labs", "zeta-ai"]);
    assert_eq!(r.providers[0].aa_creator.as_deref(), Some("Acme Labs"));
    assert_eq!(r.added, ["acme-one", "acme-two-dec-24", "zeta-3"]);
    // 範囲の外の行と、値の無い行は載らない。
    assert!(c.model("acme-old").is_none() && c.model("acme-blank").is_none());
    // 自動登録のモデルは価格を持たず、費用の比較には出ない。
    assert!(crate::lookup("acme-one").is_none());
    assert!(!c.pricing().iter().any(|p| p.id.starts_with("acme")));
    // 読み戻したカタログに、測定値が全部ある。
    let m = c.measurement("acme-one", "high").unwrap();
    assert_eq!(
        (m.retrieved.as_str(), m.scores["terminalbench_v4_0"]),
        ("2026-10-28", 0.4)
    );
    assert_eq!(c.measurements.len(), r.measurements.len());
}

#[test]
fn a_second_run_with_the_same_input_changes_nothing() {
    let (r1, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let (r2, t2) = run_with(&t1, &day1(), "2026-10-28");
    assert_eq!(t1, t2, "2 回目の実行で差分が出ない");
    assert!(r2.added.is_empty() && r2.retired.is_empty());
    assert_eq!(r1.models, r2.models);
    // 3 回目も同じ(読み戻した状態からの再生成が不動点になる)。
    let (_, t3) = run_with(&t2, &day1(), "2026-10-28");
    assert_eq!(t2, t3);
}

#[test]
fn hand_registered_models_and_text_are_never_rewritten() {
    let (r, text) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let head = |t: &str| t[..t.find(BEGIN).unwrap()].to_owned();
    assert_eq!(
        head(&text),
        head(CATALOG_TOML),
        "生成ブロックの外は 1 バイトも変わらない"
    );
    // 手登録のモデルの行は、測定値だけが更新される。
    let c = Catalog::parse(&text).unwrap();
    assert_eq!(
        c.measurement("claude-fable-5-1", "low").unwrap().retrieved,
        "2026-10-28"
    );
    // 応答に無い手登録の行は、最後の値を持ち越す(取得日は古いまま)。
    let old = c.measurement("claude-opus-5-5", "high").unwrap();
    assert_eq!(old.retrieved, "2026-10-27");
    assert!(r.missing.iter().any(|m| m.model == "claude-opus-5-5"));
    assert!(r.retire_candidates.contains(&"claude-opus-5-5".to_owned()));
    assert!(!r.retire_candidates.contains(&"claude-fable-5-1".to_owned()));
}

#[test]
fn a_different_row_with_a_hand_registered_name_is_reported_not_imported() {
    let mut rows = day1();
    rows.push(row(
        8,
        "Claude Fable 5.1 (Minimal)",
        "Anthropic",
        "2026-09-02",
        Some(0.1),
    ));
    rows.push(row(
        9,
        "claude  fable 5.1",
        "Anthropic",
        "2026-09-02",
        Some(0.1),
    ));
    // 別の提供元の同名は、手登録(Anthropic)と衝突しない。
    rows.push(row(
        10,
        "Claude Fable 5.1",
        "Imitator",
        "2026-09-02",
        Some(0.1),
    ));
    let (r, text) = run_with(CATALOG_TOML, &rows, "2026-10-28");
    let skipped: Vec<_> = r.skipped.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(skipped, ["Claude Fable 5.1 (Minimal)", "claude  fable 5.1"]);
    assert_eq!(r.skipped[0].manual_model, "Claude Fable 5.1");
    let c = Catalog::parse(&text).unwrap();
    assert!(!c
        .model("claude-fable-5-1")
        .unwrap()
        .efforts
        .contains(&"minimal".to_owned()));
    // 別の提供元の同名は取り込む。ID は手登録と衝突するので提供元の ID が付く。
    assert!(
        c.model("imitator-claude-fable-5-1").is_some(),
        "{:?}",
        r.added
    );
}

#[test]
fn a_colliding_slug_gets_the_provider_id_and_then_a_number() {
    let rows = vec![
        row(
            1,
            "Shared Name (High)",
            "Acme Labs",
            "2026-09-10",
            Some(0.4),
        ),
        row(2, "Shared  Name (Low)", "Zeta AI", "2026-09-10", Some(0.4)),
        row(3, "Shared Name", "Zeta AI", "2026-09-11", Some(0.4)),
    ];
    let (r, _) = run_with(CATALOG_TOML, &rows, "2026-10-28");
    let ids: Vec<_> = r.models.iter().map(|m| m.id.as_str()).collect();
    // 「Shared Name」は提供元ごとに別モデル。同じ基本名が 2 つ目に出たら、提供元の ID を付ける。
    assert_eq!(
        ids,
        [
            "shared-name",
            "zeta-ai-shared-name",
            "zeta-ai-shared-name-2"
        ]
    );
}

#[test]
fn two_rows_for_the_same_name_and_effort_keep_the_newer_and_report_the_conflict() {
    let rows = vec![
        row(1, "Acme One (High)", "Acme Labs", "2026-09-10", Some(0.4)),
        row(2, "Acme One (High)", "Acme Labs", "2026-10-10", Some(0.5)),
    ];
    let (r, text) = run_with(CATALOG_TOML, &rows, "2026-10-28");
    assert_eq!(r.conflicts.len(), 1);
    let cf = &r.conflicts[0];
    assert_eq!(
        (cf.base_name.as_str(), cf.effort.as_str()),
        ("Acme One", "high")
    );
    assert!(cf.kept.contains("000000000002") && cf.dropped[0].contains("000000000001"));
    let c = Catalog::parse(&text).unwrap();
    assert_eq!(
        c.measurement("acme-one", "high").unwrap().scores["terminalbench_v4_0"],
        0.5
    );
    // 同じ公開日なら、ID の大きい方。入力の並びに依らない。
    let mut same = rows.clone();
    same[0] = row(1, "Acme One (High)", "Acme Labs", "2026-10-10", Some(0.4));
    let (a, _) = run_with(CATALOG_TOML, &same, "2026-10-28");
    same.reverse();
    let (b, _) = run_with(CATALOG_TOML, &same, "2026-10-28");
    assert_eq!(a.conflicts, b.conflicts);
}

#[test]
fn the_model_id_follows_the_aa_row_id_even_when_the_name_changes() {
    let (_, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let mut rows = day1();
    rows[1] = row(
        1,
        "Acme One Pro (High)",
        "Acme Labs",
        "2026-09-10",
        Some(0.4),
    );
    rows[2] = row(
        2,
        "Acme One Pro (Low)",
        "Acme Labs",
        "2026-09-10",
        Some(0.3),
    );
    rows[3] = row(3, "Acme One Pro", "Acme Labs", "2026-09-10", Some(0.35));
    let (r, text) = run_with(&t1, &rows, "2026-10-29");
    let ids: Vec<_> = r.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["acme-one", "acme-two-dec-24", "zeta-3"],
        "別モデルにならない"
    );
    assert_eq!(
        model(&r, "acme-one").name,
        "Acme One Pro",
        "表示名は追従する"
    );
    assert!(r.added.is_empty());
    Catalog::parse(&text).unwrap();
}

#[test]
fn a_new_row_with_a_known_name_joins_the_existing_model() {
    let (_, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let mut rows = day1();
    rows.push(row(
        11,
        "Acme One (Max)",
        "Acme Labs",
        "2026-10-20",
        Some(0.6),
    ));
    let (r, _) = run_with(&t1, &rows, "2026-10-29");
    assert_eq!(
        model(&r, "acme-one").efforts,
        ["low", "high", "max", "default"]
    );
    assert!(r.added.is_empty());
}

#[test]
fn an_effort_that_changes_class_does_not_leave_the_same_row_id_twice() {
    let (_, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let mut rows = day1();
    // 同じ行 ID が、default から high へ名前を変える(括弧が付く)。
    rows[3] = row(
        3,
        "Acme One (Medium)",
        "Acme Labs",
        "2026-09-10",
        Some(0.35),
    );
    let (r, text) = run_with(&t1, &rows, "2026-10-29");
    assert_eq!(model(&r, "acme-one").efforts, ["low", "medium", "high"]);
    Catalog::parse(&text).unwrap();
}

#[test]
fn a_model_that_leaves_the_response_is_kept_as_retired_with_its_last_values() {
    let (_, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let rows: Vec<String> = day1().into_iter().filter(|r| !r.contains("Zeta")).collect();
    let (r, t2) = run_with(&t1, &rows, "2026-11-05");
    assert_eq!(r.retired, ["zeta-3"]);
    assert_eq!(model(&r, "zeta-3").status, Status::Retired);
    let c = Catalog::parse(&t2).unwrap();
    assert_eq!(c.model("zeta-3").unwrap().status, Status::Retired);
    let m = c.measurement("zeta-3", "default").unwrap();
    assert_eq!(m.retrieved, "2026-10-28", "取得日は行ごとに古いまま");
    assert_eq!(m.scores["terminalbench_v4_0"], 0.1);
    // 他のモデルは新しい取得日になる。
    assert_eq!(
        c.measurement("acme-one", "high").unwrap().retrieved,
        "2026-11-05"
    );
    // もう一度回しても退役のまま・差分なし。
    let (r3, t3) = run_with(&t2, &rows, "2026-11-05");
    assert!(r3.retired.is_empty());
    assert_eq!(t2, t3);
    // 応答に戻れば active に戻る。
    let (_, t4) = run_with(&t3, &day1(), "2026-11-06");
    let c = Catalog::parse(&t4).unwrap();
    assert_eq!(c.model("zeta-3").unwrap().status, Status::Active);
    assert_eq!(
        c.measurement("zeta-3", "default").unwrap().retrieved,
        "2026-11-06"
    );
}

#[test]
fn a_registered_model_stays_when_it_leaves_the_date_range_or_loses_its_values() {
    let (_, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    // 範囲を後ろへ動かしても、登録済みのモデルは残る。新しい行だけが範囲の影響を受ける。
    let moved = t1.replace(
        "aa_release_since = \"2026-07-01\"",
        "aa_release_since = \"2026-10-01\"",
    );
    let mut rows = day1();
    rows[1] = row(1, "Acme One (High)", "Acme Labs", "2026-09-10", None);
    rows.push(row(12, "Acme Three", "Acme Labs", "2026-09-12", Some(0.1)));
    let (r, _) = run_with(&moved, &rows, "2026-10-29");
    let ids: Vec<_> = r.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["acme-one", "acme-two-dec-24", "zeta-3"],
        "Acme Three は範囲の外"
    );
    assert_eq!(model(&r, "acme-one").status, Status::Active);
}

#[test]
fn without_a_discovery_range_nothing_new_is_registered() {
    let text = CATALOG_TOML.replace("[discovery]\naa_release_since = \"2026-07-01\"\n", "");
    let (r, _) = run_with(&text, &day1(), "2026-10-28");
    assert!(r.models.is_empty() && r.providers.is_empty());
}

#[test]
fn a_hand_registered_model_wins_over_an_auto_registered_one_with_the_same_id() {
    let (_, t1) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    // 人が、自動登録済みと同じ ID のモデルを手登録の部分へ足した。
    let manual =
        "[[model]]\nid = \"acme-one\"\nprovider = \"openai\"\nname = \"Acme One (mine)\"\n\n";
    let edited = t1.replacen(
        "# ---------------------------------------------------------------- discovery",
        &format!(
            "{manual}# ---------------------------------------------------------------- discovery"
        ),
        1,
    );
    assert!(
        Catalog::parse(&edited).is_err(),
        "全体の検査は重複を拒否する"
    );
    // 取り込みは読めて、自動登録側が外れ、同じ ID は手登録だけになる。
    let (r, text) = run_with(&edited, &day1(), "2026-10-29");
    assert!(r.models.iter().all(|m| m.id != "acme-one"));
    assert!(
        r.models.iter().any(|m| m.id == "acme-labs-acme-one"),
        "{:?}",
        r.added
    );
    Catalog::parse(&text).unwrap();
}

#[test]
fn block_roundtrips_and_splice_requires_both_markers() {
    let (r, text) = run_with(CATALOG_TOML, &day1(), "2026-10-28");
    let g = GeneratedBlock::parse(block_of(&text).unwrap()).unwrap();
    assert_eq!(g.models, r.models);
    assert_eq!(g.measurements, r.measurements);
    assert_eq!(g.aa.unwrap(), r.meta);
    assert!(splice("nothing", "x").unwrap_err().contains("開始行"));
    assert!(splice(&format!("{BEGIN}\n"), "x")
        .unwrap_err()
        .contains("終了行"));
}

#[test]
fn a_generated_block_may_not_carry_prices() {
    let e = GeneratedBlock::parse(
        "[[model]]\nid = \"x\"\nprovider = \"p\"\nname = \"X\"\ninput = 1.0\n",
    )
    .unwrap_err();
    assert!(e.contains("価格"), "{e}");
    let e = GeneratedBlock::parse("[[provider]]\nid = \"p\"\nname = \"P\"\napi_context = 1\n")
        .unwrap_err();
    assert!(e.contains("価格"), "{e}");
}

#[test]
fn rejects_non_catalog_json() {
    assert!(run(CATALOG_TOML, "[]", "2026-10-28")
        .unwrap_err()
        .contains("data"));
    assert!(run(CATALOG_TOML, "not json", "2026-10-28")
        .unwrap_err()
        .contains("JSON"));
}

#[test]
fn integral_floats_keep_a_decimal_point() {
    assert_eq!(f(1.0), "1.0");
    assert_eq!(f(0.565656565656566), "0.565656565656566");
}

#[test]
fn many_providers_and_hundreds_of_models_register_without_code_changes() {
    let mut rows = Vec::new();
    let mut n = 100;
    for p in 0..33 {
        for m in 0..5 {
            for e in ["Low", "High"] {
                n += 1;
                rows.push(row(
                    n,
                    &format!("Model {p}-{m} ({e})"),
                    &format!("Maker {p}"),
                    "2026-09-15",
                    Some(0.3),
                ));
            }
        }
    }
    let (r, t1) = run_with(CATALOG_TOML, &rows, "2026-10-28");
    assert_eq!(r.providers.len(), 33);
    assert_eq!(r.models.len(), 165);
    assert_eq!(
        r.measurements
            .iter()
            .filter(|m| m.model.starts_with("model-"))
            .count(),
        330
    );
    let (_, t2) = run_with(&t1, &rows, "2026-10-28");
    assert_eq!(t1, t2);
}
