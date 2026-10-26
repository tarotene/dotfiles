use codex_plan::{runtime::tui_args, Model, Relay, Settings};
use serde_json::json;

fn presets(id: serde_json::Value) -> String {
    json!({"id":id,"result":{"future":42,"data":[
        {"name":"Plan","mode":"plan","model":null,"reasoning_effort":"medium","future":{"keep":true}},
        {"name":"Default","mode":"default","model":null,"reasoning_effort":null},
        {"name":"Future","mode":"future","model":"unmanaged"}
    ]}}).to_string()
}

#[test]
fn patches_only_matched_responses_preserving_unknown_fields_and_id_types() {
    let mut relay = Relay::default();
    let settings = Settings::default();
    relay
        .from_client(
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
    let mut relay = Relay::default();
    for raw in [
        r#"{"id":2,"method":"thread/settings/update","params":{"model":"manual","approvalPolicy":"on-request"}}"#,
        r#"{"id":3,"method":"turn/start","params":{"sandboxPolicy":{"type":"readOnly"}}}"#,
        r#"{"id":4,"result":{"unknown":true}}"#,
        r#"{"method":"unknown","params":{"model":"manual"}}"#,
    ] {
        relay.from_client(raw, &Settings::default()).unwrap();
        assert!(relay
            .from_server(raw, &Settings::default())
            .unwrap()
            .is_none());
    }
    relay
        .from_client(
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
    let mut relay = Relay::default();
    for bad in ["{", "[]", r#"{"id":1.5}"#, r#"{"id":null}"#] {
        assert!(relay.from_client(bad, &Settings::default()).is_err());
    }
    relay
        .from_client(
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
    assert!(Relay::default()
        .from_server(&raw, &Settings::default())
        .unwrap()
        .is_none());
}

fn resumed_relay() -> Relay {
    let mut relay = Relay::default();
    let settings = Settings::default();
    relay
        .from_client(
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
    assert!(relay.from_client(&saved_turn, &settings).unwrap().is_none());
    for mode in ["default", "plan"] {
        let raw = update("thread/settings/update", mode, "manual", "high");
        let patched = relay.from_client(&raw, &settings).unwrap().unwrap();
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
            .from_client(&stale, &settings)
            .unwrap()
            .unwrap()
            .contains("medium"));
    }
    let manual = update("thread/settings/update", "plan", "manual", "high");
    assert!(relay.from_client(&manual, &settings).unwrap().is_none());
    assert!(relay.from_client(&saved_turn, &settings).unwrap().is_none());
}

#[test]
fn failed_start_does_not_invent_thread_state_or_patch_errors() {
    let mut relay = Relay::default();
    let settings = Settings::default();
    relay
        .from_client(r#"{"id":1,"method":"thread/start"}"#, &settings)
        .unwrap();
    assert!(relay
        .from_server(
            r#"{"id":1,"error":{"code":-1,"message":"denied"}}"#,
            &settings
        )
        .unwrap()
        .is_none());
    assert!(relay
        .from_client(
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
        .from_client(
            &update("thread/settings/update", "default", "manual", "high"),
            &settings,
        )
        .unwrap();
    let error = r#"{"id":99,"error":{"code":-1,"message":"unavailable"}}"#;
    assert!(relay.from_server(error, &settings).unwrap().is_none());
    assert!(relay
        .from_client(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_some());
    assert!(relay
        .from_client(
            &update("thread/settings/update", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_none());
    assert!(relay
        .from_client(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_none());
}

#[test]
fn standard_fork_starts_in_default_mode() {
    let mut relay = Relay::default();
    let settings = Settings::default();
    relay
        .from_client(
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
        .from_client(
            &update("turn/start", "default", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_none());
    assert!(relay
        .from_client(
            &update("thread/settings/update", "plan", "manual", "high"),
            &settings
        )
        .unwrap()
        .is_some());
}
