//! plan-view — プランを Markdown レンダリング済みの HTML にして Chrome の専用窓へ
//! 飛ばす(旧 `config/claude/hooks/plan-view.sh`、ADR-0024 Stage 4b、#412)。
//!
//! 設計と根拠: docs/claude/plan-view.md
//!
//! **構造の再解釈はしない。** 入力の Markdown を 1:1 で HTML(pandoc)に写すだけ。
//! LLM は呼ばない。
//!
//! - hook モード(引数なし): PreToolUse (matcher: ExitPlanMode) から stdin JSON で
//!   呼ばれる。**stdout に何も出さず、常に exit 0**(stdout の JSON は
//!   permissionDecision として解釈されるため。表示するだけの道具)。
//! - CLI モード: `plan-view [FILE|-] [--title T] [--no-open] [--out PATH]`
//!
//! スキップ手段: `touch ~/.claude/plan-views/skip` または `PLAN_VIEW_SKIP=1`
//!
//! 既知の罠(bash 版から引き継ぐ):
//! - ブラウザを同期起動すると承認ダイアログが出ない。必ず切り離して(待たずに)
//!   起動する。
//! - pandoc の組み込み CSS と skylighting の色は自前 CSS より前に出る。上書きは
//!   `--include-in-header` に依存している(plan-view.css の先頭コメント参照)。
//! - `--metadata pagetitle=` は `<title>` だけを設定する(`title=` にすると
//!   title-block が描かれ h1 と二重になる)。

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 環境変数由来の設定(bash 版の冒頭のグローバル)。
#[derive(Debug, Clone)]
pub struct Config {
    pub view_dir: PathBuf,
    pub pandoc: String,
    pub browser: String,
    pub highlight: String,
    pub window_size: String,
    pub retention_days: String,
    pub css_override: String,
    pub darwin_chrome_app: String,
    pub uname_override: String,
    /// bash の `SCRIPT_DIR`(`argv[0]` のディレクトリ。シンボリックリンクは解決しない)。
    pub script_dir: PathBuf,
    pub home: PathBuf,
}

/// `${NAME:-default}`(空文字も未設定扱い)。
fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

