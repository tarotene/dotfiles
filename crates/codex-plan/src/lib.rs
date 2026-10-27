//! Model pairs and native permission-profile selection belong to this adapter.
//! Permissions, manual model choices, instructions and inference remain native.
pub mod advice;
pub mod catalog;
pub mod comparison;
pub mod experiment;
pub mod history;
pub mod runtime;
pub mod selection;
pub mod threads;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fs, io::Write, path::Path};

pub const MAX_MESSAGE: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub model: String,
    pub reasoning_effort: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub plan: Model,
    pub execute: Model,
    /// Optional bounded experiment on the execute effort (absent = none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<experiment::Experiment>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            plan: Model {
                model: "gpt-6.1-sol".into(),
                reasoning_effort: "medium".into(),
            },
            execute: Model {
                model: "gpt-6.1-sol".into(),
                reasoning_effort: "medium".into(),
            },
            experiment: None,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let settings: Self = toml::from_str(&text)
                    .context("invalid pair settings (both models and efforts are required)")?;
                if let Some(experiment) = &settings.experiment {
                    experiment
                        .validate()
                        .context("invalid experiment settings")?;
                }
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).context("read pair settings"),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("settings path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).context("atomically save model pair")?;
        Ok(())
    }

    /// Settings for a fresh launch: while an experiment is active the execute
    /// effort is drawn per launch. Returns the one-line notice to show, if any.
    pub fn for_new_launch(
        &self,
        today: &str,
        random: &mut dyn FnMut() -> Result<u8>,
    ) -> Result<(Self, Option<String>)> {
        // The relay needs only the pair; an experiment that is not drawing must
        // not make launch re-validate its (possibly stale) candidates.
        let mut drawn = self.clone();
        drawn.experiment = None;
        let Some(experiment) = self.experiment.as_ref().filter(|e| e.is_active(today)) else {
            return Ok((drawn, None));
        };
        let effort = experiment.pick(random)?.to_owned();
        let notice = format!(
            "codex-plan: 観測中: 今回の実行は {} {effort}(〜{})。やめるには `codex-plan experiment stop`",
            self.execute.model, experiment.until
        );
        drawn.execute.reasoning_effort = effort;
        Ok((drawn, Some(notice)))
    }

    /// Every candidate effort must be supported by the execute model.
    pub fn validate_experiment(&self, models: &[Value]) -> Result<()> {
        let Some(experiment) = self.experiment.as_ref().filter(|e| e.ended.is_none()) else {
            return Ok(());
        };
        for effort in &experiment.execute_efforts {
            let mut candidate = self.clone();
            candidate.execute.reasoning_effort = effort.clone();
            candidate.experiment = None;
            candidate.validate_models(models)?;
        }
        Ok(())
    }

    pub fn validate_models(&self, models: &[Value]) -> Result<()> {
        for (side, choice) in [
            ("Plan", &self.plan),
            ("Auto（実行／native Default）", &self.execute),
        ] {
            let model = models.iter().find(|m| m["model"].as_str() == Some(&choice.model))
                .with_context(|| format!("{side}: model {} is absent from native model/list; use codex-plan configure", choice.model))?;
            let efforts = model["supportedReasoningEfforts"]
                .as_array()
                .context("unsupported model/list protocol: missing supportedReasoningEfforts")?;
            if !efforts
                .iter()
                .any(|e| e["reasoningEffort"].as_str() == Some(&choice.reasoning_effort))
            {
                bail!(
                    "{side}: {} does not support reasoning effort {}",
                    choice.model,
                    choice.reasoning_effort
                );
            }
        }
        Ok(())
    }

    pub fn patch_presets(&self, response: &mut Value) -> Result<()> {
        let presets = response
            .pointer_mut("/result/data")
            .and_then(Value::as_array_mut)
            .context("unsupported collaborationMode/list protocol: missing result.data")?;
        for (mode, model) in [("plan", &self.plan), ("default", &self.execute)] {
            let matches: Vec<_> = presets.iter_mut().filter(|p| p["mode"] == mode).collect();
            if matches.len() != 1 {
                bail!("unsupported collaborationMode/list protocol: expected one {mode} preset");
            }
            let preset = matches
                .into_iter()
                .next()
                .unwrap()
                .as_object_mut()
                .context("mode preset must be an object")?;
            preset.insert("model".into(), Value::String(model.model.clone()));
            preset.insert(
                "reasoning_effort".into(),
                Value::String(model.reasoning_effort.clone()),
            );
        }
        Ok(())
    }
}

/// A string "1" is deliberately distinct from the numeric request ID 1.
#[derive(Debug, PartialEq, Eq, Hash)]
enum RequestId {
    String(String),
    Number(serde_json::Number),
}

fn request_id(message: &Value) -> Result<Option<RequestId>> {
    match message.get("id") {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(RequestId::String(s.clone()))),
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            Ok(Some(RequestId::Number(n.clone())))
        }
        _ => bail!("unsupported JSON-RPC request ID (expected string or integer)"),
    }
}

