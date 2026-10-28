//! Backblaze, b2_list_file_versions, updated 2026-09-11, retrieved 2026-10-28:
//! <https://www.backblaze.com/apidocs/b2-list-file-versions>
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Cursor {
    pub file_name: String,
    pub file_id: Option<String>,
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn curl_json(curl: &Path, config: &str) -> Result<Value, &'static str> {
    let mut cmd = Command::new(curl);
    cmd.env_clear();
    for key in [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    let mut child = cmd
        .args([
            "-q",
            "--fail",
            "--silent",
            "--connect-timeout",
            "15",
            "--max-time",
            "60",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            #[cfg(test)]
            eprintln!("fixture process spawn: {e}");
            let _ = e;
            "cannot start B2 request"
        })?;
    let write = child
        .stdin
        .take()
        .ok_or("cannot send B2 request")?
        .write_all(config.as_bytes());
    let output = child.wait_with_output().map_err(|_| "B2 request failed")?;
    if write.is_err() {
        return Err("cannot send B2 request");
    }
    if !output.status.success() {
        return Err("B2 request failed");
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "invalid B2 response")
}

/// Authorize with the source key through stdin, keeping the key out of argv.
pub fn authorize(curl: &Path, key_id: &str, application_key: &str) -> Result<Value, &'static str> {
    if key_id.is_empty()
        || application_key.is_empty()
        || key_id.contains(':')
        || key_id
            .bytes()
            .chain(application_key.bytes())
            .any(|b| b.is_ascii_control())
    {
        return Err("invalid B2 key");
    }
    let config = format!(
        "url = \"https://api.backblazeb2.com/b2api/v4/b2_authorize_account\"\nuser = {}\n",
        quoted(&format!("{key_id}:{application_key}"))
    );
    curl_json(curl, &config)
}

/// Refuse to infer an unfiltered listing from a token whose allowed scope has
/// not been established. B2 authorize-account v4, retrieved 2026-10-28:
/// <https://www.backblaze.com/apidocs/b2-authorize-account>
pub fn authorize_scope(
    response: &Value,
    bucket_id: &str,
    prefix: &str,
) -> Result<(), &'static str> {
    let allowed = response
        .pointer("/apiInfo/storageApi/allowed")
        .ok_or("missing authorization scope")?;
    let capabilities = allowed
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or("missing capabilities")?;
    if !capabilities
        .iter()
        .any(|capability| capability.as_str() == Some("listFiles"))
    {
        return Err("listing not authorized");
    }
    let buckets = allowed.get("buckets").ok_or("missing bucket scope")?;
    if !buckets.is_null()
        && !buckets
            .as_array()
            .ok_or("invalid bucket scope")?
            .iter()
            .any(|bucket| bucket.get("id").and_then(Value::as_str) == Some(bucket_id))
    {
        return Err("bucket not authorized");
    }
    let allowed_prefix = allowed.get("namePrefix").ok_or("missing prefix scope")?;
    if !allowed_prefix.is_null()
        && !prefix.starts_with(allowed_prefix.as_str().ok_or("invalid prefix scope")?)
    {
        return Err("prefix not fully authorized");
    }
    Ok(())
}

