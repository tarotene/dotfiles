use codex_plan::{runtime::tui_args, ClientAction, Model, Relay, Settings};
use serde_json::json;

fn presets(id: serde_json::Value) -> String {
    json!({"id":id,"result":{"future":42,"data":[
        {"name":"Plan","mode":"plan","model":null,"reasoning_effort":"medium","future":{"keep":true}},
        {"name":"Default","mode":"default","model":null,"reasoning_effort":null},
        {"name":"Future","mode":"future","model":"unmanaged"}
    ]}}).to_string()
}

#[test]
fn thread_creation_keeps_named_permissions_instead_of_remote_legacy_sandbox() {
    for method in ["thread/start", "thread/resume", "thread/fork"] {
        let mut relay = Relay::new("custom-network".into());
        let raw = json!({"id":1,"method":method,"params":{
            "sandbox":"workspace-write", "permissions":null,
            "model":"saved-model", "effort":"high", "threadId":"saved",
            "config":{"model_reasoning_effort":"high"}, "future":{"keep":true}
        }});
        let patched = relay
            .forwarded(&raw.to_string(), &Settings::default())
            .unwrap()
            .expect("legacy remote sandbox must be replaced");
        let got: serde_json::Value = serde_json::from_str(&patched).unwrap();
        let mut expected = raw;
        expected["params"]
            .as_object_mut()
            .unwrap()
            .remove("sandbox");
        expected["params"]["permissions"] = json!("custom-network");
        assert_eq!(got, expected);
    }
}

#[test]
fn profile_is_native_selected_and_later_permission_changes_are_forwarded() {
    for profile in ["arbitrary-name", ":workspace", "profile.with.dots"] {
        let mut relay = Relay::new(profile.into());
        for (id, method) in ["thread/start", "thread/resume", "thread/fork"]
            .iter()
            .enumerate()
        {
            let raw = json!({"id":id,"method":method,"params":{"permissions":"old-saved-profile","sandbox":null}});
            let patched = relay
                .forwarded(&raw.to_string(), &Settings::default())
                .unwrap()
                .unwrap();
            let got: serde_json::Value = serde_json::from_str(&patched).unwrap();
            assert_eq!(got["params"]["permissions"], profile);
            assert!(got["params"].get("sandbox").is_none());
        }
        for method in ["thread/settings/update", "turn/start"] {
            let raw = json!({"id":10,"method":method,"params":{
                "permissions":"manually-selected", "sandboxPolicy":{"type":"readOnly"},
                "model":"manual", "future":true
            }})
            .to_string();
            assert!(relay
                .forwarded(&raw, &Settings::default())
                .unwrap()
                .is_none());
        }
    }
}

#[test]
fn contradictory_thread_config_is_rejected_before_forwarding() {
    for method in ["thread/start", "thread/resume", "thread/fork"] {
        for config in [
            json!({"sandbox_mode":"workspace-write"}),
            json!({"sandbox_workspace_write":{"network_access":true}}),
            json!({"sandbox_workspace_write.network_access":true}),
            json!({"sandbox_mode":null}),
            json!([]),
        ] {
            let mut relay = Relay::new("network".into());
            let raw = json!({"id":1,"method":method,"params":{"config":config}}).to_string();
            assert!(
                relay.forwarded(&raw, &Settings::default()).is_err(),
                "{raw}"
            );
            // No pending response state was recorded by the rejected request.
            let retry = json!({"id":1,"method":method,"params":{}}).to_string();
            assert!(relay.forwarded(&retry, &Settings::default()).is_ok());
        }
    }
}

#[test]
fn config_read_requires_a_valid_selector_and_no_legacy_override() {
    use codex_plan::selected_permissions;
    for bad in [
        json!({}),
        json!(null),
        json!({"default_permissions":null}),
        json!({"default_permissions":7}),
        json!({"default_permissions":"  "}),
        json!({"default_permissions":"ok","sandbox_mode":"workspace-write"}),
        json!({"default_permissions":"ok","sandbox_workspace_write":{}}),
    ] {
        assert!(selected_permissions(&json!({"result":{"config":bad}})).is_err());
    }
    assert!(selected_permissions(&json!({"result":{}})).is_err());
    assert_eq!(
        selected_permissions(&json!({"result":{"config":{
            "default_permissions":"chosen-by-native", "sandbox_mode":null,
            "sandbox_workspace_write":null,"future":true
        }}}))
        .unwrap(),
        "chosen-by-native"
    );
}

