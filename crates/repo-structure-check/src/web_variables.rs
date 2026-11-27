//! デザイントークンの不変条件: 正本 `config/web/variables.css` は、役割の集合が閉じていて、明暗の両方で
//! WCAG 2.2 AA のコントラストを満たす(ADR-1685)。
//!
//! 正本の形(ADR-1685):
//!
//! - 層は 2 つ。原色(`--ref-*`)は 16 進の値だけを持ち、役割(`--bg` など)は `var()` で原色を指して
//!   `light-dark(明, 暗)` で明暗を 1 回だけ書く。役割の層に 16 進を直書きしない。
//! - 役割の集合は [`REQUIRED_ROLES`] で閉じている。足りない役割は違反。
//! - 文字・図形の組([`PAIRS`])は、明暗の両方で、文字 4.5:1・図形 3:1 を満たす。
//! - `color-scheme: light dark` と、Artifact のホストの契約(`:root[data-theme="light"]`・
//!   `:root[data-theme="dark"]`)と、`prefers-reduced-motion` の規則を持つ。`prefers-color-scheme` の
//!   二重書きは持たない。
//!
//! 読む値は 16 進(`#rgb`・`#rrggbb`)、`var(--名前)`、`light-dark(…, …)` だけ。ほかの形(`oklch()` など)は、
//! 解釈できない値として違反にする(検査が読めない色を、通してしまわないため)。
//!
//! 範囲はこのリポジトリの内側(正本 1 ファイル)。各面が複写を持つ側の検査は、`.github/actions/design-pin`。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::manifest::Violation;

/// 正本のパス(リポジトリの根から)。
pub const VARIABLES_PATH: &str = "config/web/variables.css";

/// 出発点(`config/web/variables.css`)の役割の集合(閉じた語彙)。検査するのは出発点の自己整合だけで、
/// 面にこの集合を強いない(ADR-1729)。出発点を採る面は、この集合に足さず、値だけを上書きする。
pub const REQUIRED_ROLES: [&str; 17] = [
    "--bg",
    "--surface",
    "--text",
    "--text-muted",
    "--border",
    "--border-subtle",
    "--accent",
    "--on-accent",
    "--accent-text",
    "--focus",
    "--marker",
    "--ok",
    "--ok-bg",
    "--ng",
    "--ng-bg",
    "--warn",
    "--warn-bg",
];

