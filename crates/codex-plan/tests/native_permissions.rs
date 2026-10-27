//! Opt-in check against the installed native binary; no account or paid inference.
//! Run outside another Codex sandbox (the native daemon socket must be visible):
//! cargo test -p codex-plan --test native_permissions -- --ignored --nocapture
use codex_plan::{selected_permissions, Relay, Settings};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

const TOKEN: &str = "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    relay: Option<Relay>,
    events: Vec<Value>,
}

impl Native {
    fn send(&mut self, message: Value) {
        let raw = message.to_string();
        let raw = self
            .relay
            .as_mut()
            .and_then(|r| r.from_client(&raw, &Settings::default()).unwrap())
            .unwrap_or(raw);
        writeln!(self.input.as_mut().unwrap(), "{raw}").unwrap();
    }

    fn next(&mut self) -> Value {
        let message = self
            .output
            .recv_timeout(Duration::from_secs(45))
            .expect("native event timeout");
        if let Some(relay) = &mut self.relay {
            relay
                .from_server(&message.to_string(), &Settings::default())
                .unwrap();
        }
        self.events.push(message.clone());
        message
    }

    fn rpc(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"id":id,"method":method,"params":params}));
        loop {
            let message = self.next();
            if message.get("method").is_none() && message["id"] == id {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        for _ in 0..50 {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

fn mock_responses(listener: TcpListener, command: String, turns: usize) -> Receiver<Value> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for (index, stream) in listener.incoming().take(turns * 2).enumerate() {
            let mut stream = stream.unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(45)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            sender.send(serde_json::from_slice(&body).unwrap()).unwrap();
            let id = format!("mock-{index}");
            let item = if index % 2 == 0 {
                json!({"type":"function_call","call_id":format!("synthetic-tool-{index}"),"name":"exec_command", "arguments":json!({"cmd":command,"yield_time_ms":1000}).to_string()})
            } else {
                json!({"type":"message","id":"done","role":"assistant","content":[{"type":"output_text","text":"Synthetic check complete."}]})
            };
            let events = [
                json!({"type":"response.created","response":{"id":id}}),
                json!({"type":"response.output_item.done","item":item}),
                json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
            ];
            let data: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}", data.len()).unwrap();
        }
    });
    receiver
}