#[test]
fn patches_only_matched_responses_preserving_unknown_fields_and_id_types() {
    let mut relay = Relay::new("custom-network".into());
    let settings = Settings::default();
    relay
        .forwarded(
            r#"{"id":"1","method":"collaborationMode/list","params":{}}"#,
            &Settings::default(),
        )
        .unwrap();
    assert!(relay
        .from_server(&presets(json!(1)), &settings)
        .unwrap()
        .is_none());
    let approval = r#"{"id":"1","method":"item/commandExecution/requestApproval","params":{"sandbox":"read-only"}}"#;
    assert!(relay.from_server(approval, &settings).unwrap().is_none());
    let result: serde_json::Value = serde_json::from_str(
        &relay
            .from_server(&presets(json!("1")), &settings)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["id"], "1");
    assert_eq!(result["result"]["future"], 42);
    let data = &result["result"]["data"];
    assert_eq!(data[0]["model"], "gpt-6.1-sol");
    assert_eq!(data[0]["future"]["keep"], true);
    assert_eq!(data[1]["model"], "gpt-6.1-sol");
    assert_eq!(data[1]["reasoning_effort"], "medium");
    assert_eq!(data[2]["model"], "unmanaged");
    assert!(relay
        .from_server(&presets(json!("1")), &settings)
        .unwrap()
        .is_none());
}

#[test]
fn errors_unknown_notifications_and_manual_settings_are_not_rewritten() {
    let mut relay = Relay::new("custom-network".into());
    for raw in [
        r#"{"id":2,"method":"thread/settings/update","params":{"model":"manual","approvalPolicy":"on-request"}}"#,
        r#"{"id":3,"method":"turn/start","params":{"sandboxPolicy":{"type":"readOnly"}}}"#,
        r#"{"id":4,"result":{"unknown":true}}"#,
        r#"{"method":"unknown","params":{"model":"manual"}}"#,
    ] {
        relay.forwarded(raw, &Settings::default()).unwrap();
        assert!(relay
            .from_server(raw, &Settings::default())
            .unwrap()
            .is_none());
    }
    relay
        .forwarded(
            r#"{"id":1,"method":"collaborationMode/list"}"#,
            &Settings::default(),
        )
        .unwrap();
    assert!(relay
        .from_server(
            r#"{"id":1,"error":{"code":-1,"message":"no"}}"#,
            &Settings::default()
        )
        .unwrap()
        .is_none());
    assert!(relay
        .from_server(&presets(json!(1)), &Settings::default())
        .unwrap()
        .is_none());
}

#[test]
fn invalid_protocol_fails_before_forwarding() {
    let mut relay = Relay::new("custom-network".into());
    for bad in ["{", "[]", r#"{"id":1.5}"#, r#"{"id":null}"#] {
        assert!(relay.forwarded(bad, &Settings::default()).is_err());
    }
    relay
        .forwarded(
            r#"{"id":1,"method":"collaborationMode/list"}"#,
            &Settings::default(),
        )
        .unwrap();
    assert!(relay
        .from_server(r#"{"id":1,"result":{"data":[]}}"#, &Settings::default())
        .is_err());
}

#[test]
fn settings_round_trip_atomic_pair_and_only_missing_file_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("codex-plan/settings.toml");
    assert_eq!(Settings::load(&path).unwrap(), Settings::default());
    assert!(!path.exists());
    let selected = Settings {
        plan: Model {
            model: "chosen-plan".into(),
            reasoning_effort: "high".into(),
        },
        execute: Model {
            model: "chosen-execute".into(),
            reasoning_effort: "low".into(),
        },
        experiment: None,
    };
    selected.save(&path).unwrap();
    assert_eq!(Settings::load(&path).unwrap(), selected);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::write(&path, "[plan]\nmodel='partial'").unwrap();
    assert!(Settings::load(&path).is_err());
}