/// Bind the listing to restic's S3 endpoint, bucket name and object prefix.
/// restic v0.18.1: internal/backend/s3/config.go:56-105 (retrieved 2026-10-28).
/// Bucket ID/name mapping: B2 b2_list_buckets v4 (retrieved 2026-10-28),
/// <https://www.backblaze.com/apidocs/b2-list-buckets>.
pub fn verify_repository(
    curl: &Path,
    authorization: &Value,
    repository: &str,
    bucket_id: &str,
    prefix: &str,
) -> Result<(), &'static str> {
    authorize_scope(authorization, bucket_id, prefix)?;
    let storage = authorization
        .pointer("/apiInfo/storageApi")
        .ok_or("missing B2 storage authorization")?;
    let capabilities = storage
        .pointer("/allowed/capabilities")
        .and_then(Value::as_array)
        .ok_or("missing capabilities")?;
    if !capabilities
        .iter()
        .any(|capability| capability.as_str() == Some("listBuckets"))
    {
        return Err("bucket lookup not authorized");
    }
    let s3 = storage
        .get("s3ApiUrl")
        .and_then(Value::as_str)
        .ok_or("missing B2 S3 endpoint")?;
    let host = s3
        .strip_prefix("https://")
        .ok_or("invalid B2 S3 endpoint")?;
    if !host.starts_with("s3.")
        || !host.ends_with(".backblazeb2.com")
        || host.contains(['/', ':', '@', '?', '#', '%'])
    {
        return Err("invalid B2 S3 endpoint");
    }
    let rest = repository
        .strip_prefix("s3:https://")
        .or_else(|| repository.strip_prefix("s3://"))
        .or_else(|| repository.strip_prefix("s3:"))
        .ok_or("restic repository is not a B2 S3 repository")?;
    let (repository_host, object_path) =
        rest.split_once('/').ok_or("invalid restic S3 repository")?;
    if repository_host != host || object_path.contains(['%', '?', '#', '@', '\\']) {
        return Err("restic endpoint does not match B2 authorization");
    }
    let mut components = object_path.split('/');
    let bucket_name = components
        .next()
        .filter(|name| {
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or("invalid restic bucket")?;
    let segments: Vec<_> = components.collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err("invalid restic repository prefix");
    }
    let expected_prefix = if segments.is_empty() {
        String::new()
    } else {
        format!("{}/", segments.join("/"))
    };
    if prefix != expected_prefix {
        return Err("replication prefix does not match restic repository");
    }
    let api_url = storage
        .get("apiUrl")
        .and_then(Value::as_str)
        .ok_or("missing B2 API URL")?;
    let api_host = api_url
        .strip_prefix("https://api")
        .and_then(|s| s.strip_suffix(".backblazeb2.com"))
        .ok_or("invalid B2 API URL")?;
    if api_host.is_empty() || !api_host.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid B2 API URL");
    }
    let token = authorization
        .get("authorizationToken")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && !v.bytes().any(|b| b.is_ascii_control()))
        .ok_or("missing B2 authorization")?;
    let account = authorization
        .get("accountId")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or("missing B2 account")?;
    let body =
        serde_json::json!({"accountId": account, "bucketId": bucket_id, "bucketTypes": ["all"]});
    let config = format!(
        "url = {}\nheader = {}\nheader = \"Content-Type: application/json\"\ndata = {}\n",
        quoted(&format!("{api_url}/b2api/v4/b2_list_buckets")),
        quoted(&format!("Authorization: {token}")),
        quoted(&body.to_string())
    );
    let response = curl_json(curl, &config)?;
    let buckets = response
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or("invalid B2 bucket response")?;
    if buckets.len() != 1
        || buckets[0].get("bucketId").and_then(Value::as_str) != Some(bucket_id)
        || buckets[0].get("bucketName").and_then(Value::as_str) != Some(bucket_name)
    {
        return Err("restic bucket does not match B2 bucket");
    }
    Ok(())
}

/// Fetch pages without putting the B2 authorization token or private object
/// names in process arguments. `api_url` comes from B2 authorization.
pub fn verify_remote(
    curl: &Path,
    authorization: &Value,
    bucket_id: &str,
    prefix: &str,
) -> Result<usize, &'static str> {
    authorize_scope(authorization, bucket_id, prefix)?;
    let api_url = authorization
        .pointer("/apiInfo/storageApi/apiUrl")
        .and_then(Value::as_str)
        .ok_or("missing B2 API URL")?;
    let token = authorization
        .get("authorizationToken")
        .and_then(Value::as_str)
        .ok_or("missing B2 authorization")?;
    let host = api_url
        .strip_prefix("https://api")
        .and_then(|s| s.strip_suffix(".backblazeb2.com"))
        .ok_or("invalid B2 API URL")?;
    if host.is_empty()
        || !host.bytes().all(|b| b.is_ascii_digit())
        || token.is_empty()
        || token.bytes().any(|b| b.is_ascii_control())
        || bucket_id.is_empty()
    {
        return Err("invalid B2 API credentials or endpoint");
    }
    verify(prefix, |cursor| {
        let mut body =
            serde_json::json!({"bucketId": bucket_id, "prefix": prefix, "maxFileCount": 1000});
        if let Some(cursor) = cursor {
            body["startFileName"] = cursor.file_name.clone().into();
            if let Some(id) = &cursor.file_id {
                body["startFileId"] = id.clone().into();
            }
        }
        let config = format!(
            "url = {}\nheader = {}\nheader = \"Content-Type: application/json\"\ndata = {}\n",
            quoted(&format!("{api_url}/b2api/v4/b2_list_file_versions")),
            quoted(&format!("Authorization: {token}")),
            quoted(&body.to_string())
        );
        curl_json(curl, &config)
    })
}

