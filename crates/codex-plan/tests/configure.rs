//! Real command fixtures: paginated discovery, failure isolation, persisted pair.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};

const NATIVE: &str = r#"#!/bin/sh
if [ "$FAIL_NATIVE" = yes ]; then exit 1; fi
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*) echo '{"id":0,"result":{}}' ;;
    *'"method":"model/list"'*)
      case "$line" in
        *'"cursor":"page2"'*) echo '{"id":1,"result":{"nextCursor":null,"data":[{"model":"future-model","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]}}' ;;
        *) echo '{"id":1,"result":{"nextCursor":"page2","data":[{"model":"gpt-6.1-sol","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}]}}' ;;
      esac ;;
    *'"method":"collaborationMode/list"'*) echo '{"id":2,"result":{"data":[{"mode":"plan"},{"mode":"default"}]}}' ;;
    *'"method":"account/rateLimits/read"'*) echo '{"id":3,"result":{"accountId":"synthetic-private-account","ordinaryUsageAllowed":true,"rateLimits":{},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":11,"resetsAt":1794000000,"windowDurationMins":300}},"other":{"secondary":{"usedPercent":20,"resetsAt":1795000000,"windowDurationMins":10080}}}}}' ;;
  esac
done
"#;

struct Fixture {
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join(".local/bin/codex");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, NATIVE).unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        // PATH represents the credential launcher: it refuses app-server.
        fs::write(dir.path().join("codex"), "#!/bin/sh\nexit 91\n").unwrap();
        fs::set_permissions(dir.path().join("codex"), fs::Permissions::from_mode(0o700)).unwrap();
        Self { dir }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_codex-plan"));
        c.env("HOME", self.dir.path())
            .env("PATH", self.dir.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .env("CODEX_HOME", self.dir.path().join("native"));
        c
    }
    fn configure(&self, model: &str) -> Output {
        self.command()
            .args([
                "configure",
                "--plan-model",
                model,
                "--plan-effort",
                "high",
                "--execute-model",
                "gpt-6.1-sol",
                "--execute-effort",
                "medium",
            ])
            .output()
            .unwrap()
    }
}

#[test]
fn explicit_pair_validates_later_pages_and_failure_keeps_saved_bytes() {
    let fixture = Fixture::new();
    assert!(fixture.configure("future-model").status.success());
    let path = fixture.dir.path().join("config/codex-plan/settings.toml");
    let before = fs::read(&path).unwrap();
    let settings = codex_plan::Settings::load(&path).unwrap();
    assert_eq!(settings.plan.model, "future-model");
    assert!(!fixture.configure("unknown-model").status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
    let out = fixture
        .command()
        .env("FAIL_NATIVE", "yes")
        .args([
            "configure",
            "--plan-model",
            "future-model",
            "--plan-effort",
            "high",
            "--execute-model",
            "gpt-6.1-sol",
            "--execute-effort",
            "medium",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn non_tty_requires_explicit_options_without_writing() {
    let fixture = Fixture::new();
    let out = fixture.command().arg("configure").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("four explicit options"));
    assert!(!fixture.dir.path().join("config").exists());
}

#[test]
fn signal_during_interactive_input_exits_without_saving() {
    use std::{
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::process::CommandExt,
        },
        time::{Duration, Instant},
    };
    let fixture = Fixture::new();
    let (mut master_fd, mut slave_fd) = (-1, -1);
    // SAFETY: openpty initializes two fresh descriptors, transferred to File.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    let mut master = unsafe { fs::File::from_raw_fd(master_fd) };
    let slave = unsafe { fs::File::from_raw_fd(slave_fd) };
    let mut command = fixture.command();
    command
        .arg("configure")
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave);
    // SAFETY: only async-signal-safe libc calls run between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    unsafe {
        libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    let mut signalled = false;
    let mut status = None;
    while Instant::now() < deadline {
        let mut buffer = [0; 8192];
        if let Ok(n) = master.read(&mut buffer) {
            output.extend_from_slice(&buffer[..n]);
        }
        if !signalled && String::from_utf8_lossy(&output).contains("モデル [") {
            // SAFETY: signal only our own live child, after discovery completed.
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            signalled = true;
        }
        status = child.try_wait().unwrap();
        if status.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    assert!(signalled, "interactive prompt was not reached");
    assert!(status.is_some(), "SIGTERM did not interrupt terminal input");
    assert!(!status.unwrap().success());
    assert!(!fixture
        .dir
        .path()
        .join("config/codex-plan/settings.toml")
        .exists());
}

#[test]
fn comparison_is_available_without_history_and_keeps_unknowns_and_all_buckets() {
    let fixture = Fixture::new();
    let out = fixture
        .command()
        .args(["compare", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["native_models"].as_array().unwrap().len(), 2);
    assert_eq!(report["current_rate_windows"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["fixed_example"]["costs"].as_array().unwrap().len(),
        7
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-private-account"));
    assert!(report["remaining_days"][0]["remaining_workdays_range"].is_null());
    assert!(!fixture.dir.path().join("config").exists());
    let out = fixture
        .command()
        .env("FAIL_NATIVE", "yes")
        .args(["compare", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["native_error"].is_string());
    assert_eq!(report["reference_catalog"].as_array().unwrap().len(), 7);
}
