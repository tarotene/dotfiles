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
//! 複製の Firefox が使用中(複製の `lock` が生きた process を指す)のときは、同じ複製を
//! 渡すと「Firefox is already running」で起動に失敗するので、`<clone>-<pid>` の新しい
//! 複製(世代)へ同期して、そちらで起動する。使用中でない世代は起動のたびに消し、
//! 使用中の世代が `MAX_GENERATIONS` に達したら診断して終わる。
//!
//! stdout は MCP のプロトコルが使うので、rsync の出力は捨てる(診断は stderr)。
//!
//! 設計と根拠: docs/claude/claude-mcp-servers.md

use playwright_mcp_launcher::{
    default_profile_from_installs, find_in_path, generation_path, lock_owner, npx_args, overlaps,
    parse_args, parse_generation_pid, plan_generations, process_alive, rsync_args, Args,
    Generation, InstallsError, MAX_GENERATIONS,
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
    let clone = sync(&args, &source)?;
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
        .args(npx_args(&clone, executable.as_deref(), &args.passthrough))
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

/// 普段のプロファイルを複製へ一方向に同期し、Firefox を起動する複製の場所を返す。
/// 複製の Firefox が使用中なら、`<clone>-<pid>` の新しい世代へ同期してそれを返す
/// (`--sync-only` のときは世代を作らず、同期を飛ばす)。
fn sync(args: &Args, source: &Path) -> Result<PathBuf, Failure> {
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
    let clone = create_private_dir(&args.clone, "複製")?;
    check_overlap(&source, &clone)?;

    // 同時に起動した MCP が同じ複製・世代を選ばないよう、複製の隣のロックで直列化する。
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

    let generations = prune_generations(&clone);

    let target = match lock_owner(&clone) {
        Some(pid) if args.sync_only => {
            eprintln!(
                "playwright-mcp-launcher: 複製の Firefox が使用中(pid {pid})のため、同期を飛ばす"
            );
            return Ok(clone);
        }
        Some(pid) => {
            if generations >= MAX_GENERATIONS {
                return Err(fail(
                    format!(
                        "複製の Firefox が使用中(pid {pid})で、使用中の世代も上限({MAX_GENERATIONS})に達している"
                    ),
                    Some("不要な Playwright MCP のセッションを閉じる"),
                ));
            }
            let generation = generation_path(&clone, std::process::id());
            eprintln!(
                "playwright-mcp-launcher: 複製の Firefox が使用中(pid {pid})のため、新しい複製 {} へ同期して起動する",
                generation.display()
            );
            let generation = create_private_dir(&generation, "新しい複製")?;
            check_overlap(&source, &generation)?;
            generation
        }
        None => clone,
    };

    let status = Command::new(&args.rsync)
        .args(rsync_args(&source, &target))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| fail(format!("rsync を起動できない: {e}"), None))?;
    // 24 は「転送中に元のファイルが消えた」。使用中のプロファイルでは起こりうる
    // 良性の結果なので成功に数える。
    match status.code() {
        Some(0) | Some(24) => Ok(target),
        other => Err(fail(
            format!("rsync が失敗した(終了コード {other:?})"),
            Some("普段のプロファイルと複製の置き場の空き容量・権限を確認する"),
        )),
    }
}

/// 複製(ログイン状態が入るので、本人だけが読める)を作り、正規化したパスを返す。
fn create_private_dir(path: &Path, what: &str) -> Result<PathBuf, Failure> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|e| fail(format!("{what}を作れない: {}: {e}", path.display()), None))?;
    path.canonicalize().map_err(|e| {
        fail(
            format!("{what}のパスを解決できない: {}: {e}", path.display()),
            None,
        )
    })
}

/// `rsync --delete` が普段のプロファイルや、その兄弟(profiles.ini、他の
/// プロファイル)を消す配置を拒否する。
fn check_overlap(source: &Path, clone: &Path) -> Result<(), Failure> {
    if overlaps(source, clone) {
        return Err(fail(
            format!(
                "複製 {} と普段のプロファイル {} が重なっている",
                clone.display(),
                source.display()
            ),
            Some("--clone は普段のプロファイルと無関係な場所にする(例: ~/.cache/playwright-mcp-profile/firefox)"),
        ));
    }
    Ok(())
}

/// 複製の隣の世代(`<clone>-<pid>`)のうち使用中でないものを消し、使用中の世代の数を
/// 返す。使用中は、持ち主のラッパー(pid)が生きている、または `lock` が生きた
/// Firefox を指している。自分の pid の世代は数えない。消せなくても起動は止めず、
/// 診断だけ出す(次の起動でまた試す)。
fn prune_generations(clone: &Path) -> usize {
    let (Some(parent), Some(base)) = (clone.parent(), clone.file_name()) else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return 0;
    };
    let me = std::process::id();
    let gens: Vec<Generation> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let pid = parse_generation_pid(base, &e.file_name())?;
            (pid != me).then(|| Generation {
                pid,
                in_use: process_alive(pid) || lock_owner(&e.path()).is_some(),
            })
        })
        .collect();
    let plan = plan_generations(&gens);
    for pid in &plan.remove {
        let dir = generation_path(clone, *pid);
        if let Err(e) = fs::remove_dir_all(&dir) {
            eprintln!(
                "playwright-mcp-launcher: 使用中でない複製 {} を消せない: {e}",
                dir.display()
            );
        }
    }
    gens.len() - plan.remove.len()
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
