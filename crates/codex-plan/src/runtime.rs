use crate::{parse_message, Relay, Settings, MAX_MESSAGE};
use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{os::unix::fs::PermissionsExt, path::Path, process::Stdio, time::Duration};
use tokio::{
    net::UnixListener,
    process::{Child, ChildStdin, ChildStdout, Command},
    signal::unix::{signal, SignalKind},
    time::timeout,
};
use tokio_tungstenite::{
    accept_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

type Reader = FramedRead<ChildStdout, LinesCodec>;
type Writer = FramedWrite<ChildStdin, LinesCodec>;

fn private_directory() -> Result<tempfile::TempDir> {
    Ok(tempfile::Builder::new()
        .prefix("codex-plan-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")?)
}

/// Each server owns a separate process group, so its tools are also stopped.
/// The TUI stays in the foreground terminal group for raw input and resizing.
struct Server {
    child: Child,
    group: i32,
}

impl Server {
    fn spawn(native: &Path) -> Result<(Self, Reader, Writer)> {
        let mut command = Command::new(native);
        command
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        command.process_group(0);
        let mut child = command.spawn().context("start native codex app-server")?;
        let group = child.id().context("app-server has no PID")? as i32;
        let reader = FramedRead::new(
            child.stdout.take().unwrap(),
            LinesCodec::new_with_max_length(MAX_MESSAGE),
        );
        let writer = FramedWrite::new(
            child.stdin.take().unwrap(),
            LinesCodec::new_with_max_length(MAX_MESSAGE),
        );
        Ok((Self { child, group }, reader, writer))
    }

    async fn stop(&mut self) {
        // Closing stdin lets native Codex flush history and shut down threads.
        if timeout(Duration::from_secs(5), self.child.wait())
            .await
            .is_err()
        {
            self.kill_group();
        }
        let _ = self.child.wait().await;
        self.kill_group();
        self.group = 0;
    }

    fn kill_group(&self) {
        // SAFETY: group is the positive PID returned for our own child started
        // with process_group(0). Never use a caller-supplied PID or group zero.
        if self.group > 0 {
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill_group();
    }
}

async fn rpc(
    reader: &mut Reader,
    writer: &mut Writer,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    writer
        .send(json!({"id":id,"method":method,"params":params}).to_string())
        .await?;
    timeout(Duration::from_secs(60), async {
        while let Some(line) = reader.next().await {
            let message =
                parse_message(&line.context("read app-server JSON line (maximum 256 MiB)")?)?;
            if message.get("method").is_none() && message["id"] == id {
                if let Some(error) = message.get("error") {
                    bail!("native {method} failed: {error}");
                }
                return Ok(message);
            }
            // Preflight has no threads or tools. A server request here cannot be
            // silently acknowledged (especially if it ever becomes an approval).
            if message.get("method").is_some() && message.get("id").is_some() {
                bail!("unexpected server request during preflight");
            }
        }
        bail!("app-server disconnected during {method}")
    })
    .await
    .context("native protocol preflight timed out")?
}

#[derive(Debug, serde::Serialize)]
pub struct Discovery {
    pub models: Vec<Value>,
    pub modes: Value,
    pub rate_limits: Option<Value>,
    pub rate_limits_error: Option<String>,
}

impl Discovery {
    pub fn validate(&self, settings: &Settings) -> Result<()> {
        settings.validate_models(&self.models)?;
        settings.validate_experiment(&self.models)?;
        settings.patch_presets(&mut self.modes.clone())
    }
}

pub async fn preflight(native: &Path, settings: &Settings) -> Result<()> {
    discover(native, false).await?.validate(settings)
}

/// Discovery never starts a thread, sends prompts or changes account selection.
pub async fn discover(native: &Path, with_rates: bool) -> Result<Discovery> {
    let mut term = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let (mut server, mut reader, mut writer) = Server::spawn(native)?;
    let check = async {
        rpc(
            &mut reader,
            &mut writer,
            0,
            "initialize",
            json!({
                "clientInfo":{"name":"codex-plan","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"experimentalApi":true}
            }),
        )
        .await?;
        writer
            .send(json!({"method":"initialized"}).to_string())
            .await?;
        let mut models = Vec::new();
        let mut cursor = Value::Null;
        let mut seen = std::collections::HashSet::new();
        loop {
            let response = rpc(
                &mut reader,
                &mut writer,
                1,
                "model/list",
                json!({"cursor":cursor,"includeHidden":true}),
            )
            .await?;
            models.extend(
                response
                    .pointer("/result/data")
                    .and_then(Value::as_array)
                    .context("unsupported model/list protocol: missing result.data")?
                    .iter()
                    .cloned(),
            );
            cursor = response
                .pointer("/result/nextCursor")
                .context("unsupported model/list protocol: missing nextCursor")?
                .clone();
            if cursor.is_null() {
                break;
            }
            let token = cursor.as_str().context("invalid model/list cursor")?;
            if !seen.insert(token.to_owned()) {
                bail!("model/list repeated a pagination cursor");
            }
        }
        let modes = rpc(
            &mut reader,
            &mut writer,
            2,
            "collaborationMode/list",
            json!({}),
        )
        .await?;
        let (rate_limits, rate_limits_error) = if with_rates {
            match rpc(
                &mut reader,
                &mut writer,
                3,
                "account/rateLimits/read",
                json!({
                    "supportsLunaReserve": false,
                    "excludeResetCreditDetails": false
                }),
            )
            .await
            {
                Ok(value) => (value.get("result").cloned(), None),
                Err(error) => (None, Some(format!("{error:#}"))),
            }
        } else {
            (None, None)
        };
        Ok(Discovery {
            models,
            modes,
            rate_limits,
            rate_limits_error,
        })
    };
    let result = tokio::select! {
        result = check => result,
        _ = term.recv() => Err(anyhow::anyhow!("preflight interrupted by SIGTERM")),
        _ = hangup.recv() => Err(anyhow::anyhow!("preflight interrupted by SIGHUP")),
        _ = interrupt.recv() => Err(anyhow::anyhow!("preflight interrupted by SIGINT")),
    };
    drop(writer);
    server.stop().await;
    result
}

/// One connection per launch. Reconnecting/replaying a turn could execute tools
/// twice; a broken connection ends this launcher instead.
async fn relay(
    listener: UnixListener,
    mut reader: Reader,
    mut writer: Writer,
    settings: &Settings,
    advice: &std::sync::Mutex<crate::advice::AdviceObserver>,
) -> Result<()> {
    let (socket, _) = timeout(Duration::from_secs(60), listener.accept())
        .await
        .context("TUI did not connect within 60 seconds")??;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let websocket = timeout(
        Duration::from_secs(15),
        accept_async_with_config(socket, Some(config)),
    )
    .await
    .context("TUI WebSocket handshake timed out")??;
    let (mut outgoing, mut incoming) = websocket.split();
    let relay = std::sync::Mutex::new(Relay::default());
    // Poll both directions independently even while a pipe/socket write is
    // backpressured. Serial writes can deadlock against a busy native server.
    let to_server = async {
        loop {
            let message = incoming.next().await;
            {
                match message.context("TUI disconnected")?? {
                    Message::Text(raw) => {
                        let patched = relay.lock().unwrap().from_client(&raw, settings)?;
                        writer
                            .send(patched.unwrap_or_else(|| raw.to_string()))
                            .await
                            .context("write to app-server")?;
                    }
                    // tungstenite queues and flushes automatic pong/close frames
                    // when its stream is polled again.
                    Message::Ping(_) | Message::Pong(_) => {}
                    Message::Close(_) => return Ok(()),
                    _ => bail!("unsupported binary WebSocket message"),
                }
            }
        }
    };
    let to_client = async {
        loop {
            let raw = reader
                .next()
                .await
                .context("app-server disconnected")?
                .context("read app-server JSON line (maximum 256 MiB)")?;
            advice.lock().unwrap().observe(&parse_message(&raw)?);
            let patched = relay.lock().unwrap().from_server(&raw, settings)?;
            outgoing
                .send(Message::Text(patched.unwrap_or(raw).into()))
                .await
                .context("forward app-server message to TUI")?;
        }
    };
    tokio::select! { result = to_server => result, result = to_client => result }
}

pub fn tui_args(
    settings: &Settings,
    endpoint: &str,
    resume: Option<&str>,
    prompt: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "--remote".into(),
        endpoint.into(),
        "-c".into(),
        format!(
            "plan_mode_reasoning_effort={:?}",
            settings.plan.reasoning_effort
        ),
    ];
    if let Some(session) = resume {
        // Any model/effort override here would prevent native RestoreFromThread.
        args.extend(["resume".into(), session.into()]);
    } else {
        args.extend([
            "--model".into(),
            settings.execute.model.clone(),
            "-c".into(),
            format!(
                "model_reasoning_effort={:?}",
                settings.execute.reasoning_effort
            ),
        ]);
        if let Some(prompt) = prompt {
            args.extend(["--".into(), prompt.into()]);
        }
    }
    args
}

pub async fn launch(
    native: &Path,
    settings: &Settings,
    resume: Option<&str>,
    prompt: Option<&str>,
) -> Result<i32> {
    // Preflight owns its own handlers and always reaps its child before return.
    let discovery = discover(native, false).await?;
    discovery.validate(settings)?;
    let advice = std::sync::Mutex::new(crate::advice::AdviceObserver::default());
    let mut term = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    // Explicit permissions create the directory atomically with mode 0700. A short /tmp
    // path also fits Darwin's smaller sockaddr_un regardless of TMPDIR.
    let directory = private_directory()?;
    let socket_path = directory.path().join("rpc.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let endpoint = format!("unix://{}", socket_path.display());
    let (mut server, reader, writer) = Server::spawn(native)?;
    let mut terminal = Terminal::capture();
    let mut tui = Command::new(native)
        .arg("--cd")
        .arg(std::env::current_dir()?)
        .args(tui_args(settings, &endpoint, resume, prompt))
        .kill_on_drop(true)
        .spawn()
        .context("start native Codex TUI")?;
    let result = async {
        tokio::select! {
            status = tui.wait() => Ok(status?.code().unwrap_or(1)),
            result = relay(listener, reader, writer, settings, &advice) => {
                match result {
                    Ok(()) => timeout(Duration::from_secs(2), tui.wait()).await
                        .context("TUI closed connection but did not exit")?
                        .map(|s| s.code().unwrap_or(1)).context("wait for TUI"),
                    Err(error) => Err(error),
                }
            },
            _ = term.recv() => Ok(143),
            _ = hangup.recv() => Ok(129),
            _ = interrupt.recv() => Ok(130),
        }
    }
    .await;
    terminal.forced = tui.try_wait()?.is_none();
    let _ = tui.kill().await;
    let _ = tui.wait().await;
    server.stop().await;
    drop(directory);
    drop(terminal);
    if let Some(message) = advice.lock().unwrap().render(&discovery.models) {
        eprint!("{message}");
    }
    result
}

/// A killed TUI cannot restore raw mode. Preserve the terminal in the parent
/// so signals and transport errors still leave an usable shell.
struct Terminal {
    original: Option<libc::termios>,
    forced: bool,
}

impl Terminal {
    fn capture() -> Self {
        let mut value = std::mem::MaybeUninit::uninit();
        // SAFETY: tcgetattr initializes value on success; stdin is only queried.
        let original = unsafe {
            (libc::tcgetattr(libc::STDIN_FILENO, value.as_mut_ptr()) == 0)
                .then(|| value.assume_init())
        };
        Self {
            original,
            forced: true,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            // SAFETY: original came from tcgetattr on this terminal.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, original);
            }
            if self.forced {
                use std::io::Write;
                let _ = std::io::stderr().write_all(b"\x1b[?2026l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[<u\x1b[?1049l\x1b[?25h\x1b[0m");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixStream;
    use tokio_tungstenite::client_async_with_config;

    async fn echo_server() -> (Server, Reader, Writer) {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let reader = FramedRead::new(
            child.stdout.take().unwrap(),
            LinesCodec::new_with_max_length(MAX_MESSAGE),
        );
        let writer = FramedWrite::new(
            child.stdin.take().unwrap(),
            LinesCodec::new_with_max_length(MAX_MESSAGE),
        );
        let group = child.id().unwrap() as i32;
        (Server { child, group }, reader, writer)
    }

    #[tokio::test]
    async fn websocket_stdio_large_message_and_private_parallel_sockets() {
        let a = private_directory().unwrap();
        let b = private_directory().unwrap();
        assert_ne!(a.path(), b.path());
        assert_eq!(
            std::fs::metadata(a.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let socket = a.path().join("rpc.sock");
        let other = b.path().join("rpc.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let _other = UnixListener::bind(&other).unwrap();
        let (mut server, reader, writer) = echo_server().await;
        let task = tokio::spawn(async move {
            relay(
                listener,
                reader,
                writer,
                &Settings::default(),
                &std::sync::Mutex::new(crate::advice::AdviceObserver::default()),
            )
            .await
        });
        let stream = UnixStream::connect(&socket).await.unwrap();
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (mut client, _) = client_async_with_config("ws://localhost/", stream, Some(config))
            .await
            .unwrap();
        let raw = json!({"id":"large","result":{"data":"x".repeat(20 * 1024 * 1024)}}).to_string();
        client
            .send(Message::Text(raw.clone().into()))
            .await
            .unwrap();
        assert_eq!(
            client.next().await.unwrap().unwrap().into_text().unwrap(),
            raw
        );
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        server.stop().await;
        assert!(server.child.try_wait().unwrap().is_some());
        drop(a);
        assert!(!socket.exists());
        assert!(other.exists());
    }

    #[tokio::test]
    async fn malformed_websocket_message_ends_relay_without_forwarding() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let socket = dir.path().join("rpc.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (mut server, reader, writer) = echo_server().await;
        let task = tokio::spawn(async move {
            relay(
                listener,
                reader,
                writer,
                &Settings::default(),
                &std::sync::Mutex::new(crate::advice::AdviceObserver::default()),
            )
            .await
        });
        let (mut client, _) = client_async_with_config(
            "ws://localhost/",
            UnixStream::connect(socket).await.unwrap(),
            None,
        )
        .await
        .unwrap();
        client.send(Message::Text("{broken".into())).await.unwrap();
        assert!(task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("invalid JSON"));
        assert!(!matches!(client.next().await, Some(Ok(Message::Text(_)))));
        server.stop().await;
    }

    #[tokio::test]
    async fn native_disconnect_is_reported_and_child_is_reaped() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let socket = dir.path().join("rpc.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (mut server, reader, writer) = echo_server().await;
        let task = tokio::spawn(async move {
            relay(
                listener,
                reader,
                writer,
                &Settings::default(),
                &std::sync::Mutex::new(crate::advice::AdviceObserver::default()),
            )
            .await
        });
        let (_client, _) = client_async_with_config(
            "ws://localhost/",
            UnixStream::connect(socket).await.unwrap(),
            None,
        )
        .await
        .unwrap();
        server.kill_group();
        assert!(task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("app-server disconnected"));
        server.stop().await;
        assert!(server.child.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn bounded_line_decoder_rejects_oversize_without_partial_message() {
        use tokio::io::AsyncWriteExt;
        let (mut tx, rx) = tokio::io::duplex(128);
        let mut lines = FramedRead::new(rx, LinesCodec::new_with_max_length(16));
        tx.write_all(b"{\"data\":\"01234567890123456789\"}\n")
            .await
            .unwrap();
        assert!(lines.next().await.unwrap().is_err());
    }
}