impl Config {
    pub fn from_env(argv0: Option<OsString>) -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let view_dir = match std::env::var_os("PLAN_VIEW_DIR") {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => home.join(".claude/plan-views"),
        };
        Config {
            view_dir,
            pandoc: env_or("PLAN_VIEW_PANDOC", "pandoc"),
            browser: env_or("PLAN_VIEW_BROWSER", "google-chrome"),
            highlight: env_or("PLAN_VIEW_HIGHLIGHT", "breezeDark"),
            window_size: env_or("PLAN_VIEW_WINDOW_SIZE", "1000,900"),
            retention_days: env_or("PLAN_VIEW_RETENTION_DAYS", "30"),
            css_override: std::env::var("PLAN_VIEW_CSS").unwrap_or_default(),
            darwin_chrome_app: env_or(
                "PLAN_VIEW_DARWIN_CHROME_APP",
                "/Applications/Google Chrome.app",
            ),
            uname_override: std::env::var("PLAN_VIEW_UNAME_OVERRIDE").unwrap_or_default(),
            script_dir: script_dir(argv0),
            home,
        }
    }

    pub fn is_darwin(&self) -> bool {
        match self.uname_override.as_str() {
            "Darwin" => true,
            "Linux" => false,
            _ => std::env::consts::OS == "macos",
        }
    }

    /// darwin のネイティブ GUI セッションは DISPLAY の概念を持たないので常に真(#230)。
    pub fn has_display(&self) -> bool {
        if self.is_darwin() {
            return true;
        }
        ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
    }

    /// ADR-0005: バイナリ(darwin では .app バンドル)の**存在**でゲートする。
    pub fn browser_available(&self) -> bool {
        if self.is_darwin() {
            Path::new(&self.darwin_chrome_app).is_dir()
        } else {
            hook_io::proc::command_exists(&self.browser)
        }
    }

    pub fn pandoc_available(&self) -> bool {
        hook_io::proc::command_exists(&self.pandoc)
    }

    /// CSS の探索。配備後は `~/.claude/hooks/` に本体と .css が並ぶので
    /// SCRIPT_DIR 相対で足りる。リポジトリ上では `config/claude/assets/` に
    /// 分離されている(ADR-0007)ので SCRIPT_DIR の一つ上の assets/ も探す。
    pub fn find_css(&self) -> Option<PathBuf> {
        let candidates = [
            PathBuf::from(&self.css_override),
            self.script_dir.join("plan-view.css"),
            self.script_dir.join("../assets/plan-view.css"),
            self.home.join(".claude/hooks/plan-view.css"),
        ];
        candidates
            .into_iter()
            .find(|c| !c.as_os_str().is_empty() && fs::File::open(c).is_ok())
    }

    /// `mkdir -p` + `chmod 700`、緩い権限で残っている生成物を 600 に締める
    /// (skip は残置してよい空ファイル)。
    pub fn ensure_dirs(&self) {
        let _ = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.view_dir);
        let _ = fs::set_permissions(&self.view_dir, fs::Permissions::from_mode(0o700));
        for (path, meta) in self.direct_files() {
            if meta.permissions().mode() & 0o077 != 0 {
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
        }
    }

    /// `find "$VIEW_DIR" -maxdepth 1 -type f ! -name skip`
    fn direct_files(&self) -> Vec<(PathBuf, fs::Metadata)> {
        let Ok(rd) = fs::read_dir(&self.view_dir) else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok())
            .filter(|e| e.file_name() != "skip")
            .filter_map(|e| {
                let meta = fs::symlink_metadata(e.path()).ok()?;
                meta.is_file().then(|| (e.path(), meta))
            })
            .collect()
    }

    /// 保持期限より古い生成物を掃除する(`find -mtime +N -delete`)。skip は
    /// エスケープハッチなので絶対に消さない。
    pub fn prune_old(&self) {
        let days: u64 = match self.retention_days.as_str() {
            "" | "0" => return,
            s if s.bytes().all(|b| b.is_ascii_digit()) => match s.parse() {
                Ok(d) => d,
                Err(_) => return,
            },
            _ => return,
        };
        let now = SystemTime::now();
        for (path, meta) in self.direct_files() {
            let Ok(mtime) = meta.modified() else { continue };
            let age = now.duration_since(mtime).unwrap_or(Duration::ZERO);
            // find の -mtime +N: 経過日数(切り捨て)が N より大きい。
            if age.as_secs() / 86400 > days {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// `pandoc` で HTML を作る。`$1=src md $2=title $3=out html`。
    pub fn render_html(&self, src: &Path, title: &str, out: &Path) -> bool {
        let mut args: Vec<OsString> = vec![
            "--from=gfm".into(),
            "--to=html5".into(),
            "--standalone".into(),
            format!("--highlight-style={}", self.highlight).into(),
            "--metadata".into(),
            format!("pagetitle={title}").into(),
        ];
        let mut hdr: Option<PathBuf> = None;
        if let Some(css) = self.find_css() {
            // --css= で <link> にすると単一ファイルにならない。<style> に包んで
            // --include-in-header で渡すと、pandoc の組み込み CSS と skylighting の
            // 色より後ろに入り、かつ 1 ファイルで完結する。
            let Some((path, mut f)) = mktemp(&self.view_dir, ".hdr.", ".html") else {
                return false;
            };
            let body = fs::read(&css).unwrap_or_default();
            let _ = f.write_all(b"<style>\n");
            let _ = f.write_all(&body);
            let _ = f.write_all(b"\n</style>\n");
            drop(f);
            let mut a = OsString::from("--include-in-header=");
            a.push(&path);
            args.push(a);
            hdr = Some(path);
        }
        args.push("-o".into());
        args.push(out.into());
        args.push(src.into());
        let ok = Command::new(&self.pandoc)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if let Some(h) = hdr {
            let _ = fs::remove_file(h);
        }
        ok
    }

    /// ブラウザを切り離して起動し、即座に戻る(待たない)。ここを同期にすると
    /// 承認ダイアログが出ない。darwin は `open -a`(LaunchServices に渡してすぐ戻る)。
    pub fn open_window(&self, html: &Path) {
        let mut url = OsString::from("file://");
        url.push(html);
        let mut app = OsString::from("--app=");
        app.push(&url);
        let size = format!("--window-size={}", self.window_size);
        let mut cmd = if self.is_darwin() {
            let mut c = Command::new("open");
            c.arg("-a").arg(&self.darwin_chrome_app).arg("--args");
            c
        } else if hook_io::proc::command_exists("setsid") {
            let mut c = Command::new("setsid");
            c.arg(&self.browser);
            c
        } else {
            Command::new(&self.browser)
        };
        cmd.arg(app)
            .arg(size)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // 子は待たない(bash の `&` + `disown`)。
        let _ = cmd.spawn();
    }

    /// プラン 1 本を HTML にして(必要なら)発射する。
    /// `$1=src md $2=title override(空可) $3=meta label(空可) $4=out html $5=open?`
    pub fn view_plan(&self, src: &Path, title: &str, label: &str, out: &Path, open: bool) -> bool {
        let mut title = title.to_string();
        if title.is_empty() {
            title = extract_h1(&fs::read(src).unwrap_or_default());
        }
        if title.is_empty() {
            let base = basename(&src.to_string_lossy());
            title = base.strip_suffix(".md").unwrap_or(&base).to_string();
        }

        let mut meta = String::from("<p class=\"plan-view-meta\">");
        if !label.is_empty() {
            meta.push_str(&html_escape(label));
            meta.push_str(" · ");
        }
        meta.push_str(&hook_io::proc::date("%Y-%m-%d %H:%M"));
        meta.push_str("</p>");

        let Some((tmp, mut f)) = mktemp(&self.view_dir, ".plan.", ".md") else {
            return false;
        };
        let src_bytes = fs::read(src).unwrap_or_default();
        let _ = f.write_all(&insert_meta(&src_bytes, &meta));
        drop(f);

        if !self.render_html(&tmp, &title, out) {
            let _ = fs::remove_file(&tmp);
            return false;
        }
        let _ = fs::remove_file(&tmp);
        let _ = fs::set_permissions(out, fs::Permissions::from_mode(0o600));
        if open {
            self.open_window(out);
        }
        true
    }
}

/// argv[0] のディレクトリ(`cd "$(dirname "$0")" && pwd` 相当、リンクは解決しない)。
/// argv[0] に `/` が無ければ実行ファイルの親。
fn script_dir(argv0: Option<OsString>) -> PathBuf {
    if let Some(a) = argv0 {
        let p = PathBuf::from(&a);
        if a.to_string_lossy().contains('/') {
            let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
            let dir = if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir
            };
            if dir.is_absolute() {
                return dir;
            }
            if let Ok(cwd) = std::env::current_dir() {
                return cwd.join(dir);
            }
            return dir;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// awk の `[[:space:]]`(C ロケールの空白 6 種)。
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `/^[[:space:]]*(```|~~~)/`
fn is_fence(line: &[u8]) -> bool {
    let i = line
        .iter()
        .position(|b| !is_space(*b))
        .unwrap_or(line.len());
    let rest = &line[i..];
    rest.starts_with(b"```") || rest.starts_with(b"~~~")
}

/// `/^#[[:space:]]+/`
fn is_h1(line: &[u8]) -> bool {
    line.len() >= 2 && line[0] == b'#' && is_space(line[1])
}

/// awk のレコード分割: `\n` 区切り。末尾の改行の後ろは空レコードにしない。
fn records(text: &[u8]) -> Vec<&[u8]> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix(b"\n").unwrap_or(text);
    body.split(|b| *b == b'\n').collect()
}

/// 本文の最初の h1 を返す(無ければ空文字)。フェンスの内側は見ない
/// (bash ブロック内の `# コメント` を h1 と取り違えないため)。
pub fn extract_h1(text: &[u8]) -> String {
    let mut fence = false;
    for line in records(text) {
        if is_fence(line) {
            fence = !fence;
            continue;
        }
        if !fence && is_h1(line) {
            let mut s = &line[1..];
            while let [first, rest @ ..] = s {
                if is_space(*first) {
                    s = rest;
                } else {
                    break;
                }
            }
            while let [rest @ .., last] = s {
                if is_space(*last) {
                    s = rest;
                } else {
                    break;
                }
            }
            return String::from_utf8_lossy(s).into_owned();
        }
    }
    String::new()
}

/// メタ行(リポジトリ名 · 時刻)を h1 の直下に差し込む。h1 が無ければ先頭に置く。
/// h1 とメタ行の間の空行は必須(詰めると gfm リーダが raw HTML を h1 の続きに飲み込む)。
pub fn insert_meta(src: &[u8], meta: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + meta.len() + 4);
    if extract_h1(src).is_empty() {
        out.extend_from_slice(meta.as_bytes());
        out.extend_from_slice(b"\n\n");
        out.extend_from_slice(src);
        return out;
    }
    let mut fence = false;
    let mut done = false;
    for line in records(src) {
        out.extend_from_slice(line);
        out.push(b'\n');
        if is_fence(line) {
            fence = !fence;
            continue;
        }
        if !done && !fence && is_h1(line) {
            out.push(b'\n');
            out.extend_from_slice(meta.as_bytes());
            out.push(b'\n');
            done = true;
        }
    }
    out
}

