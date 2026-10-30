//! as-code 化(決定論化)の候補を振り返りで記録するときの閉語彙(段・兆候・却下理由・較正のしきい値)。
//!
//! `config/agents/codify-signals/vocabulary.json` が唯一の正本で、この module は `include_str!` で読む
//! (`worklog::mail::Vocabulary` と同じ型、ADR-1259)。retro の手順の文・docs の事例集の表・行の検証・
//! 日次の較正は、すべてこの語彙から出る。語彙の形の不整合(既定の段が段の一覧に無い、など)は
//! `parse` が拒否するので、壊れた語彙は読み込めない。

use serde_json::Value;
use std::sync::OnceLock;

const VOCABULARY_JSON: &str = include_str!("../../../config/agents/codify-signals/vocabulary.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub id: String,
    pub rank: i64,
    pub means: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub id: String,
    pub default_level: String,
    pub since: String,
    pub look_for: String,
    pub examples: Vec<String>,
    pub formal_spec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub id: String,
    pub strong: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calibration {
    pub min_dropped: u32,
    pub drop_ratio: (u32, u32),
    pub misread_min: u32,
    pub existing_suffices_min: u32,
    pub unclassified_min: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fire {
    pub scratch_code_files: u32,
    pub shaped_repeat: u32,
    pub strong_shapes_total: u32,
}

#[derive(Debug, Clone)]
pub struct Vocabulary {
    pub levels: Vec<Level>,
    pub aliases: Vec<(String, String)>,
    pub signals: Vec<Signal>,
    pub existing_verdicts: Vec<String>,
    pub in_repo_scope: Vec<String>,
    pub external_scope: Vec<String>,
    pub shapes: Vec<Shape>,
    pub scratch_code_ext: Vec<String>,
    pub fire: Fire,
    pub drop_reasons: Vec<String>,
    pub calibration: Calibration,
}

fn strs(x: &Value, what: &str) -> Result<Vec<String>, String> {
    x.as_array()
        .ok_or(format!("{what} が配列でない"))?
        .iter()
        .map(|s| {
            s.as_str()
                .map(str::to_string)
                .ok_or(format!("{what} に文字列でない要素がある"))
        })
        .collect()
}

fn field(o: &Value, k: &str) -> Result<String, String> {
    o.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(format!("'{k}' が無い"))
}

fn count(v: &Value, path: &str) -> Result<u32, String> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(format!("{path} が整数でない"))
}

fn unique(ids: &[&str], what: &str) -> Result<(), String> {
    for (i, a) in ids.iter().enumerate() {
        if ids[..i].contains(a) {
            return Err(format!("{what} の id '{a}' が重複している"));
        }
    }
    Ok(())
}

impl Vocabulary {
    pub fn parse(json: &str) -> Result<Vocabulary, String> {
        let v: Value =
            serde_json::from_str(json).map_err(|e| format!("語彙が JSON でない: {e}"))?;

        let mut levels = Vec::new();
        for l in v["levels"].as_array().ok_or("levels が無い")? {
            levels.push(Level {
                id: field(l, "id")?,
                rank: l["rank"].as_i64().ok_or("level の rank が無い")?,
                means: field(l, "means")?,
            });
        }
        unique(
            &levels.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            "levels",
        )?;

        let mut aliases = Vec::new();
        for (from, to) in v["aliases"].as_object().ok_or("aliases が無い")? {
            let to = to.as_str().ok_or("alias の行き先が文字列でない")?;
            if !levels.iter().any(|l| l.id == to) {
                return Err(format!("alias {from} の行き先 '{to}' が levels にない"));
            }
            if levels.iter().any(|l| &l.id == from) {
                return Err(format!("alias {from} が段と同じ名前"));
            }
            aliases.push((from.clone(), to.to_string()));
        }

        let mut signals = Vec::new();
        for s in v["signals"].as_array().ok_or("signals が無い")? {
            let default_level = field(s, "default")?;
            if !levels.iter().any(|l| l.id == default_level) {
                return Err(format!(
                    "兆候 {} の既定の段 '{default_level}' が levels にない",
                    field(s, "id")?
                ));
            }
            signals.push(Signal {
                id: field(s, "id")?,
                default_level,
                since: field(s, "since")?,
                look_for: field(s, "look_for")?,
                examples: strs(&s["examples"], "examples")?,
                formal_spec: s["formal_spec"].as_bool().unwrap_or(false),
            });
        }
        unique(
            &signals.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            "signals",
        )?;

        let mut shapes = Vec::new();
        for s in v["shapes"].as_array().ok_or("shapes が無い")? {
            let strong = match field(s, "tier")?.as_str() {
                "strong" => true,
                "weak" => false,
                t => return Err(format!("shape の tier '{t}' は strong|weak のいずれか")),
            };
            shapes.push(Shape {
                id: field(s, "id")?,
                strong,
            });
        }
        unique(
            &shapes.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            "shapes",
        )?;

        let ratio = v["calibration"]["drop_ratio"]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("calibration.drop_ratio が [分子, 分母] でない")?;
        let (num, den) = (
            count(&ratio[0], "drop_ratio[0]")?,
            count(&ratio[1], "drop_ratio[1]")?,
        );
        if den == 0 {
            return Err("calibration.drop_ratio の分母が 0".to_string());
        }

        let drop_reasons = strs(&v["drop_reasons"], "drop_reasons")?;
        if drop_reasons.is_empty() {
            return Err("drop_reasons が空".to_string());
        }

        Ok(Vocabulary {
            levels,
            aliases,
            signals,
            existing_verdicts: strs(&v["existing"]["verdicts"], "existing.verdicts")?,
            in_repo_scope: strs(&v["existing"]["in_repo_scope"], "existing.in_repo_scope")?,
            external_scope: strs(&v["existing"]["external_scope"], "existing.external_scope")?,
            shapes,
            scratch_code_ext: strs(&v["traces"]["scratch_code_ext"], "traces.scratch_code_ext")?,
            fire: Fire {
                scratch_code_files: count(
                    &v["traces"]["fire"]["scratch_code_files"],
                    "fire.scratch_code_files",
                )?,
                shaped_repeat: count(&v["traces"]["fire"]["shaped_repeat"], "fire.shaped_repeat")?,
                strong_shapes_total: count(
                    &v["traces"]["fire"]["strong_shapes_total"],
                    "fire.strong_shapes_total",
                )?,
            },
            drop_reasons,
            calibration: Calibration {
                min_dropped: count(&v["calibration"]["min_dropped"], "calibration.min_dropped")?,
                drop_ratio: (num, den),
                misread_min: count(&v["calibration"]["misread_min"], "calibration.misread_min")?,
                existing_suffices_min: count(
                    &v["calibration"]["existing_suffices_min"],
                    "calibration.existing_suffices_min",
                )?,
                unclassified_min: count(
                    &v["calibration"]["unclassified_min"],
                    "calibration.unclassified_min",
                )?,
            },
        })
    }

    pub fn get() -> &'static Vocabulary {
        static V: OnceLock<Vocabulary> = OnceLock::new();
        V.get_or_init(|| {
            Vocabulary::parse(VOCABULARY_JSON).expect("codify-signals の語彙が壊れている")
        })
    }

    pub fn level(&self, id: &str) -> Option<&Level> {
        self.levels.iter().find(|l| l.id == id)
    }

    /// 旧い段の名前(`script`)を今の段の名前に読み替える。段でも別名でもなければ `None`。
    pub fn canonical_level(&self, id: &str) -> Option<&str> {
        if let Some(l) = self.level(id) {
            return Some(&l.id);
        }
        self.aliases
            .iter()
            .find(|(from, _)| from == id)
            .map(|(_, to)| to.as_str())
    }

    pub fn signal(&self, id: &str) -> Option<&Signal> {
        self.signals.iter().find(|s| s.id == id)
    }

    pub fn is_drop_reason(&self, id: &str) -> bool {
        self.drop_reasons.iter().any(|r| r == id)
    }

    pub fn is_existing_verdict(&self, id: &str) -> bool {
        self.existing_verdicts.iter().any(|r| r == id)
    }

    /// 段を上位から下位の順に並べた id(`eliminate > … > prose`)。
    pub fn levels_desc(&self) -> Vec<&Level> {
        let mut v: Vec<&Level> = self.levels.iter().collect();
        v.sort_by(|a, b| b.rank.cmp(&a.rank));
        v
    }

    /// `docs/agents/codify-signals.md` の生成部に入れる事例集(段と兆候の 2 つの表)。
    /// 手で書き直さず、`session-retrospective --retro-casebook` の出力に差し替える。
    pub fn casebook_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str("### 段(上位ほど優先)\n\n| 段 | 意味 |\n|---|---|\n");
        for l in self.levels_desc() {
            s.push_str(&format!("| `{}` | {} |\n", l.id, l.means));
        }
        s.push_str(
            "\n### 兆候\n\n| 兆候 | 既定の段 | こういうことがあった | 事例 |\n|---|---|---|---|\n",
        );
        for g in &self.signals {
            let ex = if g.examples.is_empty() {
                "—".to_string()
            } else {
                g.examples.join("、")
            };
            let spec = if g.formal_spec {
                "(形式仕様も添える)"
            } else {
                ""
            };
            s.push_str(&format!(
                "| `{}` | `{}`{} | {} | {} |\n",
                g.id, g.default_level, spec, g.look_for, ex
            ));
        }
        s.push_str("\n### 却下の理由(`/backlog-cleanup` で却下するとき)\n\n");
        for r in &self.drop_reasons {
            s.push_str(&format!("- `{r}`\n"));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_vocabulary_parses() {
        let v = Vocabulary::get();
        assert_eq!(v.levels_desc()[0].id, "eliminate");
        assert_eq!(v.levels_desc().last().unwrap().id, "prose");
        assert!(v.signal("copy-drift").is_some());
    }

    #[test]
    fn script_reads_as_tool() {
        let v = Vocabulary::get();
        assert_eq!(v.canonical_level("script"), Some("tool"));
        assert_eq!(v.canonical_level("gate"), Some("gate"));
        assert_eq!(v.canonical_level("nonsense"), None);
    }

    #[test]
    fn every_default_is_a_level() {
        let v = Vocabulary::get();
        for s in &v.signals {
            assert!(v.level(&s.default_level).is_some(), "{}", s.id);
        }
    }

    fn mutated(f: impl FnOnce(&mut Value)) -> Result<Vocabulary, String> {
        let mut j: Value = serde_json::from_str(VOCABULARY_JSON).unwrap();
        f(&mut j);
        Vocabulary::parse(&j.to_string())
    }

    #[test]
    fn rejects_a_default_that_is_not_a_level() {
        let e = mutated(|j| j["signals"][0]["default"] = Value::from("nope")).unwrap_err();
        assert!(e.contains("既定の段"), "{e}");
    }

    #[test]
    fn rejects_duplicate_signal_ids() {
        let e = mutated(|j| {
            let first = j["signals"][0].clone();
            j["signals"].as_array_mut().unwrap().push(first);
        })
        .unwrap_err();
        assert!(e.contains("重複"), "{e}");
    }

    #[test]
    fn rejects_an_alias_to_a_missing_level() {
        let e = mutated(|j| j["aliases"]["script"] = Value::from("nope")).unwrap_err();
        assert!(e.contains("alias"), "{e}");
    }

    #[test]
    fn rejects_a_zero_denominator() {
        let e =
            mutated(|j| j["calibration"]["drop_ratio"] = serde_json::json!([2, 0])).unwrap_err();
        assert!(e.contains("分母"), "{e}");
    }

    #[test]
    fn casebook_lists_every_signal_and_drop_reason() {
        let v = Vocabulary::get();
        let md = v.casebook_markdown();
        for s in &v.signals {
            assert!(md.contains(&format!("`{}`", s.id)), "{}", s.id);
        }
        for r in &v.drop_reasons {
            assert!(md.contains(&format!("`{r}`")), "{r}");
        }
    }
}
