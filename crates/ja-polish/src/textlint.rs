//! textlint の自動修正(`--fix`)。Gemini の前後に挟んで、定型の冗長表現(「することができる。」など)を
//! 決定論的に直す(docs/adr/1387-ja-polish.md の D13)。
//!
//! - 対象は、伏せ字つきの段落。コード・URL・参照は伏せ字のまま渡るので、textlint に触られない。
//! - 規則は、nixpkgs が束ねる `textlint-rule-preset-ja-technical-writing` のうち `ja-no-redundant-expression`
//!   だけを有効にし(ほかは `PRESET_RULES_OFF` で無効)、辞書があれば `prh` を足す。
//!   設定は ja-polish が実行のたびに一時ディレクトリへ作る(作業ツリーに `.textlintrc` を置かない)。
//! - 道具が無い・失敗・時間切れ・段落の数が合わないときは、入力のまま返す。止めない(ja-polish 全体の
//!   fail-open と同じ)。直した結果の照合は、呼び出し側(`polish::polish`)が Gemini の出力と同じ関数でする。
//! - 全段落を 1 つのファイルにまとめて 1 回だけ起動する(起動に約 1 秒かかるため)。段落は空行で
//!   区切られ、直しは段落の中に閉じるので、空行で割り戻せる。

use crate::config::Config;
use crate::style;
use process_control::{spawn_group, wait_with_timeout};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// 段落を機械的に直す道具。テストでは差し替える。
pub trait Fixer: Sync {
    /// 各段落(伏せ字つき)を直して返す。戻りは入力と同じ長さ・同じ並び。直せない・失敗したときは入力のまま。
    fn fix(&self, texts: &[String]) -> Vec<String>;

    /// 段落に残る textlint の指摘の数(直さずに数える)。測定(`ja-polish bench`)が、書き直しの前後で
    /// 比べるのに使う。道具が無い・失敗したときは `None`。
    fn issues(&self, _texts: &[String]) -> Option<usize> {
        None
    }
}

/// 何も直さない。textlint を使わない設定のとき、およびテストで使う。
pub struct NoFixer;

impl Fixer for NoFixer {
    fn fix(&self, texts: &[String]) -> Vec<String> {
        texts.to_vec()
    }
}

/// textlint 1 回の待ち。
const FIX_TIMEOUT: Duration = Duration::from_secs(30);

/// 段落どうしの区切り。`blocks::split` の段落は空行を含まないので、割り戻せる。
const SEP: &str = "\n\n";

/// `textlint-rule-preset-ja-technical-writing` 10.0.1(nixpkgs の固定版)の規則のうち、無効にするもの。
/// 有効にするのは `ja-no-redundant-expression` だけ。preset が規則を足しても、足された規則は有効のままになるが、
/// 戻りは照合を通るものだけを採るので、意味や記法を壊す直しは入らない。
const PRESET_RULES_OFF: [&str; 22] = [
    "sentence-length",
    "max-comma",
    "max-ten",
    "max-kanji-continuous-len",
    "no-mix-dearu-desumasu",
    "ja-no-mixed-period",
    "arabic-kanji-numbers",
    "no-doubled-conjunction",
    "no-doubled-conjunctive-particle-ga",
    "no-double-negative-ja",
    "no-doubled-joshi",
    "no-dropping-the-ra",
    "no-nfd",
    "no-exclamation-question-mark",
    "no-hankaku-kana",
    "no-invalid-control-character",
    "ja-no-weak-phrase",
    "ja-no-successive-word",
    "ja-no-abusage",
    "ja-unnatural-alphabet",
    "no-unmatched-pair",
    "no-zero-width-spaces",
];

