//! playwright-mcp-launcher の副作用を持たない部分(引数の解釈、installs.ini の
//! 解釈、Firefox の lock の解釈、npx / rsync への引数の組み立て)。副作用のある
//! 本体は `main.rs`。設計と根拠: docs/claude/claude-mcp-servers.md

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// 起動する Playwright MCP の版。`@latest` のままだと npx のキャッシュが更新される
/// たびに別のサーバーが動くので pin する。上げるときはここを意図して変える
/// (かつて `home/identities/personal.nix` の args にあった pin の移転先、#849)。
pub const PLAYWRIGHT_MCP: &str = "@playwright/mcp@0.0.83";

/// 普段のプロファイルから複製へ写さないもの(`rsync --exclude`。`--delete` の
/// 削除からも守られる)。先頭の `/` はプロファイル直下に固定する印。
///
/// - `lock` / `.parentlock`: Firefox の使用中の印。複製側のものは複製の Firefox の
///   もので、普段の Firefox のものを写すと複製が「使用中」に見える。
/// - `user.js` / `prefs.js.playwright`: Playwright が複製の側に書く自動操作用の設定。
///   普段側に有っても写さず、複製側で消さない。
/// - 残りは、複製に要らない一時的なもの(セッション復元、クラッシュ記録、計測、
///   キャッシュ)。
pub const EXCLUDES: &[&str] = &[
    "/lock",
    "/.parentlock",
    "/user.js",
    "/prefs.js.playwright",
    "/sessionstore.jsonlz4",
    "/sessionstore-backups/",
    "/crashes/",
    "/minidumps/",
    "/datareporting/",
    "/cache2/",
    "/startupCache/",
];

/// コマンドライン。`--` より後ろは npx(Playwright MCP)へ素通しする。
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub rsync: PathBuf,
    pub clone: PathBuf,
    pub source_profile: Option<PathBuf>,
    pub firefox_dir: Option<PathBuf>,
    pub sync_only: bool,
    pub passthrough: Vec<OsString>,
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut rsync = None;
    let mut clone = None;
    let mut source_profile = None;
    let mut firefox_dir = None;
    let mut sync_only = false;
    let mut passthrough = Vec::new();

    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{name} に値が無い"))
        };
        match a.to_str() {
            Some("--rsync") => rsync = Some(value("--rsync")?),
            Some("--clone") => clone = Some(value("--clone")?),
            Some("--source-profile") => source_profile = Some(value("--source-profile")?),
            Some("--firefox-dir") => firefox_dir = Some(value("--firefox-dir")?),
            Some("--sync-only") => sync_only = true,
            Some("--") => {
                passthrough.extend(it.by_ref());
                break;
            }
            _ => return Err(format!("知らない引数: {}", a.to_string_lossy())),
        }
    }
    Ok(Args {
        rsync: rsync.ok_or("--rsync が必要(Nix が store パスを渡す)")?,
        clone: clone.ok_or("--clone が必要(複製の置き場)")?,
        source_profile,
        firefox_dir,
        sync_only,
        passthrough,
    })
}

/// `installs.ini` から、普段使いのプロファイル(`Default=`)を 1 つに決める。
/// `Default=` を持つ節がちょうど 1 つのときだけ決まる。0 個や複数(Firefox を
/// 複数の場所にインストールしている)は、どれが普段使いか決められない。
#[derive(Debug, PartialEq, Eq)]
pub enum InstallsError {
    None,
    Multiple(usize),
}

pub fn default_profile_from_installs(ini: &str) -> Result<String, InstallsError> {
    let mut defaults = Vec::new();
    for line in ini.lines() {
        if let Some(v) = line.trim().strip_prefix("Default=") {
            if !v.is_empty() {
                defaults.push(v.to_string());
            }
        }
    }
    match defaults.len() {
        0 => Err(InstallsError::None),
        1 => Ok(defaults.remove(0)),
        n => Err(InstallsError::Multiple(n)),
    }
}

/// Firefox の `lock`(symlink)の行き先 `<ip>:+<pid>` から pid を取り出す。
pub fn parse_lock_pid(target: &str) -> Option<u32> {
    target.rsplit_once(":+")?.1.parse().ok()
}

/// `kill(pid, 0)` が通る(EPERM も、別の利用者の生きた process なので生存)。
pub fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: シグナル 0 は存在確認だけで、何も送らない。
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// `src` と `dst` が同じ場所、または一方が他方の内側にあるか(正規化済みの絶対パス)。
/// `rsync --delete` は dst の中の余計なものを消すので、dst が src の親だと
/// 普段のプロファイルの兄弟(`profiles.ini` や他のプロファイル)まで消えうる。
pub fn overlaps(src: &Path, dst: &Path) -> bool {
    src.starts_with(dst) || dst.starts_with(src)
}

/// `rsync -a --delete --exclude=… <src>/ <dst>/` の引数。
pub fn rsync_args(src: &Path, dst: &Path) -> Vec<OsString> {
    let mut v: Vec<OsString> = vec!["-a".into(), "--delete".into()];
    v.extend(
        EXCLUDES
            .iter()
            .map(|e| OsString::from(format!("--exclude={e}"))),
    );
    v.push(with_trailing_slash(src));
    v.push(with_trailing_slash(dst));
    v
}

