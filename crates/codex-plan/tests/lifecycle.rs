//! Launch the real adapter against a tiny native-process fixture. No credentials
//! or model requests; this exercises process ownership, not Codex's TUI logic.
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

const NATIVE: &str = r#"#!/bin/sh
if [ "$1" = app-server ]; then
  echo $$ >> "$FIXTURE_DIR/servers"
  while IFS= read -r line; do
    case "$line" in
      *'"method":"initialize"'*) echo '{"id":0,"result":{"userAgent":"fixture"}}' ;;
      *'"method":"model/list"'*) echo '{"id":1,"result":{"nextCursor":null,"data":[{"model":"gpt-6-astra","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]},{"model":"gpt-6.1-sol","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}]}}' ;;
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
        let native = dir.path().join(".local/bin/codex");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, NATIVE).unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.path().join("codex"), "#!/bin/sh\nexit 91\n").unwrap();
        fs::set_permissions(dir.path().join("codex"), fs::Permissions::from_mode(0o700)).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_codex-plan"))
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