fn scenario(
    native: &Path,
    label: &str,
    through_relay: bool,
    network: bool,
    suppress: bool,
    launcher: bool,
) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let work = root.path().join("work");
    let config = home.join(".codex");
    let secret = home.join("private");
    for path in [&home, &work, &config, &secret] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(secret.join("secret.txt"), "synthetic-protected-file").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = mock_responses(
        listener,
        format!(
            "printf 'credential=%s\\n' \"{}\"; cat {}/secret.txt",
            if launcher {
                "$GH_TOKEN"
            } else {
                "$GITHUB_TOKEN"
            },
            secret.display()
        ),
        if launcher { 3 } else { 1 },
    );
    std::fs::write(
        config.join("config.toml"),
        format!(
            r#"
model = "gpt-6.1-sol"
model_provider = "synthetic"
model_reasoning_effort = "low"
default_permissions = "synthetic-check"
approval_policy = "{approval}"
approvals_reviewer = "auto_review"
suppress_unstable_features_warning = {suppress}
[model_providers.synthetic]
name = "Local synthetic Responses"
base_url = "http://{address}/v1"
wire_api = "responses"
supports_websockets = false
[shell_environment_policy]
ignore_default_excludes = true
include_only = ["HOME", "PATH", "GH_TOKEN", "GITHUB_TOKEN"]
[permissions.synthetic-check]
extends = ":workspace"
[permissions.synthetic-check.filesystem]
":root" = "read"
"{}" = "deny"
[permissions.synthetic-check.network]
enabled = {network}
mode = "full"
allow_local_binding = true
[features]
network_proxy = true
credential_masking = true
"#,
            secret.display(),
            approval = if launcher { "on-request" } else { "never" },
        ),
    )
    .unwrap();
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    if launcher {
        let gh = bin.join("gh");
        std::fs::write(&gh, format!("#!/bin/sh\nprintf '%s\\n' '{TOKEN}'\n")).unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let stderr = root.path().join("stderr");
    let mut command = if launcher {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "launcher_native_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("SYNTHETIC_LAUNCHER_NATIVE", native);
        command
    } else {
        let mut command = Command::new(native);
        command.args(["app-server", "--listen", "stdio://"]);
        command
    };
    // No inherited credential enters the helper; gh in the isolated PATH is
    // the only credential source for the protected entrypoint.
    let original_path = std::env::var_os("PATH").unwrap();
    command.env_clear();
    if launcher {
        command.env("SYNTHETIC_LAUNCHER_NATIVE", native);
    } else {
        command
            .env("CODEX_HOME", &config)
            .env("GITHUB_TOKEN", TOKEN);
    }
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&original_path));
    let mut child = command
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("SHELL", "/usr/bin/zsh")
        .current_dir(&work)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let input = child.stdin.take();
    let output = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let line = line.unwrap();
            // The helper execs before libtest can emit its completion output.
            if launcher && (line.is_empty() || line == "running 1 test") {
                continue;
            }
            if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                break;
            }
        }
    });
    let mut client = Native {
        child,
        input,
        output: receiver,
        relay: None,
        events: Vec::new(),
    };
    client.rpc(0,"initialize",json!({"clientInfo":{"name":"codex-plan-synthetic-check","version":"1"},"capabilities":{"experimentalApi":true}}));
    client.send(json!({"method":"initialized"}));
    let discovered = client.rpc(1, "config/read", json!({"cwd":work}));
    assert_eq!(
        discovered["config"]["default_permissions"],
        "synthetic-check"
    );
    if through_relay {
        let permissions = selected_permissions(&json!({"result":discovered})).unwrap();
        client.relay = Some(if launcher {
            Relay::protected(permissions)
        } else {
            Relay::new(permissions)
        });
    }
    let start = client.rpc(
        2,
        "thread/start",
        json!({"cwd":work,"sandbox":"workspace-write"}),
    );
    assert_eq!(start["sandbox"]["networkAccess"], through_relay && network);
    if through_relay {
        assert_eq!(start["activePermissionProfile"]["id"], "synthetic-check");
    } else {
        assert!(start["activePermissionProfile"].is_null());
    }
    if through_relay && network {
        let thread_id = start["thread"]["id"].as_str().unwrap();
        verify_tool_turn(&mut client, &requests, thread_id, &work, 30);
        let read = client.rpc(
            4,
            "thread/read",
            json!({"threadId":thread_id,"includeTurns":true}),
        );
        assert!(!read["thread"]["turns"].as_array().unwrap().is_empty());
        let resume = client.rpc(
            5,
            "thread/resume",
            json!({"threadId":thread_id,"cwd":work,"sandbox":"workspace-write"}),
        );
        assert_eq!(resume["activePermissionProfile"]["id"], "synthetic-check");
        assert_eq!(resume["sandbox"]["networkAccess"], true);
        assert_eq!(resume["model"], start["model"]);
        assert_eq!(resume["reasoningEffort"], start["reasoningEffort"]);
        assert!(!resume["thread"]["turns"].as_array().unwrap().is_empty());
        if launcher {
            verify_tool_turn(&mut client, &requests, thread_id, &work, 31);
        }
        let fork = client.rpc(
            6,
            "thread/fork",
            json!({"threadId":thread_id,"cwd":work,"sandbox":"workspace-write"}),
        );
        assert_eq!(fork["activePermissionProfile"]["id"], "synthetic-check");
        assert_eq!(fork["sandbox"]["networkAccess"], true);
        assert_eq!(fork["model"], start["model"]);
        assert_eq!(fork["reasoningEffort"], start["reasoningEffort"]);
        let fork_id = fork["thread"]["id"].as_str().unwrap();
        let fork_read = client.rpc(
            7,
            "thread/read",
            json!({"threadId":fork_id,"includeTurns":true}),
        );
        assert!(!fork_read["thread"]["turns"].as_array().unwrap().is_empty());
        if launcher {
            verify_tool_turn(&mut client, &requests, fork_id, &work, 32);
        }
    }
    // An RPC after thread/start drains the ordered startup notifications too.
    client.rpc(20, "config/read", json!({"cwd":work}));
    let warnings: Vec<_> = client
        .events
        .iter()
        .filter(|e| matches!(e["method"].as_str(), Some("warning" | "configWarning")))
        .collect();
    if through_relay && network {
        assert!(warnings.is_empty(), "{warnings:?}");
    } else {
        assert!(warnings
            .iter()
            .any(|w| w.to_string().contains("Credential masking is inactive")));
    }
    if !suppress {
        assert!(warnings
            .iter()
            .any(|w| w.to_string().contains("Under-development features enabled")));
    }
    drop(client);
    let stderr = std::fs::read_to_string(stderr).unwrap();
    // Codex intentionally refuses to install PATH helper aliases in a temporary HOME.
    for line in stderr.lines().filter(|line| !line.is_empty()) {
        let helper_warning =
            line.contains("Refusing to create helper binaries under temporary dir");
        let negative_control_warning = (!through_relay || !network)
            && line
                .contains("Credential masking is inactive because the network proxy is disabled");
        assert!(
            helper_warning || negative_control_warning,
            "unexplained native diagnostic: {line}"
        );
    }
    eprintln!(
        "{label}: native profile/network, startup warnings, and applicable tool/history checks passed (temporary-HOME PATH helper refusal expected)"
    );
}