pub struct Relay {
    permissions: String,
    protected: bool,
    pending: HashMap<RequestId, ResponseKind>,
    threads: HashMap<String, ThreadState>,
}

enum ResponseKind {
    Presets,
    Start,
    Resume,
}

struct ThreadState {
    // Track the TUI's requested mode, not asynchronously delivered snapshots:
    // an older native notification must not roll back a newer user action.
    mode: String,
    pair_until_manual_update: Option<Model>,
}

impl Relay {
    pub fn new(permissions: String) -> Self {
        Self {
            permissions,
            protected: false,
            pending: HashMap::new(),
            threads: HashMap::new(),
        }
    }

    /// Use for an app-server whose parent supplied credentials. The fixed
    /// native TUI sends --cd for thread creation and cwd for each turn.
    pub fn protected(permissions: String) -> Self {
        Self {
            protected: true,
            ..Self::new(permissions)
        }
    }

    fn validate_protected_request(&self, method: &str, params: &Value) -> Result<()> {
        if matches!(
            method,
            "command/exec" | "process/exec" | "thread/shellCommand"
        ) {
            bail!("direct native commands are unavailable in a credential-bearing session; use native sandboxed tools");
        }
        if !matches!(
            method,
            "thread/start"
                | "thread/resume"
                | "thread/fork"
                | "thread/settings/update"
                | "turn/start"
        ) {
            return Ok(());
        }
        let params = params
            .as_object()
            .context("protected request requires object params")?;
        if let Some(config) = params.get("config").filter(|value| !value.is_null()) {
            let config = toml::Value::try_from(config).context("invalid per-thread config")?;
            if codex_launcher::overrides_protections(&config) {
                bail!("per-thread config overrides credential protections; relaunch after changing protected native settings");
            }
        }
        for (key, expected) in [
            ("approvalPolicy", "on-request"),
            ("approvalsReviewer", "auto_review"),
        ] {
            if params
                .get(key)
                .filter(|value| !value.is_null())
                .is_some_and(|value| value.as_str() != Some(expected))
            {
                bail!("{key} would override credential protections");
            }
        }
        let creation = matches!(method, "thread/start" | "thread/resume" | "thread/fork");
        if !creation {
            if params
                .get("sandboxPolicy")
                .is_some_and(|value| !value.is_null())
            {
                bail!("legacy sandboxPolicy would override credential protections; keep the validated named profile");
            }
            if params
                .get("permissions")
                .filter(|value| !value.is_null())
                .is_some_and(|value| value.as_str() != Some(self.permissions.as_str()))
            {
                bail!(
                    "permission profile changes require relaunch and fresh credential validation"
                );
            }
        }
        // Native resume/fork can restore a different saved cwd. A missing cwd
        // cannot be checked safely and is never replaced with the launch cwd.
        match params.get("cwd").filter(|value| !value.is_null()) {
            Some(cwd) => {
                let cwd = cwd
                    .as_str()
                    .context("protected cwd must be a path string")?;
                if !Path::new(cwd).is_absolute() {
                    bail!("protected cwd must be absolute");
                }
                let selected = codex_launcher::validate_working_directory(Path::new(cwd))
                    .map_err(anyhow::Error::msg)?;
                if selected != self.permissions {
                    bail!("native permission profile changed; relaunch to validate and acquire fresh credentials");
                }
            }
            None if creation || method == "turn/start" => {
                bail!("credential-bearing thread creation and turns require an explicit cwd; change to the intended directory in your terminal and relaunch codex-plan");
            }
            None => {}
        }
        Ok(())
    }

