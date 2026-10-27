//! Experiment commands and the per-launch draw, against the real binary and a
//! tiny native-process fixture. No credentials or model requests.
mod common;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

const NATIVE: &str = r#"#!/bin/sh
if [ "$1" = --version ]; then echo "codex-cli 0.164.0"; exit; fi
if [ "$1" = --config ]; then shift; shift; fi
if [ "$1" = app-server ]; then
  while IFS= read -r line; do
    case "$line" in
      *'"method":"initialize"'*) echo '{"id":0,"result":{"userAgent":"fixture"}}' ;;
      *'"method":"model/list"'*) echo '{"id":1,"result":{"nextCursor":null,"data":[{"model":"gpt-6.1-sol","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}]}]}}' ;;
      *'"method":"config/read"'*) echo '{"id":4,"result":{"config":{"default_permissions":"fixture"}}}' ;;
      *'"method":"collaborationMode/list"'*) echo '{"id":2,"result":{"data":[{"mode":"plan"},{"mode":"default"}]}}' ;;
      *'"method":"account/rateLimits/read"'*) echo '{"id":3,"result":{"rateLimits":{}}}' ;;
    esac
  done
else
  printf '%s\n' "$@" > "$FIXTURE_DIR/args"
  exit 42
fi
"#;

struct Fixture {
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        common::credentials(dir.path());
        let native = dir.path().join(".local/bin/codex");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, NATIVE).unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.path().join("codex"), "#!/bin/sh\nexit 91\n").unwrap();
        fs::set_permissions(dir.path().join("codex"), fs::Permissions::from_mode(0o700)).unwrap();
        Self { dir }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_codex-plan"));
        c.env_remove("CODEX_HOME")
            .env_remove("CODEX_PROFILE")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("HOME", self.dir.path())
            .env("PATH", self.dir.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .env("FIXTURE_DIR", self.dir.path());
        c
    }
    fn settings(&self) -> std::path::PathBuf {
        self.dir.path().join("config/codex-plan/settings.toml")
    }
    fn write_settings(&self, text: &str) {
        fs::create_dir_all(self.settings().parent().unwrap()).unwrap();
        fs::write(self.settings(), text).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn args_file(&self) -> String {
        fs::read_to_string(self.dir.path().join("args")).unwrap_or_default()
    }
}

