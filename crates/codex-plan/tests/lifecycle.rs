//! Launch the real adapter against a tiny native-process fixture. No credentials
//! or model requests; this exercises process ownership, not Codex's TUI logic.
mod common;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

const NATIVE: &str = r#"#!/bin/sh
if [ "$1" = --version ]; then echo "codex-cli 0.164.0"; exit; fi
if [ "$1" = --config ]; then shift; shift; fi
if [ "$1" = app-server ]; then
  echo $$ >> "$FIXTURE_DIR/servers"
  while IFS= read -r line; do
    printf '%s\n' "$line" >> "$FIXTURE_DIR/requests"
    case "$line" in
      *'"method":"fixture/ping"'*) echo '{"id":99,"result":{"alive":true}}'; echo '{"method":"fixture/event","params":{}}' ;;
      *'"method":"initialize"'*) echo '{"id":0,"result":{"userAgent":"fixture"}}' ;;
      *'"method":"model/list"'*) echo '{"id":1,"result":{"nextCursor":null,"data":[{"model":"gpt-6-astra","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]},{"model":"gpt-6.1-sol","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}]}}' ;;
      *'"method":"config/read"'*) echo '{"id":4,"result":{"config":{"default_permissions":"fixture"}}}' ;;
      *'"method":"collaborationMode/list"'*) echo '{"id":2,"result":{"data":[{"mode":"plan"},{"mode":"default"}]}}' ;;
    esac
  done
else
  printf '%s\n' "$@" > "$FIXTURE_DIR/args"
  echo $$ > "$FIXTURE_DIR/tui"
  if [ "$FIXTURE_EXIT" = yes ]; then exit 42; fi
  exec /bin/sleep 60
fi
"#;

struct Launch {
    dir: tempfile::TempDir,
    child: Child,
}
impl Launch {
    fn start(exit: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        common::credentials(dir.path());
        let native = dir.path().join(".local/bin/codex");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, NATIVE).unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.path().join("codex"), "#!/bin/sh\nexit 91\n").unwrap();
        fs::set_permissions(dir.path().join("codex"), fs::Permissions::from_mode(0o700)).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_codex-plan"))
            .env_remove("CODEX_HOME")
            .env_remove("CODEX_PROFILE")
            .env_remove("CLAUDE_CONFIG_DIR")
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .env("PATH", dir.path())
            .env("XDG_CONFIG_HOME", dir.path())
            .env("FIXTURE_DIR", dir.path())
            .env("FIXTURE_EXIT", if exit { "yes" } else { "no" })
            .process_group(0)
            .spawn()
            .unwrap();
        let launch = Self { dir, child };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !launch.dir.path().join("tui").exists() {
            assert!(
                Instant::now() < deadline,
                "fixture did not reach TUI startup"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        launch
    }

    fn socket(&self) -> String {
        fs::read_to_string(self.dir.path().join("args"))
            .unwrap()
            .lines()
            .find_map(|s| s.strip_prefix("unix://").map(str::to_owned))
            .unwrap()
    }

    fn finish(&mut self, expected: i32) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(expected));
                break;
            }
            assert!(Instant::now() < deadline, "launcher did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!Path::new(&self.socket()).exists());
        for file in ["servers", "tui"] {
            for pid in fs::read_to_string(self.dir.path().join(file))
                .unwrap()
                .lines()
            {
                let pid: i32 = pid.parse().unwrap();
                // SAFETY: signal 0 only probes fixture PIDs; it cannot send a signal.
                assert_eq!(
                    unsafe { libc::kill(pid, 0) },
                    -1,
                    "fixture process {pid} survived"
                );
            }
        }
    }
}
impl Drop for Launch {
    fn drop(&mut self) {
        // SAFETY: the fixture owns the child process group, including its fake TUI.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn signal_cleanup_does_not_touch_parallel_launch() {
    let mut a = Launch::start(false);
    let mut b = Launch::start(false);
    assert_ne!(a.socket(), b.socket());
    // SAFETY: these are our live launcher children, not caller supplied PIDs.
    unsafe {
        libc::kill(a.child.id() as i32, libc::SIGTERM);
    }
    a.finish(143);
    assert!(Path::new(&b.socket()).exists());
    assert!(b.child.try_wait().unwrap().is_none());
    unsafe {
        libc::kill(b.child.id() as i32, libc::SIGHUP);
    }
    b.finish(129);
}

#[test]
fn tui_startup_failure_reaps_server_and_socket() {
    let mut launch = Launch::start(true);
    launch.finish(42);
}

#[tokio::test]
async fn denied_background_request_keeps_connection_and_native_notifications_alive() {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use tokio::{net::UnixStream, time::timeout};
    use tokio_tungstenite::{client_async, tungstenite::Message};

    let mut launch = Launch::start(false);
    let stream = UnixStream::connect(launch.socket()).await.unwrap();
    let (mut socket, _) = client_async("ws://localhost/", stream).await.unwrap();
    let methods = ["command/exec", "process/exec", "thread/shellCommand"];
    let mut ids = Vec::new();
    // More refusals than the outbound queue capacity: neither reader may
    // monopolize the task while local replies are backpressured.
    for (index, method) in methods.into_iter().cycle().take(96).enumerate() {
        let id = if index % 2 == 0 {
            json!(index)
        } else {
            json!(index.to_string())
        };
        socket.send(Message::Text(json!({"id":id,"method":method,"params":{"command":["git","status"],"cwd":launch.dir.path()}}).to_string().into())).await.unwrap();
        ids.push(id);
    }
    for id in ids {
        let reply = timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .expect("relay closed after background request")
            .expect("read refusal");
        let reply: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["error"]["code"], -32600);
        assert!(reply.get("result").is_none());
        assert!(launch.child.try_wait().unwrap().is_none());
    }
    socket
        .send(Message::Text(
            json!({"id":99,"method":"fixture/ping","params":{}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    for expected in [
        json!({"id":99,"result":{"alive":true}}),
        json!({"method":"fixture/event","params":{}}),
    ] {
        let reply = timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(reply.to_text().unwrap()).unwrap(),
            expected
        );
    }
    let requests = fs::read_to_string(launch.dir.path().join("requests")).unwrap();
    for method in ["command/exec", "process/exec", "thread/shellCommand"] {
        assert!(
            !requests.contains(method),
            "denied request reached native: {requests}"
        );
    }
    // SAFETY: this is our live launcher child.
    unsafe {
        libc::kill(launch.child.id() as i32, libc::SIGTERM);
    }
    launch.finish(143);
}