/// textlint の設定(JSON)。`prh` の辞書があれば足す。
pub fn rc_json(prh: Option<&Path>) -> serde_json::Value {
    let mut preset = serde_json::Map::new();
    preset.insert("ja-no-redundant-expression".into(), true.into());
    for name in PRESET_RULES_OFF {
        preset.insert(name.into(), false.into());
    }
    let mut rules = serde_json::Map::new();
    rules.insert("preset-ja-technical-writing".into(), preset.into());
    if let Some(p) = prh {
        rules.insert(
            "prh".into(),
            serde_json::json!({ "rulePaths": [p.to_string_lossy()] }),
        );
    }
    serde_json::json!({ "rules": rules })
}

/// `textlint` コマンドを呼ぶ道具。実行ファイルは環境変数 `JA_POLISH_TEXTLINT_BIN` で差し替えられる
/// (既定は PATH の `textlint`)。
pub struct Textlint {
    bin: String,
    prh: Option<PathBuf>,
}

impl Textlint {
    /// 設定で textlint を使わない(`textlint = false`)ときは `None`。
    pub fn from_config(cfg: &Config) -> Option<Self> {
        if !cfg.textlint {
            return None;
        }
        Some(Textlint {
            bin: std::env::var("JA_POLISH_TEXTLINT_BIN").unwrap_or_else(|_| "textlint".to_string()),
            prh: style::prh_path().filter(|p| p.is_file()),
        })
    }

    fn try_fix(&self, texts: &[String]) -> Result<Vec<String>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let dir = TempDir::new()?;
        let rc = dir.path().join("rc.json");
        let input = dir.path().join("input.md");
        std::fs::write(&rc, rc_json(self.prh.as_deref()).to_string()).map_err(|e| e.to_string())?;
        std::fs::write(&input, format!("{}\n", texts.join(SEP))).map_err(|e| e.to_string())?;

        let mut cmd = Command::new(&self.bin);
        cmd.arg("--config")
            .arg(&rc)
            .arg("--fix")
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child =
            spawn_group(&mut cmd).map_err(|e| format!("{} を起動できません: {e}", self.bin))?;
        let status =
            wait_with_timeout(&mut child, FIX_TIMEOUT).ok_or("textlint が時間内に終わりません")?;
        // 0: 指摘なし、1: 直せない指摘が残った(直した結果は書き込まれている)。それ以外は textlint 自身の失敗。
        match status.code() {
            Some(0) | Some(1) => {}
            other => return Err(format!("textlint が終了コード {other:?} で失敗しました")),
        }

        let out = std::fs::read_to_string(&input).map_err(|e| e.to_string())?;
        let parts: Vec<String> = out
            .trim_end_matches('\n')
            .split(SEP)
            .map(str::to_string)
            .collect();
        if parts.len() != texts.len() {
            return Err("段落の数が合いません".to_string());
        }
        Ok(parts)
    }
}

impl Textlint {
    /// 直さずに指摘を数える(`--format json` の `messages` の合計)。
    fn try_count(&self, texts: &[String]) -> Result<usize, String> {
        if texts.is_empty() {
            return Ok(0);
        }
        let dir = TempDir::new()?;
        let rc = dir.path().join("rc.json");
        let input = dir.path().join("input.md");
        std::fs::write(&rc, rc_json(self.prh.as_deref()).to_string()).map_err(|e| e.to_string())?;
        std::fs::write(&input, format!("{}\n", texts.join(SEP))).map_err(|e| e.to_string())?;

        let mut cmd = Command::new(&self.bin);
        cmd.arg("--config")
            .arg(&rc)
            .arg("--format")
            .arg("json")
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child =
            spawn_group(&mut cmd).map_err(|e| format!("{} を起動できません: {e}", self.bin))?;
        let mut out_pipe = child.stdout.take().expect("piped stdout");
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut out_pipe, &mut buf);
            buf
        });
        let status =
            wait_with_timeout(&mut child, FIX_TIMEOUT).ok_or("textlint が時間内に終わりません")?;
        let out = reader.join().unwrap_or_default();
        match status.code() {
            Some(0) | Some(1) => {}
            other => return Err(format!("textlint が終了コード {other:?} で失敗しました")),
        }
        let v: serde_json::Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
        Ok(v.as_array()
            .map(|files| {
                files
                    .iter()
                    .map(|f| f["messages"].as_array().map_or(0, Vec::len))
                    .sum()
            })
            .unwrap_or(0))
    }
}