/// Success requires all pages and at least one uploaded version.
pub fn verify<F>(prefix: &str, mut fetch: F) -> Result<usize, &'static str>
where
    F: FnMut(Option<&Cursor>) -> Result<Value, &'static str>,
{
    let mut cursor: Option<Cursor> = None;
    let mut seen_cursors = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut uploads = 0;
    loop {
        let page = fetch(cursor.as_ref())?;
        let files = page
            .get("files")
            .and_then(Value::as_array)
            .ok_or("invalid replication page")?;
        for file in files {
            let name = file
                .get("fileName")
                .and_then(Value::as_str)
                .ok_or("missing file name")?;
            if !name.starts_with(prefix) {
                return Err("unexpected object prefix");
            }
            match file.get("action").and_then(Value::as_str) {
                Some("upload") => {
                    let id = file
                        .get("fileId")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                        .ok_or("missing file version id")?;
                    if !seen_ids.insert(id.to_owned()) {
                        return Err("duplicate file version");
                    }
                    if file.get("replicationStatus").and_then(Value::as_str) != Some("COMPLETED") {
                        return Err("replication incomplete or unknown");
                    }
                    uploads += 1;
                }
                Some("hide") => {}
                _ => return Err("unknown or unfinished file action"),
            }
        }
        let name = page.get("nextFileName").ok_or("missing pagination state")?;
        let id = page.get("nextFileId").ok_or("missing pagination state")?;
        if name.is_null() && id.is_null() {
            break;
        }
        let name = name
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or("invalid pagination state")?;
        let id = Some(
            id.as_str()
                .filter(|id| !id.is_empty())
                .ok_or("invalid pagination state")?
                .to_owned(),
        );
        let next = Cursor {
            file_name: name.to_owned(),
            file_id: id,
        };
        if !seen_cursors.insert(next.clone()) {
            return Err("repeated pagination cursor");
        }
        cursor = Some(next);
    }
    if uploads == 0 {
        return Err("no uploaded objects verified");
    }
    Ok(uploads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verifies_all_pages_and_versions() {
        let pages = [
            json!({"files":[{"action":"upload","fileName":"r/a","fileId":"1","replicationStatus":"COMPLETED"}],"nextFileName":"r/a","nextFileId":"2"}),
            json!({"files":[{"action":"upload","fileName":"r/a","fileId":"2","replicationStatus":"COMPLETED"},{"action":"hide","fileName":"r/b"}],"nextFileName":null,"nextFileId":null}),
        ];
        let mut count = 0;
        assert_eq!(
            verify("r/", |cursor| {
                if count == 1 {
                    assert_eq!(cursor.unwrap().file_id.as_deref(), Some("2"));
                }
                let value = pages[count].clone();
                count += 1;
                Ok(value)
            }),
            Ok(2)
        );
    }

    #[test]
    fn incomplete_unknown_and_missing_fail_closed() {
        for status in [
            json!("PENDING"),
            json!("FAILED"),
            json!("REPLICA"),
            json!(null),
            json!("future"),
        ] {
            let page = json!({"files":[{"action":"upload","fileName":"r/a","fileId":"1","replicationStatus":status}],"nextFileName":null,"nextFileId":null});
            assert!(verify("r/", |_| Ok(page.clone())).is_err());
        }
        assert!(verify("r/", |_| Err("network")).is_err());
        assert!(verify("r/", |_| Ok(
            json!({"files":[],"nextFileName":null,"nextFileId":null})
        ))
        .is_err());
        assert!(verify("r/", |_| Ok(json!({"files":[]}))).is_err());
        assert!(verify("r/", |_| Ok(
            json!({"files":[],"nextFileName":"r/a","nextFileId":"1"})
        ))
        .is_err());
    }

    #[test]
    fn rejects_wrong_prefix_duplicate_versions_and_unfinished_actions() {
        let outside = json!({"files":[{"action":"upload","fileName":"other/a","fileId":"1","replicationStatus":"COMPLETED"}],"nextFileName":null,"nextFileId":null});
        assert!(verify("r/", |_| Ok(outside.clone())).is_err());
        let duplicate = json!({"files":[{"action":"upload","fileName":"r/a","fileId":"1","replicationStatus":"COMPLETED"},{"action":"upload","fileName":"r/a","fileId":"1","replicationStatus":"COMPLETED"}],"nextFileName":null,"nextFileId":null});
        assert!(verify("r/", |_| Ok(duplicate.clone())).is_err());
        for action in ["start", "folder", "future"] {
            let page = json!({"files":[{"action":action,"fileName":"r/a"}],"nextFileName":null,"nextFileId":null});
            assert!(verify("r/", |_| Ok(page.clone())).is_err());
        }
    }

    #[test]
    fn remote_sends_token_in_stdin_not_arguments() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("curl");
        std::fs::write(&path, r##"#!/bin/sh
case "$*" in *supersecret*) exit 2;; esac
found=no
while IFS= read -r line; do case "$line" in *'Authorization: supersecret'*) found=yes;; esac; done
[ "$found" = yes ] || exit 3
printf '%s\n' '{"files":[{"action":"upload","fileName":"r/a","fileId":"1","replicationStatus":"COMPLETED"}],"nextFileName":null,"nextFileId":null}'
"##).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        for attempt in 0..50 {
            match Command::new(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .status()
            {
                Err(error) if error.raw_os_error() == Some(26) && attempt < 49 => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                result => {
                    result.unwrap();
                    break;
                }
            }
        }
        let auth = json!({"authorizationToken":"supersecret","apiInfo":{"storageApi":{"apiUrl":"https://api001.backblazeb2.com","allowed":{"buckets":[{"id":"bucket"}],"namePrefix":"r/","capabilities":["listFiles"]}}}});
        let result = verify_remote(&path, &auth, "bucket", "r/");
        assert_eq!(result, Ok(1));
        assert!(verify_remote(Path::new("/does-not-exist"), &auth, "wrong-bucket", "r/").is_err());
        assert!(verify_remote(Path::new("/does-not-exist"), &auth, "bucket", "").is_err());
    }

    #[test]
    fn repository_must_match_authorized_endpoint_bucket_and_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let curl = temp.path().join("curl");
        std::fs::write(&curl, "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"buckets\":[{\"bucketId\":\"bucket-id\",\"bucketName\":\"sample-bucket\"}]}'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o700)).unwrap();
        for attempt in 0..50 {
            match Command::new(&curl)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .status()
            {
                Err(error) if error.raw_os_error() == Some(26) && attempt < 49 => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                result => {
                    result.unwrap();
                    break;
                }
            }
        }
        let auth = json!({"accountId":"account","authorizationToken":"token","apiInfo":{"storageApi":{"apiUrl":"https://api001.backblazeb2.com","s3ApiUrl":"https://s3.us-west-001.backblazeb2.com","allowed":{"buckets":[{"id":"bucket-id"}],"namePrefix":null,"capabilities":["listFiles","listBuckets"]}}}});
        let repo = "s3:https://s3.us-west-001.backblazeb2.com/sample-bucket/restic";
        assert!(verify_repository(&curl, &auth, repo, "bucket-id", "restic/").is_ok());
        for (candidate, id, prefix) in [
            (
                "s3:https://s3.us-west-002.backblazeb2.com/sample-bucket/restic",
                "bucket-id",
                "restic/",
            ),
            (
                "s3:https://s3.us-west-001.backblazeb2.com/other-bucket/restic",
                "bucket-id",
                "restic/",
            ),
            (repo, "other-id", "restic/"),
            (repo, "bucket-id", "other/"),
            (
                "s3:https://s3.us-west-001.backblazeb2.com/sample-bucket/a/../restic",
                "bucket-id",
                "restic/",
            ),
            (
                "s3:https://s3.us-west-001.backblazeb2.com/sample-bucket/%72estic",
                "bucket-id",
                "restic/",
            ),
        ] {
            assert!(verify_repository(&curl, &auth, candidate, id, prefix).is_err());
        }
    }
}
