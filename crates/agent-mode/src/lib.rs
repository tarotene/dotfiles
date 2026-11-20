//! agent-mode — セッション単位の「モード」(`docs/agents/agent-mode.md`、ADR は同じ文書の冒頭)。
//!
//! 決定の記録(ADR・設計文書)に縛られない整理のような、**特定のセッションだけ**拘束の範囲を変えたい
//! 用途のために、宣言(`config/agents/modes/<名前>.toml`)1 つから、次の 3 つを展開して `claude` を exec する。
//!
//! 1. 読み取り禁止の設定(`--settings` にインラインで渡す `permissions.deny` の `Read(..)` と `Edit(..)`)。
//!    Claude Code の Read の deny は、同じパスへの Edit・Write も止める。
//! 2. 追記する文(`--append-system-prompt-file`)。宣言と同じディレクトリのファイル。
//! 3. 止める検査の束(`SKIP_<STEM>=1`)。名前は `agent_session::escape::ALL` が唯一の正本で、宣言には stem だけを
//!    書き、表に無い stem・セッション単位を持たない stem は起動を失敗させる(閉語彙)。
//!
//! 宣言が読めない・検証に落ちるときは、通常のセッションに倒さず、起動しない(`Outcome::Fail`)。
//!
//! `inject-gate` は、セッション開始時に文脈を注入する hook の包みで、モードが注入を止めているとき
//! (`INJECTION_ENV` が `off`)だけ、元のコマンドを実行せず無音で終わる。

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use agent_session::escape;
use serde::Deserialize;

/// 起動したモードの名前を子プロセスに伝える環境変数。
pub const MODE_ENV: &str = "DOTFILES_AGENT_MODE";
/// モードが文脈の注入を止めているとき `off` になる環境変数(`inject-gate` が読む)。
pub const INJECTION_ENV: &str = "DOTFILES_AGENT_MODE_INJECTION";
/// `INJECTION_ENV` の、止めている値。
pub const INJECTION_OFF: &str = "off";

/// 宣言の置き場(`$XDG_CONFIG_HOME` からの相対)。
const MODES_DIR: &str = "dotfiles/agent-modes";

/// セッション開始時の文脈の注入を、このモードで止めるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Injection {
    On,
    Off,
}

/// 読ませない・書かせない場所。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Records {
    /// gitignore 形式の glob(`**/docs/adr/**` など)。`Read(..)` と `Edit(..)` の規則になる。
    pub globs: Vec<String>,
}

/// モードの宣言(`<名前>.toml`)。未知のキーは受けない。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    pub description: String,
    /// 追記する文のファイル名(宣言と同じディレクトリ)。
    pub prompt: String,
    pub injection: Injection,
    /// 止める検査の stem(`escape::ALL` の `stem`)。
    pub skip: Vec<String>,
    pub records: Records,
}

/// 検証に通ったモード。
#[derive(Debug)]
pub struct Loaded {
    pub name: String,
    pub mode: Mode,
    pub prompt_path: PathBuf,
}

/// 展開の結果。
#[derive(Debug, PartialEq, Eq)]
pub struct Expansion {
    /// `claude --settings` に渡す JSON。
    pub settings: String,
    pub prompt_path: PathBuf,
    /// 子プロセスに足す環境変数(宣言順)。
    pub env: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    BadName(String),
    Read(PathBuf, String),
    Parse(PathBuf, String),
    UnknownStem(String),
    NoSessionScope(String),
    Duplicate(&'static str, String),
    BadGlob(String),
    BadPrompt(String),
    MissingPrompt(PathBuf),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadName(n) => write!(
                f,
                "モード名 `{n}` は使えません(小文字で始まり、小文字・数字・`-` だけ)"
            ),
            Error::Read(p, e) => write!(f, "宣言を読めません: {}: {e}", p.display()),
            Error::Parse(p, e) => write!(f, "宣言の書式が不正です: {}: {e}", p.display()),
            Error::UnknownStem(s) => write!(
                f,
                "skip の `{s}` は逃げ道の表(agent_session::escape::ALL)にありません"
            ),
            Error::NoSessionScope(s) => write!(
                f,
                "skip の `{s}` はセッション単位の逃げ道を持ちません(呼び出し単位だけの guard は束ねられない)"
            ),
            Error::Duplicate(what, v) => write!(f, "{what} に `{v}` が重複しています"),
            Error::BadGlob(g) => write!(
                f,
                "records.globs の `{g}` は使えません(空・空白・制御文字・括弧は不可)"
            ),
            Error::BadPrompt(p) => write!(
                f,
                "prompt `{p}` はファイル名だけを書いてください(`/`・`..` は不可)"
            ),
            Error::MissingPrompt(p) => write!(f, "prompt のファイルがありません: {}", p.display()),
        }
    }
}