fn with_trailing_slash(p: &Path) -> OsString {
    let mut s = p.as_os_str().to_owned();
    s.push("/");
    s
}

/// `npx` の引数。Firefox の実行ファイルが `None` のとき(macOS)は Playwright の
/// 既定の場所に任せる。
pub fn npx_args(
    clone: &Path,
    executable: Option<&Path>,
    passthrough: &[OsString],
) -> Vec<OsString> {
    let mut v: Vec<OsString> = vec![
        PLAYWRIGHT_MCP.into(),
        "--browser".into(),
        "moz-firefox".into(),
    ];
    if let Some(exe) = executable {
        v.push("--executable-path".into());
        v.push(exe.as_os_str().to_owned());
    }
    v.push("--user-data-dir".into());
    v.push(clone.as_os_str().to_owned());
    v.extend(passthrough.iter().cloned());
    v
}

/// `PATH` から実行ファイル名を探す(最初に見つかった実在のファイル)。
pub fn find_in_path(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn installs_one_default() {
        let ini = "[4F96D1932A9F858E]\nDefault=i3jnkg9i.default-release\nLocked=1\n";
        assert_eq!(
            default_profile_from_installs(ini),
            Ok("i3jnkg9i.default-release".to_string())
        );
    }

    #[test]
    fn installs_none_or_multiple() {
        assert_eq!(default_profile_from_installs(""), Err(InstallsError::None));
        assert_eq!(
            default_profile_from_installs("[A]\nLocked=1\n"),
            Err(InstallsError::None)
        );
        let two = "[A]\nDefault=x\n[B]\nDefault=y\n";
        assert_eq!(
            default_profile_from_installs(two),
            Err(InstallsError::Multiple(2))
        );
    }

    #[test]
    fn lock_pid() {
        assert_eq!(parse_lock_pid("100.90.53.85:+1502548"), Some(1502548));
        assert_eq!(parse_lock_pid("garbage"), None);
        assert_eq!(parse_lock_pid("1.2.3.4:+x"), None);
    }

    #[test]
    fn own_process_is_alive_and_absurd_pid_is_not() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(i32::MAX as u32));
        assert!(!process_alive(u32::MAX));
    }

    #[test]
    fn overlap_rules() {
        let src = Path::new("/h/.mozilla/firefox/p.default");
        assert!(overlaps(src, src));
        assert!(overlaps(src, Path::new("/h/.mozilla/firefox")));
        assert!(overlaps(
            src,
            Path::new("/h/.mozilla/firefox/p.default/clone")
        ));
        assert!(!overlaps(
            src,
            Path::new("/h/.cache/playwright-mcp-profile/firefox")
        ));
    }

    #[test]
    fn parse_args_full_and_passthrough() {
        let a = parse_args(os(&[
            "--rsync",
            "/r",
            "--clone",
            "/c",
            "--source-profile",
            "/s",
            "--firefox-dir",
            "/f",
            "--sync-only",
            "--",
            "--output-dir",
            "/o",
        ]))
        .unwrap();
        assert_eq!(a.rsync, PathBuf::from("/r"));
        assert_eq!(a.clone, PathBuf::from("/c"));
        assert_eq!(a.source_profile, Some(PathBuf::from("/s")));
        assert_eq!(a.firefox_dir, Some(PathBuf::from("/f")));
        assert!(a.sync_only);
        assert_eq!(a.passthrough, os(&["--output-dir", "/o"]));
    }

    #[test]
    fn parse_args_errors() {
        assert!(parse_args(os(&["--clone", "/c"])).is_err());
        assert!(parse_args(os(&["--rsync", "/r"])).is_err());
        assert!(parse_args(os(&["--rsync"])).is_err());
        assert!(parse_args(os(&["--rsync", "/r", "--clone", "/c", "--bogus"])).is_err());
    }

    #[test]
    fn rsync_args_shape() {
        let v = rsync_args(Path::new("/s"), Path::new("/d"));
        assert_eq!(v[0], "-a");
        assert_eq!(v[1], "--delete");
        assert!(v.contains(&OsString::from("--exclude=/lock")));
        assert!(v.contains(&OsString::from("--exclude=/user.js")));
        assert_eq!(v[v.len() - 2], "/s/");
        assert_eq!(v[v.len() - 1], "/d/");
    }

    #[test]
    fn npx_args_shape() {
        let v = npx_args(
            Path::new("/c"),
            Some(Path::new("/usr/lib/firefox/firefox")),
            &os(&["--output-dir", "/o"]),
        );
        assert_eq!(
            v,
            os(&[
                PLAYWRIGHT_MCP,
                "--browser",
                "moz-firefox",
                "--executable-path",
                "/usr/lib/firefox/firefox",
                "--user-data-dir",
                "/c",
                "--output-dir",
                "/o",
            ])
        );
        let none = npx_args(Path::new("/c"), None, &[]);
        assert!(!none.contains(&OsString::from("--executable-path")));
    }
}
