//! playwright-mcp-launcher — Playwright MCP の起動ラッパー。
//!
//! 普段の Firefox のプロファイル(ログイン状態を含む)を専用の複製へ `rsync` で
//! 差分同期し、その複製を `--user-data-dir` にして素の Firefox
//! (`--browser moz-firefox`)で Playwright MCP を起動する。`~/.claude.json` の
//! `mcpServers.playwright` がこのラッパーを指す(`home/identities/personal.nix`)。
//!
//! 普段のプロファイルを直接渡さない理由: Playwright は Firefox の起動のたびに
//! プロファイルへ `user.js`(セーフブラウジング・トラッキング防止の無効化、パスワード
//! 保存の無効化など)を書き、`prefs.js` を退避する。直接渡すと、普段の Firefox にも
//! その設定が効いて残る。複製だけを渡せば、その書き込みが普段のプロファイルに届く
//! 経路が無い。同期は一方向で、エージェント側で起きたログインや変更は次の起動の
//! 同期で消える。
//!
//! 縮退はしない: プロファイルが決まらない・Firefox 不在・rsync 失敗は stderr に
//! 診断を出して exit 1 する(esa-mcp-launcher と同じ。黙って別のブラウザや空の
//! プロファイルで動き続けると、未ログインの原因が見えなくなる)。
//!
//! stdout は MCP のプロトコルが使うので、rsync の出力は捨てる(診断は stderr)。
//!
//! 設計と根拠: docs/claude/claude-mcp-servers.md

use playwright_mcp_launcher::{
    default_profile_from_installs, find_in_path, npx_args, overlaps, parse_args, parse_lock_pid,
    process_alive, rsync_args, Args, InstallsError,
};
use process_control::command_exists;
use std::fs::{self, DirBuilder, OpenOptions};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// 診断(1 行目 + 任意のヒント行)。
struct Failure {
    msg: String,
    hint: Option<String>,
}