impl std::error::Error for Error {}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_glob(glob: &str) -> bool {
    !glob.is_empty()
        && !glob
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '(' || c == ')')
}

/// 検証(純粋)。`prompt_exists` は、prompt のファイルが在るかを呼び出し側が答える。
fn validate(mode: &Mode, prompt_path: &Path, prompt_exists: bool) -> Result<(), Error> {
    let mut seen_stems: Vec<&str> = Vec::new();
    for stem in &mode.skip {
        let hatch = escape::ALL
            .iter()
            .find(|h| h.stem == stem)
            .ok_or_else(|| Error::UnknownStem(stem.clone()))?;
        if !hatch.session {
            return Err(Error::NoSessionScope(stem.clone()));
        }
        if seen_stems.contains(&stem.as_str()) {
            return Err(Error::Duplicate("skip", stem.clone()));
        }
        seen_stems.push(stem);
    }
    let mut seen_globs: Vec<&str> = Vec::new();
    for glob in &mode.records.globs {
        if !valid_glob(glob) {
            return Err(Error::BadGlob(glob.clone()));
        }
        if seen_globs.contains(&glob.as_str()) {
            return Err(Error::Duplicate("records.globs", glob.clone()));
        }
        seen_globs.push(glob);
    }
    let p = mode.prompt.as_str();
    if p.is_empty() || p.contains('/') || p.contains("..") {
        return Err(Error::BadPrompt(mode.prompt.clone()));
    }
    if !prompt_exists {
        return Err(Error::MissingPrompt(prompt_path.to_path_buf()));
    }
    Ok(())
}

/// `<dir>/<name>.toml` を読んで検証する。
pub fn load(dir: &Path, name: &str) -> Result<Loaded, Error> {
    if !valid_name(name) {
        return Err(Error::BadName(name.to_string()));
    }
    let path = dir.join(format!("{name}.toml"));
    let text =
        std::fs::read_to_string(&path).map_err(|e| Error::Read(path.clone(), e.to_string()))?;
    let mode: Mode =
        toml::from_str(&text).map_err(|e| Error::Parse(path.clone(), e.to_string()))?;
    let prompt_path = dir.join(&mode.prompt);
    validate(&mode, &prompt_path, prompt_path.is_file())?;
    Ok(Loaded {
        name: name.to_string(),
        mode,
        prompt_path,
    })
}

/// 宣言から、設定の JSON・prompt のパス・環境変数を展開する(純粋)。
pub fn expand(loaded: &Loaded) -> Expansion {
    let deny: Vec<String> = loaded
        .mode
        .records
        .globs
        .iter()
        .flat_map(|g| [format!("Read({g})"), format!("Edit({g})")])
        .collect();
    let settings = serde_json::json!({ "permissions": { "deny": deny } }).to_string();

    let mut env = vec![(MODE_ENV.to_string(), loaded.name.clone())];
    if loaded.mode.injection == Injection::Off {
        env.push((INJECTION_ENV.to_string(), INJECTION_OFF.to_string()));
    }
    for stem in &loaded.mode.skip {
        // 検証済み: 表にあり、セッション単位を持つ。
        if let Some(hatch) = escape::ALL.iter().find(|h| h.stem == stem) {
            env.push((hatch.session_name(), "1".to_string()));
        }
    }
    Expansion {
        settings,
        prompt_path: loaded.prompt_path.clone(),
        env,
    }
}

/// 子プロセスの起動の指示。
#[derive(Debug, PartialEq, Eq)]
pub struct Action {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// そのまま終了する(終了コード)。
    Exit(u8),
    /// 標準出力に書いて成功で終わる。
    Print(String),
    /// 起動する(exec)。
    Exec(Action),
    /// 起動しない。標準エラーに出して失敗で終わる。
    Fail(String),
}

const USAGE: &str = "使い方:
  agent-mode exec <モード名> [--dir <宣言のディレクトリ>] -- <プログラム> [引数...]
  agent-mode check <モード名> [--dir <宣言のディレクトリ>]
  agent-mode inject-gate -- <プログラム> [引数...]";