/// `sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'`
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `basename -- "$1"`
pub fn basename(s: &str) -> String {
    let t = s.trim_end_matches('/');
    if t.is_empty() {
        return if s.is_empty() {
            String::new()
        } else {
            "/".into()
        };
    }
    t.rsplit('/').next().unwrap_or(t).to_string()
}

/// `mktemp "$dir/<prefix>XXXXXX<suffix>"`(0600 で排他作成)。
pub fn mktemp(dir: &Path, prefix: &str, suffix: &str) -> Option<(PathBuf, fs::File)> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ ((std::process::id() as u64) << 32);
    for _ in 0..100 {
        let mut name = String::from(prefix);
        for _ in 0..6 {
            // xorshift64
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            name.push(ALPHABET[(seed % ALPHABET.len() as u64) as usize] as char);
        }
        name.push_str(suffix);
        let path = dir.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(f) => return Some((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// 生成物はプラン本文そのものを含むため、既定 umask に任せず 077 に落とす。
pub fn restrict_umask() {
    // SAFETY: umask はプロセス全体の属性を書き換えるだけで、メモリ安全性に関わらない。
    unsafe {
        libc::umask(0o077);
    }
}

const USAGE: &str = "使い方: plan-view [FILE|-] [--title T] [--no-open] [--out PATH]

  FILE      Markdown ファイル。`-` で標準入力。省略時は ~/.claude/plans/ の最新。
  --title   <title> と表示タイトル。既定は本文の最初の h1、無ければファイル名。
  --no-open ブラウザを開かず HTML を作るだけ。
  --out     HTML の出力先。既定は ~/.claude/plan-views/ 配下。

環境変数: PLAN_VIEW_DIR PLAN_VIEW_PANDOC PLAN_VIEW_BROWSER PLAN_VIEW_CSS
          PLAN_VIEW_HIGHLIGHT PLAN_VIEW_WINDOW_SIZE PLAN_VIEW_RETENTION_DAYS
無効化:   PLAN_VIEW_SKIP=1 または ~/.claude/plan-views/skip
";

fn die(msg: &str) -> i32 {
    eprintln!("plan-view: {msg}");
    1
}

fn latest_plan(cfg: &Config) -> Option<PathBuf> {
    // 入力が空なら 3 段目(最新の plans/*.md)だけが効く。
    let dir = cfg.home.join(".claude/plans");
    match hook_io::plan::plan_source_in(&hook_io::HookInput::default(), Some(&dir))? {
        hook_io::plan::PlanSource::File(p) => Some(p),
        hook_io::plan::PlanSource::Inline(_) => None,
    }
}

/// bash の `$PWD`(論理パス)。環境の PWD が今いる場所を指していればそれ、
/// 無ければ getcwd。
fn logical_pwd() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(pwd) = std::env::var_os("PWD").map(PathBuf::from) {
        if pwd.is_absolute() && fs::canonicalize(&pwd).ok() == fs::canonicalize(&cwd).ok() {
            return pwd;
        }
    }
    cwd
}

/// CLI モード。戻り値は終了コード。
pub fn run_cli(cfg: &Config, args: &[String]) -> i32 {
    let mut src = String::new();
    let mut title = String::new();
    let mut out = String::new();
    let mut open = true;

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--title" => {
                if i + 1 >= args.len() {
                    return die("--title に値がありません");
                }
                title = args[i + 1].clone();
                i += 2;
            }
            "--out" => {
                if i + 1 >= args.len() {
                    return die("--out に値がありません");
                }
                out = args[i + 1].clone();
                i += 2;
            }
            "--no-open" => {
                open = false;
                i += 1;
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            "--" => {
                if let Some(next) = args.get(i + 1) {
                    src = next.clone();
                }
                break;
            }
            // `-` は標準入力。下の `-*` より前に判定する。
            "-" => {
                if !src.is_empty() {
                    return die("ファイルは 1 つだけ指定してください");
                }
                src = "-".into();
                i += 1;
            }
            _ if a.starts_with('-') => return die(&format!("未知のフラグ: {a}")),
            _ => {
                if !src.is_empty() {
                    return die("ファイルは 1 つだけ指定してください");
                }
                src = a.to_string();
                i += 1;
            }
        }
    }

    if !cfg.pandoc_available() {
        return die(&format!(
            "pandoc が見つかりません (PLAN_VIEW_PANDOC={})",
            cfg.pandoc
        ));
    }

    cfg.ensure_dirs();
    cfg.prune_old();

    let mut tmp: Option<PathBuf> = None;
    let src_path: PathBuf = if src == "-" {
        let Some((path, mut f)) = mktemp(&cfg.view_dir, ".stdin.", ".md") else {
            return die("一時ファイルを作れません");
        };
        let _ = std::io::copy(&mut std::io::stdin(), &mut f);
        drop(f);
        tmp = Some(path.clone());
        path
    } else if src.is_empty() {
        match latest_plan(cfg) {
            Some(p) => p,
            None => return die("~/.claude/plans/ にプランがありません"),
        }
    } else {
        PathBuf::from(&src)
    };
    if !src_path.is_file() {
        return die(&format!("ファイルがありません: {}", src_path.display()));
    }

    let out_path = if out.is_empty() {
        cfg.view_dir
            .join(format!("cli-{}.html", hook_io::proc::date("%Y%m%d-%H%M%S")))
    } else {
        PathBuf::from(&out)
    };

    if open {
        if cfg.is_darwin() {
            if !cfg.browser_available() {
                return die(&format!(
                    "Google Chrome.app が見つかりません ({})",
                    cfg.darwin_chrome_app
                ));
            }
        } else if !cfg.browser_available() {
            return die(&format!(
                "ブラウザが見つかりません (PLAN_VIEW_BROWSER={})",
                cfg.browser
            ));
        }
        if !cfg.has_display() {
            return die(
                "DISPLAY / WAYLAND_DISPLAY がありません（--no-open なら HTML だけ作れます）",
            );
        }
    }

    let label = basename(&logical_pwd().to_string_lossy());
    let ok = cfg.view_plan(&src_path, &title, &label, &out_path, open);
    if let Some(t) = &tmp {
        let _ = fs::remove_file(t);
    }
    if !ok {
        return die("HTML の生成に失敗しました");
    }
    println!("{}", out_path.display());
    0
}