    pub fn from_client(&mut self, raw: &str, settings: &Settings) -> Result<Option<String>> {
        let mut value = parse_message(raw)?;
        let method = value["method"].as_str().unwrap_or_default().to_owned();
        if self.protected {
            self.validate_protected_request(&method, &value["params"])?;
        }
        let creation = matches!(
            method.as_str(),
            "thread/start" | "thread/resume" | "thread/fork"
        );
        if creation {
            let params = value["params"]
                .as_object_mut()
                .context("thread creation requires object params")?;
            if let Some(config) = params.get("config").filter(|c| !c.is_null()) {
                let config = config
                    .as_object()
                    .context("thread config overrides must be an object")?;
                if let Some(key) = config.keys().find(|key| legacy_permission_key(key)) {
                    bail!("thread config override {key} conflicts with native named permissions; remove the legacy sandbox override");
                }
            }
            // Remote TUI 0.164.0 projects profiles to the lossy legacy enum.
            // Re-select by native name, including on resume/fork. Native remains
            // responsible for resolving and enforcing the selected policy.
            params.remove("sandbox");
            params.insert(
                "permissions".into(),
                Value::String(self.permissions.clone()),
            );
        }
        let kind = match method.as_str() {
            "collaborationMode/list" => Some(ResponseKind::Presets),
            "thread/start" => Some(ResponseKind::Start),
            "thread/fork"
                if value.pointer("/params/experimentalPredictionMode")
                    != Some(&Value::Bool(true)) =>
            {
                Some(ResponseKind::Start)
            }
            "thread/resume" => Some(ResponseKind::Resume),
            _ => None,
        };
        if let Some(kind) = kind {
            let id = request_id(&value)?.context("tracked method requires an ID")?;
            if self.pending.insert(id, kind).is_some() {
                bail!("duplicate in-flight request ID");
            }
        }
        if matches!(method.as_str(), "thread/settings/update" | "turn/start") {
            if let Some(mode) = value
                .pointer("/params/collaborationMode/mode")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                let thread = value
                    .pointer("/params/threadId")
                    .and_then(Value::as_str)
                    .context("mode update is missing threadId")?;
                let state = self
                    .threads
                    .get_mut(thread)
                    .context("mode update before thread/start or thread/resume response")?;
                if state.mode != mode {
                    let model = match mode.as_str() {
                        "plan" => &settings.plan,
                        "default" => &settings.execute,
                        _ => bail!("unsupported collaboration mode: {mode}"),
                    };
                    state.mode = mode;
                    state.pair_until_manual_update = Some(model.clone());
                } else if method == "thread/settings/update" {
                    // /model and reasoning changes in the same mode are user
                    // choices. They take precedence until another mode switch.
                    state.pair_until_manual_update = None;
                }
                if let Some(model) = &state.pair_until_manual_update {
                    // A turn can be submitted before the TUI consumes the native
                    // settings notification, carrying stale Plan effort. Keep the
                    // pair until an explicit same-mode settings update, not merely
                    // until a notification is forwarded.
                    let params = value["params"]
                        .as_object_mut()
                        .context("invalid mode update params")?;
                    let mode_settings = params
                        .get_mut("collaborationMode")
                        .and_then(|c| c.get_mut("settings"))
                        .and_then(Value::as_object_mut)
                        .context("invalid collaborationMode.settings")?;
                    mode_settings.insert("model".into(), Value::String(model.model.clone()));
                    mode_settings.insert(
                        "reasoning_effort".into(),
                        Value::String(model.reasoning_effort.clone()),
                    );
                    for (key, content) in
                        [("model", &model.model), ("effort", &model.reasoning_effort)]
                    {
                        if params.contains_key(key) {
                            params.insert(key.into(), Value::String(content.clone()));
                        }
                    }
                    return Ok(Some(serde_json::to_string(&value)?));
                }
            }
        }
        if creation {
            Ok(Some(serde_json::to_string(&value)?))
        } else {
            Ok(None)
        }
    }

    /// Return None to forward the original bytes, including unknown fields.
    pub fn from_server(&mut self, raw: &str, settings: &Settings) -> Result<Option<String>> {
        let mut value = parse_message(raw)?;
        // Server requests use their own ID namespace. Never mistake an approval
        // request with the same ID for the response we are waiting for.
        if value.get("method").is_none() {
            if let Some(id) = request_id(&value)? {
                if let Some(kind) = self.pending.remove(&id) {
                    if value.get("error").is_none() {
                        match kind {
                            ResponseKind::Presets => {
                                settings.patch_presets(&mut value)?;
                                return Ok(Some(serde_json::to_string(&value)?));
                            }
                            ResponseKind::Start | ResponseKind::Resume => {
                                let thread = value
                                    .pointer("/result/thread/id")
                                    .and_then(Value::as_str)
                                    .context("thread response is missing thread.id")?;
                                let mode = match kind {
                                    ResponseKind::Resume => value
                                        .pointer("/result/collaborationMode/mode")
                                        .and_then(Value::as_str)
                                        .context(
                                            "resume response is missing saved collaborationMode",
                                        )?,
                                    _ => "default",
                                };
                                self.threads.insert(
                                    thread.to_owned(),
                                    ThreadState {
                                        mode: mode.to_owned(),
                                        pair_until_manual_update: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

fn legacy_permission_key(key: &str) -> bool {
    ["sandbox_mode", "sandbox_workspace_write"]
        .iter()
        .any(|legacy| key == *legacy || key.starts_with(&format!("{legacy}.")))
}

/// Read the selector from native's cwd-aware effective config, never a second
/// TOML parser or a hard-coded profile name. Native resolves profile contents.
pub fn selected_permissions(response: &Value) -> Result<String> {
    let config = response
        .pointer("/result/config")
        .and_then(Value::as_object)
        .context("unsupported config/read protocol: missing result.config")?;
    if let Some((key, _)) = config
        .iter()
        .find(|(key, value)| legacy_permission_key(key) && !value.is_null())
    {
        bail!("native config contains legacy {key}; use default_permissions and a named permissions profile");
    }
    config
        .get("default_permissions")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .context("native config/read requires a nonempty default_permissions profile name")
}

pub fn parse_message(raw: &str) -> Result<Value> {
    if raw.len() > MAX_MESSAGE {
        bail!("JSON-RPC message exceeds 256 MiB");
    }
    let value: Value = serde_json::from_str(raw).context("invalid JSON-RPC JSON")?;
    if !value.is_object() {
        bail!("JSON-RPC message must be an object");
    }
    request_id(&value)?;
    Ok(value)
}