impl Fixer for Textlint {
    fn fix(&self, texts: &[String]) -> Vec<String> {
        self.try_fix(texts).unwrap_or_else(|_| texts.to_vec())
    }

    fn issues(&self, texts: &[String]) -> Option<usize> {
        self.try_count(texts).ok()
    }
}

/// 一時ディレクトリ。落ちても残さないよう、`Drop` で消す。
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("ja-polish-textlint-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&dir).map_err(|e| format!("一時ディレクトリを作れません: {e}"))?;
        Ok(TempDir(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn rc_json_enables_only_the_redundant_expression_rule() {
        let v = rc_json(None);
        let preset = &v["rules"]["preset-ja-technical-writing"];
        assert_eq!(preset["ja-no-redundant-expression"], true);
        assert_eq!(preset["sentence-length"], false);
        assert_eq!(
            preset.as_object().unwrap().len(),
            1 + PRESET_RULES_OFF.len()
        );
        assert!(v["rules"].get("prh").is_none());
    }

    #[test]
    fn rc_json_adds_prh_with_the_dictionary_path() {
        let v = rc_json(Some(Path::new("/x/prh.yml")));
        assert_eq!(v["rules"]["prh"]["rulePaths"][0], "/x/prh.yml");
    }

    /// 偽の textlint: 最後の引数(入力ファイル)の「することができる」を「できる」に直す。
    fn fake_bin(dir: &Path, body: &str) -> String {
        let p = dir.join("textlint");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        // 書いた直後に別のテストのスレッドが fork すると、書き込み用の fd を子が引き継ぎ、exec が
        // ETXTBSY(os error 26)で失敗することがある。実行できるようになるまで待つ。
        for _ in 0..200 {
            match Command::new(&p)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => break,
            }
        }
        p.to_string_lossy().into_owned()
    }

    fn tl(bin: String) -> Textlint {
        Textlint { bin, prh: None }
    }

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fixes_all_paragraphs_in_one_run_and_keeps_their_order() {
        let dir = tempfile::tempdir().unwrap();
        // 引数: --config <rc> --fix <input>。入力は 4 つ目。
        let bin = fake_bin(
            dir.path(),
            "f=\"$4\"\nsed -i 's/することができる/できる/' \"$f\"\nexit 1",
        );
        let out = tl(bin).fix(&v(&[
            "検証することができる。",
            "そのまま。",
            "確認することができる。",
        ]));
        assert_eq!(out, v(&["検証できる。", "そのまま。", "確認できる。"]));
    }

    #[test]
    fn a_missing_binary_or_a_crash_returns_the_input_unchanged() {
        let input = v(&["検証することができる。"]);
        assert_eq!(tl("/no/such/textlint".to_string()).fix(&input), input);
        let dir = tempfile::tempdir().unwrap();
        let crash = fake_bin(dir.path(), "exit 2");
        assert_eq!(tl(crash).fix(&input), input);
    }

    #[test]
    fn a_changed_paragraph_count_returns_the_input_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        // 空行を足して、段落の数を変える。
        let bin = fake_bin(dir.path(), "printf '\\n\\nextra\\n' >> \"$4\"");
        let input = v(&["一つ目。", "二つ目。"]);
        assert_eq!(tl(bin).fix(&input), input);
    }

    #[test]
    fn no_fixer_and_empty_input_are_identity() {
        assert_eq!(NoFixer.fix(&v(&["あ"])), v(&["あ"]));
        assert!(tl("/no/such".to_string()).fix(&[]).is_empty());
    }

    #[test]
    fn the_temp_directory_is_removed_when_dropped() {
        let path = {
            let d = TempDir::new().unwrap();
            assert!(d.path().is_dir());
            d.path().to_path_buf()
        };
        assert!(!path.exists());
    }
}