/// hook モード。何があっても stdout に書かない(呼び出し側で exit 0)。
pub fn run_hook(cfg: &Config, stdin: &str) {
    // --- エスケープハッチ ---
    if cfg.view_dir.join("skip").exists() || std::env::var("PLAN_VIEW_SKIP").is_ok_and(|v| v == "1")
    {
        return;
    }
    // --- バイナリ存在でゲート(ADR-0005) ---
    if !cfg.pandoc_available() || !cfg.browser_available() {
        return;
    }
    // --- 画面が無いセッション(SSH 経由など)では何もしない ---
    if !cfg.has_display() {
        return;
    }

    cfg.ensure_dirs();
    cfg.prune_old();

    // 不正 JSON でも bash 版は jq の失敗を `|| echo unknown` 等で吸収して
    // 最新プランの描画に進む。同じく既定値で続行する。
    let input = hook_io::HookInput::parse(stdin).unwrap_or_else(|| hook_io::HookInput {
        session_id: "unknown".into(),
        ..Default::default()
    });
    let session = hook_io::ledger::sanitize_session_id(&input.session_id);
    let cwd = input
        .cwd
        .clone()
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| cfg.home.clone());

    // --- プラン本文の取得: tool_input.plan → planFilePath → 最新の plans/*.md ---
    let plans_dir = cfg.home.join(".claude/plans");
    let mut plan_tmp: Option<PathBuf> = None;
    let plan_file = match hook_io::plan::plan_source_in(&input, Some(&plans_dir)) {
        Some(hook_io::plan::PlanSource::Inline(text)) => {
            let Some((path, mut f)) = mktemp(&cfg.view_dir, ".hook.", ".md") else {
                return;
            };
            // `printf '%s\n' "$(jq -r .tool_input.plan)"`: 末尾改行を 1 つに揃える。
            let _ = f.write_all(text.trim_end_matches('\n').as_bytes());
            let _ = f.write_all(b"\n");
            drop(f);
            plan_tmp = Some(path.clone());
            path
        }
        Some(hook_io::plan::PlanSource::File(p)) => p,
        None => return,
    };

    let sid8: String = session.chars().take(8).collect();
    let out = cfg.view_dir.join(format!(
        "{}-{sid8}.html",
        hook_io::proc::date("%Y%m%d-%H%M%S")
    ));
    let _ = cfg.view_plan(
        &plan_file,
        "",
        &basename(&cwd.to_string_lossy()),
        &out,
        true,
    );
    if let Some(t) = plan_tmp {
        let _ = fs::remove_file(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h1_extraction() {
        assert_eq!(
            extract_h1("# 本当のタイトル\n\n本文\n".as_bytes()),
            "本当のタイトル"
        );
        // フェンス内の `# コメント` を拾わない
        assert_eq!(
            extract_h1("```bash\n# これはコメント\necho hi\n```\n\n# 本当のタイトル\n".as_bytes()),
            "本当のタイトル"
        );
        assert_eq!(
            extract_h1("## Context\n\n見出しは h2 から\n".as_bytes()),
            ""
        );
        // shebang を h1 と誤認しない
        assert_eq!(
            extract_h1("#!/usr/bin/env bash\n\n# 本当のタイトル\n".as_bytes()),
            "本当のタイトル"
        );
        assert_eq!(extract_h1(b"#   spaced  \t\r\n"), "spaced");
        assert_eq!(extract_h1(b"~~~\n# in\n~~~\n# out"), "out");
    }

    const META: &str = "<p class=\"plan-view-meta\">repo · now</p>";

    fn lines(b: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(b)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn meta_after_h1() {
        let out = lines(&insert_meta("# 本当のタイトル\n\n本文\n".as_bytes(), META));
        // h1(1) / 空行(2) / メタ行(3)
        assert_eq!(out[0], "# 本当のタイトル");
        assert_eq!(out[1], "");
        assert_eq!(out[2], META);
        assert_eq!(out.iter().filter(|l| *l == "# 本当のタイトル").count(), 1);
    }

    #[test]
    fn meta_without_h1_goes_first() {
        let out = lines(&insert_meta(
            "## Context\n\n見出しは h2 から\n".as_bytes(),
            META,
        ));
        assert_eq!(out[0], META);
        assert_eq!(out.iter().filter(|l| *l == "## Context").count(), 1);
    }

    #[test]
    fn meta_inserted_once_across_fences() {
        let src = "```bash\n# これはコメント\necho hi\n```\n\n# 本当のタイトル\n# 二つ目\n";
        let out = lines(&insert_meta(src.as_bytes(), META));
        assert_eq!(
            out.iter().filter(|l| l.contains("plan-view-meta")).count(),
            1
        );
        assert_eq!(out[6], "");
        assert_eq!(out[7], META);
    }

    #[test]
    fn meta_adds_trailing_newline_like_awk() {
        assert_eq!(insert_meta(b"# t", "M"), b"# t\n\nM\n".to_vec());
        // h1 なしは cat と同じく本文をそのまま
        assert_eq!(insert_meta(b"x", "M"), b"M\n\nx".to_vec());
    }

    #[test]
    fn escape_and_basename() {
        assert_eq!(html_escape("a&<b>"), "a&amp;&lt;b&gt;");
        assert_eq!(basename("/x/y/"), "y");
        assert_eq!(basename("/"), "/");
        assert_eq!(basename("plan.md"), "plan.md");
    }

    fn cfg_in(dir: &Path) -> Config {
        Config {
            view_dir: dir.to_path_buf(),
            pandoc: "pandoc".into(),
            browser: "google-chrome".into(),
            highlight: "breezeDark".into(),
            window_size: "1000,900".into(),
            retention_days: "30".into(),
            css_override: String::new(),
            darwin_chrome_app: String::new(),
            uname_override: String::new(),
            script_dir: dir.to_path_buf(),
            home: dir.to_path_buf(),
        }
    }

    fn backdate(p: &Path, days: u64) {
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(days * 86400))
            .unwrap();
    }

    #[test]
    fn prune_keeps_skip_and_fresh() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("20250101-000000-deadbeef.html");
        let skip = d.path().join("skip");
        let fresh = d.path().join("fresh.html");
        let edge = d.path().join("edge.html");
        for p in [&old, &skip, &fresh, &edge] {
            fs::write(p, "x").unwrap();
        }
        backdate(&old, 40);
        backdate(&skip, 40);
        backdate(&edge, 30); // 経過 30 日(切り捨て)は +30 に当たらない
        cfg_in(d.path()).prune_old();
        assert!(!old.exists());
        assert!(skip.exists());
        assert!(fresh.exists());
        assert!(edge.exists());

        let mut c = cfg_in(d.path());
        c.retention_days = "0".into();
        backdate(&fresh, 400);
        c.prune_old();
        assert!(fresh.exists());
    }

    #[test]
    fn ensure_dirs_tightens() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("views");
        let c = cfg_in(&v);
        c.ensure_dirs();
        let loose = v.join("loose.html");
        fs::write(&loose, "x").unwrap();
        fs::set_permissions(&loose, fs::Permissions::from_mode(0o644)).unwrap();
        let skip = v.join("skip");
        fs::write(&skip, "").unwrap();
        fs::set_permissions(&skip, fs::Permissions::from_mode(0o644)).unwrap();
        c.ensure_dirs();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&v), 0o700);
        assert_eq!(mode(&loose), 0o600);
        assert_eq!(mode(&skip), 0o644);
    }

    #[test]
    fn css_candidates_in_order() {
        let d = tempfile::tempdir().unwrap();
        let mut c = cfg_in(d.path());
        c.script_dir = d.path().join("hooks");
        c.home = d.path().join("home");
        assert_eq!(c.find_css(), None);
        fs::create_dir_all(d.path().join("home/.claude/hooks")).unwrap();
        fs::write(d.path().join("home/.claude/hooks/plan-view.css"), "h").unwrap();
        assert_eq!(
            c.find_css(),
            Some(c.home.join(".claude/hooks/plan-view.css"))
        );
        // `hooks/../assets` はカーネルが hooks の実在を要求する(bash 版も同じ)
        fs::create_dir_all(&c.script_dir).unwrap();
        fs::create_dir_all(d.path().join("assets")).unwrap();
        fs::write(d.path().join("assets/plan-view.css"), "a").unwrap();
        assert_eq!(
            c.find_css(),
            Some(c.script_dir.join("../assets/plan-view.css"))
        );
        fs::write(c.script_dir.join("plan-view.css"), "s").unwrap();
        assert_eq!(c.find_css(), Some(c.script_dir.join("plan-view.css")));
        c.css_override = d.path().join("assets/plan-view.css").display().to_string();
        assert_eq!(c.find_css(), Some(d.path().join("assets/plan-view.css")));
        c.css_override = "/nonexistent.css".into();
        assert_eq!(c.find_css(), Some(c.script_dir.join("plan-view.css")));
    }

    #[test]
    fn script_dir_does_not_resolve_links() {
        assert_eq!(
            script_dir(Some("/home/u/.claude/hooks/plan-view".into())),
            PathBuf::from("/home/u/.claude/hooks")
        );
    }
}