fn fail(msg: impl Into<String>, hint: Option<&str>) -> Failure {
    Failure {
        msg: msg.into(),
        hint: hint.map(str::to_string),
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args_os().skip(1)) {
        Ok(a) => a,
        Err(m) => {
            eprintln!("playwright-mcp-launcher: {m}");
            eprintln!("  使い方: --rsync <path> --clone <dir> [--source-profile <dir>] [--firefox-dir <dir>] [--sync-only] [-- <Playwright MCP への引数>]");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(code) => code,
        Err(f) => {
            eprintln!("playwright-mcp-launcher: {}", f.msg);
            if let Some(h) = f.hint {
                eprintln!("  {h}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<ExitCode, Failure> {
    let source = source_profile(&args)?;
    sync(&args, &source)?;
    if args.sync_only {
        return Ok(ExitCode::SUCCESS);
    }

    let executable = firefox_executable()?;
    if !command_exists("npx") {
        return Err(fail(
            "npx が見つからない(mise の node runtime を確認)",
            None,
        ));
    }
    let err = Command::new("npx")
        .args(npx_args(
            &args.clone,
            executable.as_deref(),
            &args.passthrough,
        ))
        .exec();
    // exec が返るのは起動失敗のときだけ(シェルの `exec: not found` と同じ 127)。
    eprintln!("playwright-mcp-launcher: npx の起動に失敗した: {err}");
    Ok(ExitCode::from(127))
}

/// 普段の Firefox のプロファイル(同期元)を決める。
fn source_profile(args: &Args) -> Result<PathBuf, Failure> {
    if let Some(p) = &args.source_profile {
        return Ok(p.clone());
    }
    let dir = match &args.firefox_dir {
        Some(d) => d.clone(),
        None => default_firefox_dir()?,
    };
    let ini_path = dir.join("installs.ini");
    let ini = fs::read_to_string(&ini_path).map_err(|e| {
        fail(
            format!("{} を読めない: {e}", ini_path.display()),
            Some("Firefox を一度起動してプロファイルを作るか、--source-profile で指定する"),
        )
    })?;
    match default_profile_from_installs(&ini) {
        Ok(rel) => {
            let p = PathBuf::from(&rel);
            Ok(if p.is_absolute() { p } else { dir.join(p) })
        }
        Err(InstallsError::None) => Err(fail(
            format!("{} に Default= を持つ節が無い", ini_path.display()),
            Some("--source-profile で普段のプロファイルを指定する"),
        )),
        Err(InstallsError::Multiple(n)) => Err(fail(
            format!(
                "{} に Default= を持つ節が {n} 個あり、普段使いを決められない",
                ini_path.display()
            ),
            Some("--source-profile で普段のプロファイルを指定する"),
        )),
    }
}

fn default_firefox_dir() -> Result<PathBuf, Failure> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| fail("HOME が未設定", None))?;
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Firefox")
    } else {
        home.join(".mozilla/firefox")
    })
}

/// 普段のプロファイルを複製へ一方向に同期する。複製の Firefox が使用中なら飛ばす。
fn sync(args: &Args, source: &Path) -> Result<(), Failure> {
    let source = source.canonicalize().map_err(|e| {
        fail(
            format!("普段のプロファイルを開けない: {}: {e}", source.display()),
            Some("--source-profile のパス、または installs.ini の Default= を確認する"),
        )
    })?;
    if !args.rsync.is_file() {
        return Err(fail(
            format!("rsync が無い: {}", args.rsync.display()),
            None,
        ));
    }

    if let Some(parent) = args.clone.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            fail(
                format!("複製の親を作れない: {}: {e}", parent.display()),
                None,
            )
        })?;
    }
    // 複製にはログイン状態が入るので、本人だけが読める。
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&args.clone)
        .map_err(|e| {
            fail(
                format!("複製を作れない: {}: {e}", args.clone.display()),
                None,
            )
        })?;
    let clone = args.clone.canonicalize().map_err(|e| {
        fail(
            format!("複製のパスを解決できない: {}: {e}", args.clone.display()),
            None,
        )
    })?;
    // `rsync --delete` が普段のプロファイルや、その兄弟(profiles.ini、他の
    // プロファイル)を消す配置を拒否する。
    if overlaps(&source, &clone) {
        return Err(fail(
            format!(
                "複製 {} と普段のプロファイル {} が重なっている",
                clone.display(),
                source.display()
            ),
            Some("--clone は普段のプロファイルと無関係な場所にする(例: ~/.cache/playwright-mcp-profile/firefox)"),
        ));
    }

    // 同時に起動した MCP が同じ複製を同期しないよう、複製の隣のロックで直列化する。
    let mut lock_path = clone.as_os_str().to_owned();
    lock_path.push(".sync.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| fail(format!("同期のロックを開けない: {e}"), None))?;
    lock.lock()
        .map_err(|e| fail(format!("同期のロックを取れない: {e}"), None))?;

    if let Some(pid) = clone_in_use(&clone) {
        eprintln!(
            "playwright-mcp-launcher: 複製の Firefox が使用中(pid {pid})のため、同期を飛ばす"
        );
        return Ok(());
    }

    let status = Command::new(&args.rsync)
        .args(rsync_args(&source, &clone))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| fail(format!("rsync を起動できない: {e}"), None))?;
    // 24 は「転送中に元のファイルが消えた」。使用中のプロファイルでは起こりうる
    // 良性の結果なので成功に数える。
    match status.code() {
        Some(0) | Some(24) => Ok(()),
        other => Err(fail(
            format!("rsync が失敗した(終了コード {other:?})"),
            Some("普段のプロファイルと複製の置き場の空き容量・権限を確認する"),
        )),
    }
}

/// 複製の `lock`(Firefox が起動中に張る symlink)が生きた process を指していれば
/// その pid。
fn clone_in_use(clone: &Path) -> Option<u32> {
    let target = fs::read_link(clone.join("lock")).ok()?;
    let pid = parse_lock_pid(&target.to_string_lossy())?;
    process_alive(pid).then_some(pid)
}

/// Playwright に渡す Firefox の実行ファイル。macOS は Playwright の既定の場所
/// (`/Applications/Firefox.app/…`)に任せる(`None`)。Linux は PATH の `firefox` を
/// 実体まで解決する(apt の `/usr/bin/firefox` は symlink、snap は `/snap/bin/firefox`)。
fn firefox_executable() -> Result<Option<PathBuf>, Failure> {
    if cfg!(target_os = "macos") {
        return Ok(None);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = find_in_path("firefox", &path).ok_or_else(|| {
        fail(
            "PATH に firefox が見つからない",
            Some("Firefox を入れる(Linux は apt。system layer、ADR-0001)"),
        )
    })?;
    found.canonicalize().map(Some).map_err(|e| {
        fail(
            format!("firefox のパスを解決できない: {}: {e}", found.display()),
            None,
        )
    })
}