fn today() -> String {
    codex_plan::history::local_day(codex_plan::comparison::now()).unwrap()
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

const PAIR: &str = "[plan]\nmodel = 'gpt-6.1-sol'\nreasoning_effort = 'medium'\n[execute]\nmodel = 'gpt-6.1-sol'\nreasoning_effort = 'medium'\n";

#[test]
fn start_validates_candidates_against_native_and_keeps_the_pair() {
    let f = Fixture::new();
    f.write_settings(PAIR);
    let before = fs::read(f.settings()).unwrap();
    for args in [
        // ultra is not offered by the fixture's native model/list.
        vec![
            "experiment",
            "start",
            "--execute-efforts",
            "low,ultra",
            "--until",
            "2999-12-31",
        ],
        vec![
            "experiment",
            "start",
            "--execute-efforts",
            "low",
            "--until",
            "2999-12-31",
        ],
        vec![
            "experiment",
            "start",
            "--execute-efforts",
            "low,medium",
            "--until",
            "soon",
        ],
        vec!["experiment", "start", "--execute-efforts", "low,medium"],
    ] {
        let out = f.run(&args);
        assert!(!out.status.success(), "{args:?}");
        assert_eq!(fs::read(f.settings()).unwrap(), before, "{args:?}");
    }
    let out = f.run(&[
        "experiment",
        "start",
        "--execute-efforts",
        "low,medium",
        "--until",
        "2999-12-31",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let saved = codex_plan::Settings::load(&f.settings()).unwrap();
    let experiment = saved.experiment.unwrap();
    assert_eq!(experiment.start, today());
    assert_eq!(experiment.until, "2999-12-31");
    assert_eq!(experiment.ended, None);
    assert_eq!(saved.execute.reasoning_effort, "medium", "pair untouched");
    assert!(text(&out.stdout).contains("running"));
    let status = f.run(&["status"]);
    assert!(text(&status.stdout).contains("experiment: running"));
}

#[test]
fn running_start_extends_same_candidates_only_and_stop_keeps_the_period() {
    let f = Fixture::new();
    f.write_settings(PAIR);
    let start = [
        "experiment",
        "start",
        "--execute-efforts",
        "low,medium",
        "--until",
        "2999-12-30",
    ];
    assert!(f.run(&start).status.success());
    // Different candidates cannot replace a running experiment.
    let before = fs::read(f.settings()).unwrap();
    let other = f.run(&[
        "experiment",
        "start",
        "--execute-efforts",
        "medium,low",
        "--until",
        "2999-12-31",
    ]);
    assert!(!other.status.success());
    assert!(text(&other.stderr).contains("already running"));
    assert_eq!(fs::read(f.settings()).unwrap(), before);
    // Same candidates extend the last day; start (the assigned period) is kept.
    let extend = f.run(&[
        "experiment",
        "start",
        "--execute-efforts",
        "low,medium",
        "--until",
        "2999-12-31",
    ]);
    assert!(extend.status.success(), "{}", text(&extend.stderr));
    let extended = codex_plan::Settings::load(&f.settings())
        .unwrap()
        .experiment
        .unwrap();
    assert_eq!(
        (extended.start.as_str(), extended.until.as_str()),
        (today().as_str(), "2999-12-31")
    );

    let stop = f.run(&["experiment", "stop"]);
    assert!(stop.status.success(), "{}", text(&stop.stderr));
    let saved = codex_plan::Settings::load(&f.settings()).unwrap();
    let experiment = saved.experiment.clone().unwrap();
    assert_eq!(experiment.ended.as_deref(), Some(today().as_str()));
    assert_eq!(
        experiment.start,
        today(),
        "period kept for later aggregation"
    );
    assert_eq!(saved.execute.reasoning_effort, "medium");
    // Idempotent: a second stop changes nothing.
    let after_stop = fs::read(f.settings()).unwrap();
    assert!(f.run(&["experiment", "stop"]).status.success());
    assert_eq!(fs::read(f.settings()).unwrap(), after_stop);
    // A stopped experiment can be started again, as a new assigned period.
    assert!(f.run(&start).status.success());
}

#[test]
fn stop_without_an_experiment_fails_and_writes_nothing() {
    let f = Fixture::new();
    let out = f.run(&["experiment", "stop"]);
    assert!(!out.status.success());
    assert!(!f.settings().exists());
}

#[test]
fn configure_keeps_a_running_experiment() {
    let f = Fixture::new();
    f.write_settings(PAIR);
    assert!(f
        .run(&[
            "experiment",
            "start",
            "--execute-efforts",
            "low,medium",
            "--until",
            "2999-12-31"
        ])
        .status
        .success());
    let out = f.run(&[
        "configure",
        "--plan-model",
        "gpt-6.1-sol",
        "--plan-effort",
        "medium",
        "--execute-model",
        "gpt-6.1-sol",
        "--execute-effort",
        "low",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let saved = codex_plan::Settings::load(&f.settings()).unwrap();
    assert_eq!(saved.execute.reasoning_effort, "low");
    assert!(saved.experiment.is_some());
}

#[test]
fn invalid_experiment_settings_are_refused_not_defaulted() {
    let f = Fixture::new();
    f.write_settings(&format!(
        "{PAIR}[experiment]\nexecute_efforts = ['low', 'medium']\nstart = '2026-13-01'\nuntil = '2999-12-31'\n"
    ));
    let out = f.run(&["status"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("experiment"));
}

#[test]
fn fresh_launch_draws_from_the_candidates_and_says_so() {
    let f = Fixture::new();
    f.write_settings(&format!(
        "{PAIR}[experiment]\nexecute_efforts = ['low', 'medium']\nstart = '2026-10-27'\nuntil = '2999-12-31'\n"
    ));
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..24 {
        let out = f.run(&[]);
        assert_eq!(out.status.code(), Some(42), "{}", text(&out.stderr));
        assert!(
            text(&out.stderr).contains("観測中"),
            "{}",
            text(&out.stderr)
        );
        let args = f.args_file();
        let effort = ["low", "medium"]
            .into_iter()
            .find(|e| args.contains(&format!("model_reasoning_effort=\"{e}\"")))
            .unwrap_or_else(|| panic!("no candidate effort in {args}"));
        assert!(text(&out.stderr).contains(&format!("gpt-6.1-sol {effort}")));
        seen.insert(effort);
        assert!(
            args.contains("plan_mode_reasoning_effort=\"medium\""),
            "plan is not drawn"
        );
        if seen.len() == 2 {
            break;
        }
    }
    // 24 fair draws miss one candidate with probability 2^-23.
    assert_eq!(seen.len(), 2, "both candidates must be reachable");
}

#[test]
fn expired_or_stopped_experiments_and_resume_do_not_draw() {
    let f = Fixture::new();
    // Expired (until in the past) and stopped both keep the saved pair.
    for experiment in [
        "start = '2000-01-01'\nuntil = '2000-01-02'\n",
        "start = '2026-10-27'\nuntil = '2999-12-31'\nended = '2026-10-28'\n",
    ] {
        f.write_settings(&format!(
            "{PAIR}[experiment]\nexecute_efforts = ['low', 'low2']\n{experiment}"
        ));
        let out = f.run(&[]);
        assert_eq!(out.status.code(), Some(42), "{}", text(&out.stderr));
        assert!(!text(&out.stderr).contains("観測中"));
        assert!(f.args_file().contains("model_reasoning_effort=\"medium\""));
    }
    // Resume must not pass a model/effort override (native restores the thread's own).
    f.write_settings(&format!(
        "{PAIR}[experiment]\nexecute_efforts = ['low', 'medium']\nstart = '2026-10-27'\nuntil = '2999-12-31'\n"
    ));
    let out = f.run(&["resume", "--last"]);
    assert_eq!(out.status.code(), Some(42), "{}", text(&out.stderr));
    assert!(!text(&out.stderr).contains("観測中"));
    assert!(!f.args_file().contains("model_reasoning_effort"));
}

fn write_rollout(dir: &Path, name: &str, lines: &[String]) {
    let path = dir.join(".codex/sessions/2026/10/27").join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, lines.join("\n")).unwrap();
}

#[test]
fn compare_reports_arms_without_leaking_tool_text() {
    let f = Fixture::new();
    f.write_settings(PAIR);
    let now = codex_plan::comparison::now();
    let stamp = |secs: i64| {
        // RFC 3339 UTC from epoch seconds (no chrono dependency in this crate).
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    };
    let begin = now - 600;
    let record = |at: i64, kind: &str, payload: String| {
        format!(
            r#"{{"timestamp":"{}","type":"{kind}","payload":{payload}}}"#,
            stamp(at)
        )
    };
    let thread = |id: &str, effort: &str| -> Vec<String> {
        vec![
            record(begin, "session_meta", format!(r#"{{"id":"{id}","timestamp":"{}"}}"#, stamp(begin))),
            record(begin + 1, "event_msg", format!(
                r#"{{"type":"task_started","turn_id":"{id}-t","started_at":{},"turn_attribution":{{"turn_trigger":"user"}}}}"#,
                begin + 1
            )),
            record(begin + 1, "turn_context", format!(
                r#"{{"turn_id":"{id}-t","model":"gpt-6.1-sol","effort":"{effort}","collaboration_mode":{{"mode":"default"}}}}"#
            )),
            record(begin + 5, "response_item", r#"{"type":"custom_tool_call","call_id":"c","name":"exec","input":"gh pr create --body SECRET-BODY"}"#.into()),
            record(begin + 9, "response_item", r#"{"type":"custom_tool_call_output","call_id":"c","output":[{"type":"input_text","text":"SECRET-OUT\nhttps://github.com/o/r/pull/12\n"}]}"#.into()),
            record(begin + 20, "event_msg", format!(
                r#"{{"type":"task_complete","turn_id":"{id}-t","duration_ms":19000,"time_to_first_token_ms":2500}}"#
            )),
        ]
    };
    write_rollout(f.dir.path(), "rollout-a.jsonl", &thread("a", "low"));
    write_rollout(f.dir.path(), "rollout-b.jsonl", &thread("b", "medium"));
    let out = f.run(&["compare", "--json", "--days", "3"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        !text(&out.stdout).contains("SECRET"),
        "tool text must never reach the report"
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["schema_version"], 2);
    let arms = &report["experiment_arms"];
    assert_eq!(arms["threads_read"], 2);
    let arm = |effort: &str| {
        arms["arms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["effort"] == effort)
            .cloned()
            .unwrap_or_else(|| panic!("no {effort} arm in {arms}"))
    };
    for effort in ["low", "medium"] {
        let a = arm(effort);
        assert_eq!(a["assignment"], "observed", "no experiment was configured");
        assert_eq!(a["threads_with_pr"], 1);
        assert_eq!(a["sufficient"], false, "one PR is not enough to compare");
        // PR output at begin+9 in a turn started at begin+1 = 8 s of agent time.
        assert_eq!(a["to_pr_agent_ms"]["median"], 8000);
        assert_eq!(a["to_pr_wall_ms"]["median"], 8000);
        assert_eq!(a["to_pr_user_turns"]["median"], 1);
        assert_eq!(a["default_ttft_ms"]["median"], 2500);
    }
    let human = f.run(&["compare", "--days", "3"]);
    assert!(text(&human.stdout).contains("件数不足"));
    assert!(!text(&human.stdout).contains("SECRET"));
}

#[test]
fn drawn_effort_reaches_the_tui_and_presets_but_never_plan_or_resume() {
    use codex_plan::{experiment::Experiment, runtime::tui_args, Settings};
    let settings = Settings {
        experiment: Some(
            Experiment::start(
                vec!["low".into(), "medium".into()],
                "2026-10-27",
                "2026-11-01",
            )
            .unwrap(),
        ),
        ..Settings::default()
    };
    for (byte, want) in [(0u8, "low"), (1u8, "medium")] {
        let (drawn, notice) = settings
            .for_new_launch("2026-10-31", &mut || Ok(byte))
            .unwrap();
        let notice = notice.expect("an active experiment announces the draw");
        assert!(notice.contains(&format!("gpt-6.1-sol {want}")), "{notice}");
        assert!(drawn.experiment.is_none(), "launch needs only the pair");
        let args = tui_args(&drawn, "unix:///x", None, Some("do it"));
        assert!(
            args.contains(&format!("model_reasoning_effort=\"{want}\"")),
            "{args:?}"
        );
        assert!(args.contains(&"plan_mode_reasoning_effort=\"medium\"".to_string()));
        // Mode switches restore the drawn pair, not the saved one.
        let mut modes = serde_json::json!({"result":{"data":[{"mode":"plan"},{"mode":"default"}]}});
        drawn.patch_presets(&mut modes).unwrap();
        assert_eq!(modes["result"]["data"][1]["reasoning_effort"], want);
        assert_eq!(modes["result"]["data"][0]["reasoning_effort"], "medium");
        // A resumed thread keeps its own effort: no override at all.
        let resumed = tui_args(&drawn, "unix:///x", Some("id"), None);
        assert!(
            !resumed.iter().any(|a| a.contains("model_reasoning_effort")),
            "{resumed:?}"
        );
    }
    // After `until`, or once stopped, the saved pair is used unchanged and unannounced.
    let (late, notice) = settings
        .for_new_launch("2026-11-02", &mut || Ok(1))
        .unwrap();
    assert_eq!(
        (late.execute.reasoning_effort.as_str(), notice),
        ("medium", None)
    );
    let mut stopped = settings.clone();
    stopped.experiment.as_mut().unwrap().ended = Some("2026-10-28".into());
    let (kept, notice) = stopped.for_new_launch("2026-10-29", &mut || Ok(0)).unwrap();
    assert_eq!(
        (kept.execute.reasoning_effort.as_str(), notice),
        ("medium", None)
    );
    // A failing entropy source is an error, never a silent default draw.
    assert!(settings
        .for_new_launch("2026-10-31", &mut || anyhow::bail!("no entropy"))
        .is_err());
}