/// Runs in a separate process so the shared launcher reads only fixture state.
#[test]
#[ignore = "internal subprocess entrypoint for native acceptance"]
fn launcher_native_helper() {
    let Some(native) = std::env::var_os("SYNTHETIC_LAUNCHER_NATIVE") else {
        return;
    };
    let mut command =
        codex_launcher::command(Path::new(&native), codex_launcher::Launch::SessionServer)
            .expect("prepare protected native server");
    panic!("execute protected native server: {}", command.exec());
}

fn verify_tool_turn(
    client: &mut Native,
    requests: &Receiver<Value>,
    thread_id: &str,
    cwd: &Path,
    id: u64,
) {
    client.rpc(id, "turn/start", json!({"threadId":thread_id,"cwd":cwd,"input":[{"type":"text","text":"Run synthetic verification.","text_elements":[]}]}));
    loop {
        if client.next()["method"] == "turn/completed" {
            break;
        }
    }
    let raw: Vec<_> = (0..2)
        .map(|_| requests.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    for request in &raw {
        assert!(
            !request.to_string().contains(TOKEN),
            "synthetic credential reached model request"
        );
    }
    let outputs: Vec<_> = raw[1]["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["type"] == "function_call_output")
        .collect();
    let output = outputs.last().expect("tool result in model request")["output"]
        .as_str()
        .unwrap();
    assert!(output.contains("Permission denied"), "{output}");
    assert!(!output.contains("synthetic-protected-file"));
    let placeholder = output
        .split("credential=")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    assert!(placeholder.starts_with("ghp_"));
    assert_eq!(placeholder.len(), TOKEN.len());
    assert_ne!(placeholder, TOKEN);
}

fn native_binary() -> std::path::PathBuf {
    std::env::var_os("CODEX_NATIVE_TEST_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/bin/codex")
        })
}

#[test]
#[ignore = "requires installed Codex 0.164.0, zsh, loopback sockets, and native sandbox access"]
fn native_permissions_and_credential_masking() {
    let native = native_binary();
    scenario(
        &native,
        "legacy negative control",
        false,
        true,
        false,
        false,
    );
    scenario(&native, "relay named permissions", true, true, true, false);
    scenario(
        &native,
        "network disabled negative control",
        true,
        false,
        true,
        false,
    );
}

#[test]
#[ignore = "requires installed Codex 0.164.0, zsh, loopback sockets, and native sandbox access"]
fn shared_launcher_credentials_in_new_resumed_and_forked_tools() {
    scenario(
        &native_binary(),
        "shared launcher new/resume/fork credentials",
        true,
        true,
        true,
        true,
    );
}