/// 文字・図形の組: (前景の役割, 背景の役割, 最小のコントラスト比, 何を表す組か)。
/// 文字は 4.5:1(WCAG 2.2 SC 1.4.3)、図形・境界・フォーカスは 3:1(SC 1.4.11)。
pub const PAIRS: [(&str, &str, f64, &str); 21] = [
    ("--text", "--bg", 4.5, "本文"),
    ("--text", "--surface", 4.5, "面の上の本文"),
    ("--text-muted", "--bg", 4.5, "弱い文字"),
    ("--text-muted", "--surface", 4.5, "面の上の弱い文字"),
    ("--accent-text", "--bg", 4.5, "リンク・強調の文字"),
    (
        "--accent-text",
        "--surface",
        4.5,
        "面の上のリンク・強調の文字",
    ),
    ("--on-accent", "--accent", 4.5, "強調色の上の文字"),
    ("--ok", "--ok-bg", 4.5, "成功の文字"),
    ("--ng", "--ng-bg", 4.5, "誤りの文字"),
    ("--warn", "--warn-bg", 4.5, "注意の文字"),
    ("--ok", "--bg", 4.5, "地の上の成功の文字"),
    ("--ng", "--bg", 4.5, "地の上の誤りの文字"),
    ("--warn", "--bg", 4.5, "地の上の注意の文字"),
    ("--ok", "--surface", 4.5, "面の上の成功の文字"),
    ("--ng", "--surface", 4.5, "面の上の誤りの文字"),
    ("--warn", "--surface", 4.5, "面の上の注意の文字"),
    ("--border", "--bg", 3.0, "入力欄・ボタンの境界"),
    ("--border", "--surface", 3.0, "面の上の入力欄・ボタンの境界"),
    ("--focus", "--bg", 3.0, "フォーカスの輪"),
    ("--focus", "--surface", 3.0, "面の上のフォーカスの輪"),
    ("--accent", "--bg", 3.0, "強調色の図形"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    fn label(self) -> &'static str {
        match self {
            Scheme::Light => "明",
            Scheme::Dark => "暗",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rgb(u8, u8, u8);

fn violation(path: &str, detail: String) -> Violation {
    Violation {
        subject: path.to_string(),
        detail,
    }
}

/// `/* … */` を取り除く(複数行を含む)。
pub(crate) fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `--名前: 値;`(1 行)の宣言を、出てきた順に取る。
pub(crate) fn declarations(stripped: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in stripped.lines() {
        let line = line.trim();
        if !line.starts_with("--") || !line.ends_with(';') {
            continue;
        }
        let body = &line[..line.len() - 1];
        if let Some((name, value)) = body.split_once(':') {
            out.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    out
}

/// 最上位の `,` で 2 つに分ける(括弧の中の `,` では分けない)。
fn split_top_level_comma(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

fn parse_hex(s: &str) -> Result<Rgb, String> {
    let h = s.trim_start_matches('#');
    let digit = |c: char| c.to_digit(16).map(|d| d as u8);
    let chars: Vec<char> = h.chars().collect();
    match chars.len() {
        3 => {
            let d: Option<Vec<u8>> = chars.iter().map(|c| digit(*c)).collect();
            let d = d.ok_or_else(|| format!("16 進として読めない `{s}`"))?;
            Ok(Rgb(d[0] * 17, d[1] * 17, d[2] * 17))
        }
        6 => {
            let d: Option<Vec<u8>> = chars.iter().map(|c| digit(*c)).collect();
            let d = d.ok_or_else(|| format!("16 進として読めない `{s}`"))?;
            Ok(Rgb(d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]))
        }
        _ => Err(format!("`#rgb` か `#rrggbb` でない `{s}`")),
    }
}

/// 値を、明か暗の色に解く。`var()` は宣言を引き、`light-dark()` は明暗で選ぶ。
fn resolve(
    map: &BTreeMap<String, String>,
    value: &str,
    scheme: Scheme,
    depth: usize,
) -> Result<Rgb, String> {
    if depth > 8 {
        return Err("`var()` が循環しているか、深すぎる".to_string());
    }
    let v = value.trim();
    if let Some(inner) = v.strip_prefix("var(").and_then(|r| r.strip_suffix(')')) {
        let name = inner.trim();
        if name.contains(',') {
            return Err(format!("`var()` の既定値は読まない `{v}`"));
        }
        let next = map
            .get(name)
            .ok_or_else(|| format!("`{name}` の宣言が無い"))?;
        return resolve(map, next, scheme, depth + 1);
    }
    if let Some(inner) = v
        .strip_prefix("light-dark(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let (light, dark) = split_top_level_comma(inner)
            .ok_or_else(|| format!("`light-dark()` の引数が 2 つでない `{v}`"))?;
        let pick = match scheme {
            Scheme::Light => light,
            Scheme::Dark => dark,
        };
        return resolve(map, pick, scheme, depth + 1);
    }
    if v.starts_with('#') {
        return parse_hex(v);
    }
    Err(format!(
        "解釈できない値 `{v}`(16 進・`var()`・`light-dark()` だけ読む)"
    ))
}

fn luminance(c: Rgb) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG 2.2 のコントラスト比。
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x >= y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// 空白の連なりを 1 つにそろえる(規則の書き方の揺れを許す)。
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 正本の文字列を検査する。
pub fn check_tokens(path: &str, text: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let stripped = strip_comments(text);
    let decls = declarations(&stripped);
    let map: BTreeMap<String, String> = decls.iter().cloned().collect();

    // 層: 原色は 16 進だけ、役割は 16 進を直書きしない。
    for (name, value) in &decls {
        if name.starts_with("--ref-") {
            if parse_hex(value).is_err() {
                out.push(violation(
                    path,
                    format!(
                        "原色 `{name}` が 16 進の値でない(`{value}`)。原色は `#rrggbb` だけを持つ"
                    ),
                ));
            }
        } else if value.contains('#') {
            out.push(violation(
                path,
                format!("役割・尺度 `{name}` が 16 進を直書きしている(`{value}`)。原色(`--ref-*`)を `var()` で指す"),
            ));
        }
    }

    // 役割の集合と、明暗の解決。
    let mut resolved: BTreeMap<(&str, bool), Rgb> = BTreeMap::new();
    for role in REQUIRED_ROLES {
        let Some(value) = map.get(role) else {
            out.push(violation(
                path,
                format!("役割 `{role}` の宣言が無い(役割の集合は閉じている)"),
            ));
            continue;
        };
        for scheme in [Scheme::Light, Scheme::Dark] {
            match resolve(&map, value, scheme, 0) {
                Ok(c) => {
                    resolved.insert((role, scheme == Scheme::Dark), c);
                }
                Err(e) => out.push(violation(
                    path,
                    format!("役割 `{role}`({})を色に解けない: {e}", scheme.label()),
                )),
            }
        }
    }

    // コントラスト。
    for (fg, bg, min, what) in PAIRS {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let key_fg = (fg, scheme == Scheme::Dark);
            let key_bg = (bg, scheme == Scheme::Dark);
            if let (Some(a), Some(b)) = (resolved.get(&key_fg), resolved.get(&key_bg)) {
                let ratio = contrast(*a, *b);
                if ratio < min {
                    out.push(violation(
                        path,
                        format!(
                            "{what}(`{fg}` を `{bg}` の上に置く。{})が {ratio:.2}:1 で、基準の {min}:1 に届かない",
                            scheme.label()
                        ),
                    ));
                }
            }
        }
    }

    // 規則の形。
    let flat = squash(&stripped);
    for (needle, why) in [
        (
            "color-scheme: light dark",
            "`:root` に `color-scheme: light dark;`(OS の設定への追従)",
        ),
        (
            ":root[data-theme=\"light\"]",
            "Artifact のホストの契約 `:root[data-theme=\"light\"]` の規則",
        ),
        (
            ":root[data-theme=\"dark\"]",
            "Artifact のホストの契約 `:root[data-theme=\"dark\"]` の規則",
        ),
        (
            "prefers-reduced-motion",
            "`prefers-reduced-motion` の規則(動きの長さを 0 にする)",
        ),
    ] {
        if !flat.contains(needle) {
            out.push(violation(path, format!("{why}が無い")));
        }
    }
    if flat.contains("prefers-color-scheme") {
        out.push(violation(
            path,
            "`@media (prefers-color-scheme)` を書いている。明暗は `light-dark()` で 1 回だけ書く(二重書きにしない)".to_string(),
        ));
    }
    out
}

/// ツリーの正本を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let p = root.join(VARIABLES_PATH);
    let text = fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))?;
    Ok(check_tokens(VARIABLES_PATH, &text))
}

/// 生成した機械可読の値のパス(リポジトリの根から)。手で書かず、`tokens_json` が作る。
pub const VARIABLES_JSON_PATH: &str = "config/web/variables.json";

/// 再生成のコマンド(ずれたときの案内)。
pub const VARIABLES_JSON_REGENERATE: &str =
    "cargo run -q -p repo-structure-check --example web_variables_json > config/web/variables.json";

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// 正本の CSS から、機械可読の値(JSON)を作る。原色・役割(明暗を解いた 16 進)・尺度を、決まった順で出す。
/// 印刷物(Typst)や、claude.ai の Design System 型の Artifact が読む。CSS が唯一の正本で、これは派生物。
pub fn tokens_json(text: &str) -> Result<String, String> {
    let stripped = strip_comments(text);
    let mut seen = std::collections::BTreeSet::new();
    let decls: Vec<(String, String)> = declarations(&stripped)
        .into_iter()
        .filter(|(name, _)| seen.insert(name.clone())) // `@media` の中の再宣言は、最初の値だけ
        .collect();
    let map: BTreeMap<String, String> = decls.iter().cloned().collect();

    let mut refs = Vec::new();
    for (name, value) in &decls {
        if name.starts_with("--ref-") {
            let rgb = parse_hex(value).map_err(|e| format!("原色 `{name}`: {e}"))?;
            refs.push(format!(
                "    {{ \"name\": {}, \"value\": {} }}",
                json_str(name.trim_start_matches("--")),
                json_str(&hex(rgb))
            ));
        }
    }
    let mut colors = Vec::new();
    for role in REQUIRED_ROLES {
        let value = map
            .get(role)
            .ok_or_else(|| format!("役割 `{role}` の宣言が無い"))?;
        let light = resolve(&map, value, Scheme::Light, 0)
            .map_err(|e| format!("役割 `{role}`(明): {e}"))?;
        let dark =
            resolve(&map, value, Scheme::Dark, 0).map_err(|e| format!("役割 `{role}`(暗): {e}"))?;
        colors.push(format!(
            "    {{ \"name\": {}, \"light\": {}, \"dark\": {} }}",
            json_str(role.trim_start_matches("--")),
            json_str(&hex(light)),
            json_str(&hex(dark))
        ));
    }
    let mut scale = Vec::new();
    for (name, value) in &decls {
        if name.starts_with("--ref-") || REQUIRED_ROLES.contains(&name.as_str()) {
            continue;
        }
        scale.push(format!(
            "    {{ \"name\": {}, \"value\": {} }}",
            json_str(name.trim_start_matches("--")),
            json_str(value)
        ));
    }
    Ok(format!(
        "{{\n  \"generated\": {},\n  \"source\": {},\n  \"themes\": [\"light\", \"dark\"],\n  \"refs\": [\n{}\n  ],\n  \"colors\": [\n{}\n  ],\n  \"scale\": [\n{}\n  ]\n}}\n",
        json_str(&format!("手で書かない。`{VARIABLES_JSON_REGENERATE}` で作る")),
        json_str(VARIABLES_PATH),
        refs.join(",\n"),
        colors.join(",\n"),
        scale.join(",\n"),
    ))
}

/// ツリーの `variables.json` が、正本の CSS から作ったものと一致するか。
pub fn check_tokens_json_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    let css = read(VARIABLES_PATH)?;
    let have = read(VARIABLES_JSON_PATH)?;
    let want = tokens_json(&css)?;
    if have == want {
        return Ok(Vec::new());
    }
    Ok(vec![violation(
        VARIABLES_JSON_PATH,
        format!("正本 `{VARIABLES_PATH}` から作った内容と違う(手で直した、または CSS を変えて作り直していない)。`{VARIABLES_JSON_REGENERATE}` で作り直す"),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正本を、実行時に読む。`include_str!` は、nix の clippy の derivation(cargo 用に絞ったソース)で、
    /// `config/` を読めず、コンパイルが落ちる。
    static REAL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(VARIABLES_PATH);
        fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} を読めない: {e}", p.display()))
    });

    fn details(vs: &[Violation]) -> String {
        vs.iter()
            .map(|v| v.detail.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_real_tokens_pass() {
        let vs = check_tokens(VARIABLES_PATH, &REAL);
        assert!(vs.is_empty(), "{}", details(&vs));
    }

    #[test]
    fn contrast_matches_the_wcag_definition() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        assert!((contrast(black, white) - 21.0).abs() < 1e-9);
        // 同じ色は 1:1。
        assert!((contrast(white, white) - 1.0).abs() < 1e-9);
        // 実測で確かめた組(暗色の白文字を淡い強調色の上に置く)。
        let accent = parse_hex("#e0907c").unwrap();
        assert!((contrast(white, accent) - 2.49).abs() < 0.01);
    }

    #[test]
    fn a_low_contrast_pair_is_reported_in_the_scheme_it_fails() {
        // 弱い文字を、明の背景とほぼ同じ明るさにする。
        let bad = REAL.replace("--ref-steel-600: #5b6670;", "--ref-steel-600: #c3c9ce;");
        let vs = check_tokens(VARIABLES_PATH, &bad);
        let d = details(&vs);
        assert!(d.contains("弱い文字") && d.contains("明"), "{d}");
    }

    #[test]
    fn a_white_text_on_a_light_accent_in_the_dark_scheme_fails() {
        // 暗色の `--on-accent` を白にすると、淡い強調色の上で読めない(実測した型 P4)。
        let bad = REAL.replace(
            "--on-accent: light-dark(var(--ref-white), var(--ref-steel-900));",
            "--on-accent: light-dark(var(--ref-white), var(--ref-white));",
        );
        let d = details(&check_tokens(VARIABLES_PATH, &bad));
        assert!(d.contains("強調色の上の文字") && d.contains("暗"), "{d}");
    }

    #[test]
    fn a_missing_role_is_reported() {
        let bad = REAL.replace(
            "  --ng-bg: light-dark(var(--ref-red-50), var(--ref-red-1200));\n",
            "",
        );
        let d = details(&check_tokens(VARIABLES_PATH, &bad));
        assert!(d.contains("`--ng-bg` の宣言が無い"), "{d}");
    }

    #[test]
    fn a_raw_hex_in_the_role_layer_is_reported() {
        let bad = REAL.replace(
            "--focus: light-dark(var(--ref-steel-900), var(--ref-steel-100));",
            "--focus: light-dark(#1b1f23, #e9ecee);",
        );
        let d = details(&check_tokens(VARIABLES_PATH, &bad));
        assert!(d.contains("16 進を直書き"), "{d}");
    }

    #[test]
    fn a_color_the_check_cannot_read_is_reported_not_passed() {
        let bad = REAL.replace(
            "--ref-steel-900: #1b1f23;",
            "--ref-steel-900: oklch(0.4 0.2 260);",
        );
        let d = details(&check_tokens(VARIABLES_PATH, &bad));
        assert!(
            d.contains("原色 `--ref-steel-900` が 16 進の値でない"),
            "{d}"
        );
        assert!(d.contains("色に解けない"), "{d}");
    }

    #[test]
    fn the_host_contract_and_the_single_writing_are_required() {
        let no_theme = REAL.replace(":root[data-theme=\"dark\"]", ":root[data-x=\"dark\"]");
        assert!(details(&check_tokens(VARIABLES_PATH, &no_theme)).contains("data-theme=\"dark\""));
        let doubled = format!(
            "{}\n@media (prefers-color-scheme: dark) {{ :root {{ --x: 0; }} }}\n",
            *REAL
        );
        assert!(details(&check_tokens(VARIABLES_PATH, &doubled)).contains("二重書き"));
    }

    #[test]
    fn a_comment_that_mentions_the_media_query_is_not_a_violation() {
        // 説明のコメントに `prefers-color-scheme` と書いても、規則ではないので通る(実際の正本もそう書く)。
        let ok = format!(
            "{}\n/* `@media (prefers-color-scheme)` は書かない */\n",
            *REAL
        );
        assert!(check_tokens(VARIABLES_PATH, &ok).is_empty());
    }

    #[test]
    fn the_generated_tokens_carry_every_role_in_both_schemes() {
        let json = tokens_json(&REAL).expect("generate");
        for role in REQUIRED_ROLES {
            assert!(
                json.contains(&format!("\"name\": \"{}\"", role.trim_start_matches("--"))),
                "{role} が生成物に無い"
            );
        }
        // `--bg: light-dark(var(--ref-steel-100), var(--ref-steel-900))` を、明暗の 16 進に解く。
        assert!(
            json.contains("{ \"name\": \"bg\", \"light\": \"#e9ecee\", \"dark\": \"#1b1f23\" }"),
            "{json}"
        );
        // `@media (prefers-reduced-motion)` の中の再宣言(0ms)でなく、最初の宣言(100ms)を載せる。
        assert!(
            json.contains("{ \"name\": \"dur-fast\", \"value\": \"100ms\" }"),
            "{json}"
        );
        // 引用符を含む書体の列は、JSON の文字列として正しく escape される。
        assert!(json.contains("\\\"Noto Sans JP\\\""), "{json}");
    }

    #[test]
    fn the_generated_tokens_are_deterministic() {
        assert_eq!(tokens_json(&REAL).unwrap(), tokens_json(&REAL).unwrap());
    }

    #[test]
    fn a_role_that_cannot_be_resolved_is_an_error_not_a_hole() {
        let bad = REAL.replace(
            "--ref-steel-900: #1b1f23;",
            "--ref-steel-900: oklch(0.2 0 0);",
        );
        assert!(tokens_json(&bad).is_err());
    }

    #[test]
    fn a_var_cycle_is_reported_and_does_not_hang() {
        // `--ref-steel-100` が `--bg` を指し、`--bg` が `--ref-steel-100` を指す。
        let bad = REAL.replace("--ref-steel-100: #e9ecee;", "--ref-steel-100: var(--bg);");
        let d = details(&check_tokens(VARIABLES_PATH, &bad));
        assert!(d.contains("循環"), "{d}");
    }
}
