use std::{fs, os::unix::fs::PermissionsExt, path::Path};

pub fn credentials(home: &Path) {
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        r#"default_permissions = "fixture"
approval_policy = "on-request"
approvals_reviewer = "auto_review"
[features]
network_proxy = true
credential_masking = true
[permissions.fixture]
extends = ":workspace"
[permissions.fixture.network]
enabled = true
"#,
    )
    .unwrap();
    let gh = home.join("gh");
    fs::write(
        &gh,
        "#!/bin/sh\nprintf 'ghp_0000000000000000000000000000000000000000\\n'\n",
    )
    .unwrap();
    fs::set_permissions(gh, fs::Permissions::from_mode(0o700)).unwrap();
}