/// `exec`・`check` の引数: モード名と `--dir`、`--` 以降の起動するコマンド。
struct Parsed {
    name: String,
    dir: Option<PathBuf>,
    command: Vec<String>,
}

fn parse_mode_args(args: &[String]) -> Result<Parsed, String> {
    let name = args
        .first()
        .filter(|a| !a.starts_with('-'))
        .ok_or("モード名がありません")?
        .clone();
    let mut dir = None;
    let mut command = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => {
                let v = args.get(i + 1).ok_or("--dir に値がありません")?;
                dir = Some(PathBuf::from(v));
                i += 2;
            }
            "--" => {
                command = args[i + 1..].to_vec();
                break;
            }
            other => return Err(format!("未知の引数 `{other}`")),
        }
    }
    Ok(Parsed { name, dir, command })
}

fn modes_dir(
    explicit: Option<PathBuf>,
    get: &dyn Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, String> {
    if let Some(d) = explicit {
        return Ok(d);
    }
    process_env::Xdg::Config
        .resolve_with(get)
        .map(|base| base.join(MODES_DIR))
        .ok_or_else(|| "設定のディレクトリを決められません(HOME も XDG_CONFIG_HOME も無い)".into())
}

fn split_command(command: &[String]) -> Result<(PathBuf, &[String]), String> {
    match command.split_first() {
        Some((program, rest)) => Ok((PathBuf::from(program), rest)),
        None => Err("`--` のあとに起動するコマンドがありません".into()),
    }
}

/// エントリポイント(純粋。環境は `get` で引く)。
pub fn run(args: &[String], get: &dyn Fn(&str) -> Option<OsString>) -> Outcome {
    let Some((sub, rest)) = args.split_first() else {
        return Outcome::Fail(USAGE.to_string());
    };
    match sub.as_str() {
        "exec" | "check" => {
            let parsed = match parse_mode_args(rest) {
                Ok(p) => p,
                Err(e) => return Outcome::Fail(format!("agent-mode: {e}\n{USAGE}")),
            };
            let dir = match modes_dir(parsed.dir, get) {
                Ok(d) => d,
                Err(e) => return Outcome::Fail(format!("agent-mode: {e}")),
            };
            let loaded = match load(&dir, &parsed.name) {
                Ok(l) => l,
                Err(e) => return Outcome::Fail(format!("agent-mode: {e}")),
            };
            let expansion = expand(&loaded);
            if sub == "check" {
                let env: serde_json::Map<String, serde_json::Value> = expansion
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                let report = serde_json::json!({
                    "mode": loaded.name,
                    "description": loaded.mode.description,
                    "settings": serde_json::from_str::<serde_json::Value>(&expansion.settings)
                        .unwrap_or_default(),
                    "prompt": expansion.prompt_path,
                    "env": env,
                });
                return Outcome::Print(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&report).unwrap_or_default()
                ));
            }
            let (program, rest) = match split_command(&parsed.command) {
                Ok(v) => v,
                Err(e) => return Outcome::Fail(format!("agent-mode: {e}\n{USAGE}")),
            };
            // 起動するプログラムの直後に、モードのフラグを置く(利用者の引数はその後ろ)。
            let mut argv: Vec<OsString> = vec![
                "--settings".into(),
                expansion.settings.into(),
                "--append-system-prompt-file".into(),
                expansion.prompt_path.into(),
            ];
            argv.extend(rest.iter().map(OsString::from));
            Outcome::Exec(Action {
                program,
                args: argv,
                env: expansion.env,
            })
        }
        "inject-gate" => {
            let command = match rest.split_first() {
                Some((dd, command)) if dd == "--" => command,
                _ => return Outcome::Fail(format!("agent-mode: `--` がありません\n{USAGE}")),
            };
            // モードが注入を止めているときは、元の hook を実行せず、無音で終わる(文脈を足さない)。
            let off = get(INJECTION_ENV).is_some_and(|v| v == INJECTION_OFF);
            if off {
                return Outcome::Exit(0);
            }
            match split_command(command) {
                Ok((program, args)) => Outcome::Exec(Action {
                    program,
                    args: args.iter().map(OsString::from).collect(),
                    env: Vec::new(),
                }),
                Err(e) => Outcome::Fail(format!("agent-mode: {e}\n{USAGE}")),
            }
        }
        other => Outcome::Fail(format!("agent-mode: 未知のサブコマンド `{other}`\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn write_mode(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(format!("{name}.toml")), body).unwrap();
        std::fs::write(dir.join("p.md"), "追記する文").unwrap();
    }

    fn mode_toml(skip: &str, globs: &str) -> String {
        format!(
            "description = \"試験\"\nprompt = \"p.md\"\ninjection = \"off\"\nskip = [{skip}]\n\n[records]\nglobs = [{globs}]\n"
        )
    }

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).map(OsString::from)
    }

    #[test]
    fn expands_deny_rules_prompt_and_env_from_the_table() {
        let d = tempfile::tempdir().unwrap();
        write_mode(
            d.path(),
            "m",
            &mode_toml(
                "\"PLAN_PRECEDENT_GATE\", \"NEW_TOOL_GUARD\"",
                "\"**/docs/adr/**\"",
            ),
        );
        let e = expand(&load(d.path(), "m").unwrap());
        let settings: serde_json::Value = serde_json::from_str(&e.settings).unwrap();
        assert_eq!(
            settings["permissions"]["deny"],
            serde_json::json!(["Read(**/docs/adr/**)", "Edit(**/docs/adr/**)"])
        );
        assert_eq!(e.prompt_path, d.path().join("p.md"));
        assert_eq!(
            e.env,
            vec![
                (MODE_ENV.to_string(), "m".to_string()),
                (INJECTION_ENV.to_string(), INJECTION_OFF.to_string()),
                (escape::PLAN_PRECEDENT_GATE.session_name(), "1".to_string()),
                (escape::NEW_TOOL_GUARD.session_name(), "1".to_string()),
            ]
        );
    }

    #[test]
    fn injection_on_adds_no_injection_variable() {
        let d = tempfile::tempdir().unwrap();
        write_mode(
            d.path(),
            "m",
            &mode_toml("", "").replace("injection = \"off\"", "injection = \"on\""),
        );
        let e = expand(&load(d.path(), "m").unwrap());
        assert_eq!(e.env, vec![(MODE_ENV.to_string(), "m".to_string())]);
    }

    #[test]
    fn a_stem_outside_the_table_fails() {
        let d = tempfile::tempdir().unwrap();
        write_mode(d.path(), "m", &mode_toml("\"NO_SUCH_GATE\"", ""));
        assert_eq!(
            load(d.path(), "m").unwrap_err(),
            Error::UnknownStem("NO_SUCH_GATE".into())
        );
    }

    #[test]
    fn a_call_only_guard_cannot_be_bundled() {
        // RULESETS_WRITE_GUARD は呼び出し単位だけ(セッション単位を持たない)。
        let d = tempfile::tempdir().unwrap();
        write_mode(d.path(), "m", &mode_toml("\"RULESETS_WRITE_GUARD\"", ""));
        assert_eq!(
            load(d.path(), "m").unwrap_err(),
            Error::NoSessionScope("RULESETS_WRITE_GUARD".into())
        );
    }

    #[test]
    fn duplicates_and_bad_globs_fail() {
        let d = tempfile::tempdir().unwrap();
        write_mode(
            d.path(),
            "m",
            &mode_toml("\"NEW_TOOL_GUARD\", \"NEW_TOOL_GUARD\"", ""),
        );
        assert!(matches!(
            load(d.path(), "m").unwrap_err(),
            Error::Duplicate("skip", _)
        ));
        for bad in ["\"\"", "\"a b\"", "\"a)\"", "\"Read(x\""] {
            write_mode(d.path(), "m", &mode_toml("", bad));
            assert!(
                matches!(load(d.path(), "m").unwrap_err(), Error::BadGlob(_)),
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_keys_and_bad_names_and_bad_prompts_fail() {
        let d = tempfile::tempdir().unwrap();
        write_mode(
            d.path(),
            "m",
            &format!("{}extra = 1\n", mode_toml("", "")).replace("\n\n[records]", "\n[records]"),
        );
        // `extra` は [records] の中に入るので、records の未知のキーとして落ちる。
        assert!(matches!(load(d.path(), "m").unwrap_err(), Error::Parse(..)));
        for name in ["", "A", "../x", "-x", "a b", "1a"] {
            assert_eq!(
                load(d.path(), name).unwrap_err(),
                Error::BadName(name.into())
            );
        }
        write_mode(d.path(), "m", &mode_toml("", "").replace("p.md", "../p.md"));
        assert!(matches!(
            load(d.path(), "m").unwrap_err(),
            Error::BadPrompt(_)
        ));
        write_mode(d.path(), "m", &mode_toml("", "").replace("p.md", "none.md"));
        assert!(matches!(
            load(d.path(), "m").unwrap_err(),
            Error::MissingPrompt(_)
        ));
        assert!(matches!(
            load(d.path(), "absent").unwrap_err(),
            Error::Read(..)
        ));
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exec_puts_the_mode_flags_right_after_the_program() {
        let d = tempfile::tempdir().unwrap();
        write_mode(
            d.path(),
            "m",
            &mode_toml("\"NEW_TOOL_GUARD\"", "\"**/adr/**\""),
        );
        let out = run(
            &args(&[
                "exec",
                "m",
                "--dir",
                d.path().to_str().unwrap(),
                "--",
                "/bin/claude",
                "-p",
                "hello",
            ]),
            &no_env,
        );
        let Outcome::Exec(a) = out else {
            panic!("exec のはず: {out:?}")
        };
        assert_eq!(a.program, PathBuf::from("/bin/claude"));
        assert_eq!(a.args[0], OsString::from("--settings"));
        assert_eq!(a.args[2], OsString::from("--append-system-prompt-file"));
        assert_eq!(a.args[3], d.path().join("p.md").into_os_string());
        assert_eq!(
            &a.args[4..],
            [OsString::from("-p"), OsString::from("hello")]
        );
        assert!(a
            .env
            .contains(&(escape::NEW_TOOL_GUARD.session_name(), "1".to_string())));
    }

    #[test]
    fn exec_resolves_the_default_directory_from_xdg_config_home() {
        let d = tempfile::tempdir().unwrap();
        let modes = d.path().join(MODES_DIR);
        std::fs::create_dir_all(&modes).unwrap();
        write_mode(&modes, "m", &mode_toml("", ""));
        let get = env_of(&[("XDG_CONFIG_HOME", d.path().to_str().unwrap())]);
        let out = run(&args(&["exec", "m", "--", "claude"]), &get);
        assert!(matches!(out, Outcome::Exec(_)), "{out:?}");
    }

    #[test]
    fn a_broken_declaration_never_falls_back_to_a_plain_session() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().to_str().unwrap();
        for a in [
            args(&["exec", "typo", "--dir", dir, "--", "claude"]),
            args(&["exec", "m", "--dir", dir]),
            args(&["exec"]),
            args(&["bogus"]),
            args(&[]),
        ] {
            assert!(matches!(run(&a, &no_env), Outcome::Fail(_)), "{a:?}");
        }
    }

    #[test]
    fn check_prints_the_expansion_without_starting_anything() {
        let d = tempfile::tempdir().unwrap();
        write_mode(
            d.path(),
            "m",
            &mode_toml("\"NEW_TOOL_GUARD\"", "\"**/adr/**\""),
        );
        let out = run(
            &args(&["check", "m", "--dir", d.path().to_str().unwrap()]),
            &no_env,
        );
        let Outcome::Print(text) = out else {
            panic!("print のはず: {out:?}")
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["mode"], "m");
        assert_eq!(v["env"][escape::NEW_TOOL_GUARD.session_name()], "1");
    }

    #[test]
    fn inject_gate_is_silent_only_while_the_mode_turns_injection_off() {
        let off = env_of(&[(INJECTION_ENV, INJECTION_OFF)]);
        assert_eq!(
            run(&args(&["inject-gate", "--", "/bin/hook", "x"]), &off),
            Outcome::Exit(0)
        );
        let on = run(&args(&["inject-gate", "--", "/bin/hook", "x"]), &no_env);
        assert_eq!(
            on,
            Outcome::Exec(Action {
                program: PathBuf::from("/bin/hook"),
                args: vec![OsString::from("x")],
                env: Vec::new(),
            })
        );
        // 値が `off` 以外(空・`on`)なら止めない。
        let other = env_of(&[(INJECTION_ENV, "on")]);
        assert!(matches!(
            run(&args(&["inject-gate", "--", "/bin/hook"]), &other),
            Outcome::Exec(_)
        ));
        assert!(matches!(
            run(&args(&["inject-gate", "/bin/hook"]), &no_env),
            Outcome::Fail(_)
        ));
    }

    #[test]
    fn the_shipped_refactor_mode_is_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/agents/modes");
        let loaded = load(&dir, "refactor").unwrap();
        let e = expand(&loaded);
        assert!(e.env.contains(&(MODE_ENV.to_string(), "refactor".into())));
        assert!(!loaded.mode.records.globs.is_empty());
    }
}