#[test]
fn model_catalog_requires_exact_id_and_supported_effort() {
    let settings = Settings::default();
    let mut models = vec![
        json!({"model":"gpt-6-astra","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}),
        json!({"model":"gpt-6.1-sol","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}),
    ];
    settings.validate_models(&models).unwrap();
    models[1]["supportedReasoningEfforts"][0]["reasoningEffort"] = json!("high");
    assert!(settings.validate_models(&models).is_err());
    models.pop();
    assert!(settings.validate_models(&models).is_err());
}

#[test]
fn resume_has_no_overrides_of_saved_model_or_effort() {
    let settings = Settings::default();
    let resume = tui_args(&settings, "unix:///tmp/test", Some("--last"), None);
    assert!(!resume
        .iter()
        .any(|a| a == "--model" || a.starts_with("model_reasoning_effort=")));
    assert!(resume
        .iter()
        .any(|a| a.starts_with("plan_mode_reasoning_effort=")));
    assert!(resume.ends_with(&["resume".into(), "--last".into()]));
    let new = tui_args(&settings, "unix:///tmp/test", None, Some("hello"));
    assert!(new.windows(2).any(|a| a == ["--model", "gpt-6.1-sol"]));
}

#[test]
fn large_plugin_response_is_not_truncated_or_reencoded() {
    let raw = json!({"id":15,"result":{"data":"x".repeat(20 * 1024 * 1024)}}).to_string();
    assert!(Relay::new("custom-network".into())
        .from_server(&raw, &Settings::default())
        .unwrap()
        .is_none());
}

fn resumed_relay() -> Relay {
    let mut relay = Relay::new("custom-network".into());
    let settings = Settings::default();
    relay
        .forwarded(
            r#"{"id":"resume","method":"thread/resume","params":{"threadId":"saved"}}"#,
            &settings,
        )
        .unwrap();
    // A saved non-default effort is not overwritten on resume.
    let saved = r#"{"id":"resume","result":{"thread":{"id":"saved"},"collaborationMode":{"mode":"plan","settings":{"model":"manual","reasoning_effort":"high"}}}}"#;
    assert!(relay.from_server(saved, &settings).unwrap().is_none());
    relay
}

fn update(method: &str, mode: &str, model: &str, effort: &str) -> String {
    json!({"id":99,"method":method,"params":{
        "threadId":"saved", "model":model,"effort":effort,
        "approvalPolicy":"on-request","sandboxPolicy":{"type":"readOnly"},
        "collaborationMode":{"mode":mode,"settings":{
            "model":model,"reasoning_effort":effort,"developer_instructions":"native instructions"}},
        "unknown":{"keep":true}
    }}).to_string()
}

#[test]
fn mode_change_restores_pair_and_stale_turn_while_manual_choice_is_preserved() {
    let settings = Settings::default();
    let mut relay = resumed_relay();
    let saved_turn = update("turn/start", "plan", "manual", "high");
    assert!(relay.forwarded(&saved_turn, &settings).unwrap().is_none());
    for mode in ["default", "plan"] {
        let raw = update("thread/settings/update", mode, "manual", "high");
        let patched = relay.forwarded(&raw, &settings).unwrap().unwrap();
        let actual: serde_json::Value = serde_json::from_str(&patched).unwrap();
        let expected_model = "gpt-6.1-sol";
        assert_eq!(actual["params"]["model"], expected_model);
        assert_eq!(actual["params"]["effort"], "medium");
        let mut expected: serde_json::Value = serde_json::from_str(&raw).unwrap();
        expected["params"]["model"] = json!(expected_model);
        expected["params"]["effort"] = json!("medium");
        expected["params"]["collaborationMode"]["settings"]["model"] = json!(expected_model);
        expected["params"]["collaborationMode"]["settings"]["reasoning_effort"] = json!("medium");
        assert_eq!(actual, expected); // permissions, instructions, unknowns untouched
                                      // A delayed old notification must not undo the newer client-side mode.
        assert!(relay.from_server(r#"{"method":"thread/settings/updated","params":{"threadId":"saved","threadSettings":{"collaborationMode":{"mode":"plan"}}}}"#, &settings).unwrap().is_none());
        let stale = update("turn/start", mode, "manual", "high");
        assert!(relay
            .forwarded(&stale, &settings)
            .unwrap()
            .unwrap()
            .contains("medium"));
    }
    let manual = update("thread/settings/update", "plan", "manual", "high");
    assert!(relay.forwarded(&manual, &settings).unwrap().is_none());
    assert!(relay.forwarded(&saved_turn, &settings).unwrap().is_none());
}

#[test]
fn failed_start_does_not_invent_thread_state_or_patch_errors() {
    let mut relay = Relay::new("custom-network".into());
    let settings = Settings::default();
    relay
        .forwarded(r#"{"id":1,"method":"thread/start","params":{}}"#, &settings)
        .unwrap();
    assert!(relay
        .from_server(
            r#"{"id":1,"error":{"code":-1,"message":"denied"}}"#,
            &settings
        )
        .unwrap()
        .is_none());
    assert!(relay
        .forwarded(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .is_err());
}

#[test]
fn rejected_switch_does_not_undo_tui_mode_or_steal_later_manual_choice() {
    let mut relay = resumed_relay();
    let settings = Settings::default();
    relay
        .forwarded(
            &update("thread/settings/update", "default", "manual", "high"),
            &settings,
        )
        .unwrap();
    let error = r#"{"id":99,"error":{"code":-1,"message":"unavailable"}}"#;
    assert!(relay.from_server(error, &settings).unwrap().is_none());
    assert!(relay
        .forwarded(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_some());
    assert!(relay
        .forwarded(
            &update("thread/settings/update", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_none());
    assert!(relay
        .forwarded(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_none());
}

#[test]
fn standard_fork_starts_in_default_mode() {
    let mut relay = Relay::new("custom-network".into());
    let settings = Settings::default();
    relay
        .forwarded(
            r#"{"id":1,"method":"thread/fork","params":{"threadId":"parent"}}"#,
            &settings,
        )
        .unwrap();
    relay
        .from_server(
            r#"{"id":1,"result":{"thread":{"id":"saved"},"model":"manual"}}"#,
            &settings,
        )
        .unwrap();
    assert!(relay
        .forwarded(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_none());
    assert!(relay
        .forwarded(
            &update("thread/settings/update", "plan", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_some());
}

#[test]
fn credential_relay_rechecks_native_protections_before_forwarding() {
    const CONFIG: &str = r#"default_permissions = "fixture"
approval_policy = "on-request"
approvals_reviewer = "auto_review"
[features]
network_proxy = true
credential_masking = true
[permissions.fixture]
extends = ":workspace"
[permissions.fixture.network]
enabled = true
"#;
    if std::env::var_os("PROTECTED_RELAY_CHILD").is_none() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join(".codex")).unwrap();
        std::fs::write(directory.path().join(".codex/config.toml"), CONFIG).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "credential_relay_rechecks_native_protections_before_forwarding",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", directory.path())
            .env("PROTECTED_RELAY_CHILD", "1")
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let request = |method: &str, extra: serde_json::Value| {
        let mut params = json!({"cwd":home,"threadId":"saved"});
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"id":1,"method":method,"params":params}).to_string()
    };
    for method in [
        "thread/start",
        "thread/resume",
        "thread/fork",
        "thread/settings/update",
        "turn/start",
    ] {
        for config in [
            json!({"features":{"network_proxy":false}}),
            json!({"features":{"credential_masking":false}}),
            json!({"features.credential_masking":false}),
            json!({"permissions.fixture.network.enabled":false}),
            json!({"permissions":{"fixture":{"extends":":danger-full-access"}}}),
            json!({"profiles":{"unsafe":{"features":{"credential_masking":false}}}}),
            json!({"shell_environment_policy":{"set":{"Gh_Token":"conflict"}}}),
            json!({"approval_policy":"never"}),
            json!({"sandbox_mode":"danger-full-access"}),
        ] {
            let mut relay = Relay::protected("fixture".into());
            assert!(relay
                .from_client(
                    &request(method, json!({"config":config})),
                    &Settings::default()
                )
                .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
            // Rejected calls must not reserve the request ID.
            assert!(relay
                .from_client(
                    &request(method, json!({"model":"manual","effort":"high"})),
                    &Settings::default()
                )
                .is_ok_and(|action| matches!(action, ClientAction::Forward(_))));
        }
        for extra in [
            json!({"approvalPolicy":"never"}),
            json!({"approvalsReviewer":"user"}),
            json!({"cwd":"relative"}),
        ] {
            let mut relay = Relay::protected("fixture".into());
            assert!(relay
                .from_client(&request(method, extra), &Settings::default())
                .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
        }
    }
    for method in ["thread/start", "thread/resume", "thread/fork", "turn/start"] {
        let mut relay = Relay::protected("fixture".into());
        assert!(relay
            .from_client(&request(method, json!({"cwd":null})), &Settings::default())
            .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
    }
    for method in ["thread/settings/update", "turn/start"] {
        for extra in [
            json!({"permissions":":danger-full-access"}),
            json!({"sandboxPolicy":{"type":"dangerFullAccess"}}),
        ] {
            let mut relay = Relay::protected("fixture".into());
            assert!(relay
                .from_client(&request(method, extra), &Settings::default())
                .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
        }
        let mut relay = Relay::protected("fixture".into());
        assert!(relay
            .from_client(
                &request(
                    method,
                    json!({"permissions":"fixture","model":"manual","effort":"high"})
                ),
                &Settings::default()
            )
            .is_ok_and(|action| matches!(action, ClientAction::Forward(_))));
    }
    for method in ["command/exec", "process/exec", "thread/shellCommand"] {
        assert!(Relay::protected("fixture".into())
            .from_client(&request(method, json!({})), &Settings::default())
            .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
    }
    let project = home.join("project");
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::write(
        project.join(".codex/config.toml"),
        "[features]\ncredential_masking = false\n",
    )
    .unwrap();
    assert!(Relay::protected("fixture".into())
        .from_client(
            &request("thread/resume", json!({"cwd":project})),
            &Settings::default()
        )
        .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
    for key in ["GH_TOKEN", "Gh_ToKeN", "GITHUB_ENTERPRISE_TOKEN"] {
        std::fs::write(
            home.join(".codex/config.toml"),
            format!("{CONFIG}\n[shell_environment_policy.set]\n{key} = \"synthetic-override\"\n"),
        )
        .unwrap();
        assert!(Relay::protected("fixture".into())
            .from_client(&request("turn/start", json!({})), &Settings::default())
            .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
    }
    std::fs::write(
        home.join(".codex/config.toml"),
        format!("{CONFIG}\n[shell_environment_policy]\ninclude_only = [\"HOME\", \"GH_TOKEN\"]\n"),
    )
    .unwrap();
    assert!(Relay::protected("fixture".into())
        .from_client(&request("thread/start", json!({})), &Settings::default())
        .is_ok_and(|action| matches!(action, ClientAction::Forward(_))));
    // Protection is checked again on every turn, not just once at startup.
    std::fs::write(
        home.join(".codex/config.toml"),
        CONFIG.replace("credential_masking = true", "credential_masking = false"),
    )
    .unwrap();
    assert!(Relay::protected("fixture".into())
        .from_client(&request("turn/start", json!({})), &Settings::default())
        .is_ok_and(|action| matches!(action, ClientAction::Reply(_))));
}

// Model/permission transformation tests distinguish unchanged bytes from patches.
trait Forwarded {
    fn forwarded(&mut self, raw: &str, settings: &Settings) -> anyhow::Result<Option<String>>;
}
impl Forwarded for Relay {
    fn forwarded(&mut self, raw: &str, settings: &Settings) -> anyhow::Result<Option<String>> {
        match self.from_client(raw, settings)? {
            ClientAction::Forward(message) => Ok((message != raw).then_some(message)),
            ClientAction::Reply(reply) => anyhow::bail!("unexpected local refusal: {reply}"),
        }
    }
}

#[test]
fn local_refusals_preserve_ids_do_not_echo_params_or_reserve_pending_ids() {
    for id in [json!(42), json!("42")] {
        let mut relay = Relay::protected("fixture".into());
        let raw =
            json!({"id":id,"method":"command/exec","params":{"command":["synthetic-secret"]}})
                .to_string();
        let ClientAction::Reply(reply) = relay.from_client(&raw, &Settings::default()).unwrap()
        else {
            panic!("refused command was forwarded")
        };
        let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["error"]["code"], -32600);
        assert!(!reply.to_string().contains("synthetic-secret"));
        let request = json!({"id":id,"method":"collaborationMode/list","params":{}}).to_string();
        assert_eq!(
            relay.from_client(&request, &Settings::default()).unwrap(),
            ClientAction::Forward(request)
        );
        assert!(relay
            .from_server(&presets(id), &Settings::default())
            .unwrap()
            .is_some());
    }
    for raw in [
        r#"{"method":"command/exec","params":{}}"#,
        r#"{"id":null,"method":"command/exec","params":{}}"#,
        r#"{"id":true,"method":"command/exec","params":{}}"#,
        "{",
    ] {
        assert!(Relay::protected("fixture".into())
            .from_client(raw, &Settings::default())
            .is_err());
    }
}
