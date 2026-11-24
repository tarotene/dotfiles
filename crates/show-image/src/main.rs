//! `show-image <画像のパス>` — 本人にその場で見せたい図・画像を、herdr の隣のペインに描かせる入口。
//!
//! エージェントの Bash には制御端末が無く、出力はハーネスが捕まえるので、画像は端末に届かない。
//! 画像は別のペインの出力として描く(herdr は Kitty graphics protocol だけを解釈する)。そのために
//! `herdr pane split` と `herdr pane run` が要るが、`pane run` はペインのシェルに任意のコマンドを
//! 打てるので、sandbox の外で動かす口としては広すぎる。この CLI は、受け取るのを画像のパス 1 つに
//! 絞り、ペインに打つコマンド列を固定する(`clear; chafa -f kitty '<path>'; printf …`)。
//! sandbox の除外と許可は、この CLI の形(`show-image *`)にだけ与える。
//!
//! 終了コード: 0 = 描いた / 2 = 使い方かパスの誤り / 3 = このホスト・この場所では出せない。
//! 3 のとき、呼び出し側は従来の経路(Web ページなど)に落ちる。
//!
//! 外側の端末が画像に対応しているかは、herdr のペインの中からは検出できない(API に項目が無く、同じ
//! サーバーに別の端末がつながりうる)。そこで判定は、ホストの宣言(`~/.config/dotfiles/terminal-graphics`
//! に `kitty`。home-manager が書く)で行い、描いたあとに画像のパスを文字でも出す(外側が対応して
//! いないと、画像の部分は何も出ずに空白になる。段0の実測)。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// そのまま `chafa` に渡す画像の拡張子。小文字で比べる。
const RASTER_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const SVG_EXTENSION: &str = "svg";
/// home-manager が書く、画像対応の端末を宣言するマーカー(`$XDG_CONFIG_HOME` の下)。
const MARKER: &str = "dotfiles/terminal-graphics";
const MARKER_VALUE: &str = "kitty";

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    /// 使い方かパスの誤り(終了コード 2)。
    Usage(String),
    /// このホスト・この場所では出せない(終了コード 3)。
    Unavailable(String),
}

impl Refusal {
    fn code(&self) -> u8 {
        match self {
            Refusal::Usage(_) => 2,
            Refusal::Unavailable(_) => 3,
        }
    }

    fn message(&self) -> &str {
        match self {
            Refusal::Usage(m) | Refusal::Unavailable(m) => m,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Kind {
    Raster,
    Svg,
}

/// 実行環境から読む値をひとまとめにしたもの(試験では組み立てて渡す)。
struct Context {
    herdr_pane: Option<String>,
    herdr_env: Option<String>,
    herdr_bin: String,
    socket: String,
    config_home: PathBuf,
    state_dir: PathBuf,
    cache_dir: PathBuf,
}

impl Context {
    fn from_env() -> Context {
        Context {
            herdr_pane: process_env::var_nonempty("HERDR_PANE_ID"),
            herdr_env: process_env::var_nonempty("HERDR_ENV"),
            herdr_bin: process_env::var_nonempty("HERDR_BIN_PATH")
                .unwrap_or_else(|| "herdr".into()),
            socket: process_env::var_nonempty("HERDR_SOCKET_PATH").unwrap_or_default(),
            config_home: process_env::Xdg::Config.resolve_lossy(),
            state_dir: process_env::Xdg::State.resolve_lossy().join("show-image"),
            cache_dir: process_env::Xdg::Cache.resolve_lossy().join("show-image"),
        }
    }
}

/// herdr の操作(試験では偽物に替える)。
trait Herdr {
    fn pane_exists(&self, pane: &str) -> bool;
    fn split_right(&self, from: &str) -> Result<String, String>;
    fn run(&self, pane: &str, command: &str) -> Result<(), String>;
}

/// SVG を PNG にする(試験では偽物に替える)。
trait Rasterizer {
    fn svg_to_png(&self, svg: &Path, png: &Path) -> Result<(), String>;
}

struct HerdrCli {
    bin: String,
}

impl Herdr for HerdrCli {
    fn pane_exists(&self, pane: &str) -> bool {
        Command::new(&self.bin)
            .args(["pane", "get", pane])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn split_right(&self, from: &str) -> Result<String, String> {
        let out = Command::new(&self.bin)
            .args(["pane", "split", from, "--direction", "right", "--no-focus"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("herdr を起動できません: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "herdr pane split が失敗しました: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        parse_split_output(&String::from_utf8_lossy(&out.stdout))
    }

    fn run(&self, pane: &str, command: &str) -> Result<(), String> {
        // herdr は引数を空白で連結して、クォートせずにペインのシェルへ打つ(実測)。
        // クォートは `build_pane_command` が済ませているので、1 つの引数として渡す。
        let out = Command::new(&self.bin)
            .args(["pane", "run", pane, command])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("herdr を起動できません: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "herdr pane run が失敗しました: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

struct RsvgConvert;

impl Rasterizer for RsvgConvert {
    fn svg_to_png(&self, svg: &Path, png: &Path) -> Result<(), String> {
        let out = Command::new("rsvg-convert")
            .arg(svg)
            .arg("-o")
            .arg(png)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("rsvg-convert を起動できません: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "SVG を PNG にできません: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

/// `herdr pane split` の標準出力(JSON 1 件)から、新しいペインの id を取る。
fn parse_split_output(stdout: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("herdr の応答を読めません: {e}"))?;
    value
        .pointer("/result/pane/pane_id")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "herdr の応答に pane_id がありません".to_owned())
}

/// 引数 1 つを、描ける画像として読む。パスは正規化した絶対パスにする。
///
/// ペインのシェルには、パスが文字としてそのまま打たれる。シェルごとにクォートの意味が違う
/// `'` と `\` と、行を割る制御文字を含むパスは、読み替えずに拒否する。
fn resolve_image(arg: &str) -> Result<(PathBuf, Kind), Refusal> {
    let path = fs::canonicalize(arg)
        .map_err(|e| Refusal::Usage(format!("画像を開けません: {arg} ({e})")))?;
    if !path.is_file() {
        return Err(Refusal::Usage(format!(
            "ファイルではありません: {}",
            path.display()
        )));
    }
    let text = path
        .to_str()
        .ok_or_else(|| Refusal::Usage("パスが UTF-8 ではありません".to_owned()))?;
    if text
        .chars()
        .any(|c| c == '\'' || c == '\\' || c.is_control())
    {
        return Err(Refusal::Usage(
            "パスに `'`・`\\`・制御文字が含まれています(ペインのシェルに打てません)。別の名前で置き直してください"
                .to_owned(),
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let kind = if ext == SVG_EXTENSION {
        Kind::Svg
    } else if RASTER_EXTENSIONS.contains(&ext.as_str()) {
        Kind::Raster
    } else {
        return Err(Refusal::Usage(format!(
            "対応していない拡張子です: .{ext}(png jpg jpeg gif webp svg)"
        )));
    };
    Ok((path, kind))
}

/// ペインのシェルへ打つ 1 行。パス以外は固定。
fn build_pane_command(image: &Path) -> String {
    let q = format!("'{}'", image.display());
    format!("clear; chafa -f kitty {q}; printf '%s\\n' {q}")
}

fn graphics_declared(config_home: &Path) -> bool {
    fs::read_to_string(config_home.join(MARKER))
        .map(|s| s.trim() == MARKER_VALUE)
        .unwrap_or(false)
}

/// SVG を、キャッシュの下の PNG にする。同じ名前なら上書きする(溜めない)。
fn rasterize(svg: &Path, ctx: &Context, rasterizer: &dyn Rasterizer) -> Result<PathBuf, Refusal> {
    fs::create_dir_all(&ctx.cache_dir)
        .map_err(|e| Refusal::Unavailable(format!("キャッシュを作れません: {e}")))?;
    let stem: String = svg
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let png = ctx.cache_dir.join(format!("{stem}.png"));
    rasterizer
        .svg_to_png(svg, &png)
        .map_err(Refusal::Unavailable)?;
    Ok(png)
}

/// herdr のサーバーごとに 1 つの閲覧ペインを使い回す。残っていなければ作り直す。
fn viewer_pane(ctx: &Context, from: &str, herdr: &dyn Herdr) -> Result<String, Refusal> {
    let key: String = ctx
        .socket
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let state_file = ctx.state_dir.join(format!("viewer-{key}"));
    if let Ok(saved) = fs::read_to_string(&state_file) {
        let saved = saved.trim();
        if !saved.is_empty() && herdr.pane_exists(saved) {
            return Ok(saved.to_owned());
        }
    }
    let pane = herdr.split_right(from).map_err(Refusal::Unavailable)?;
    // 覚えそこねても描画は成功しているので、状態の書き込みの失敗は握りつぶさず、次回の作り直しに任せる。
    if fs::create_dir_all(&ctx.state_dir).is_ok() {
        let _ = fs::write(&state_file, format!("{pane}\n"));
    }
    Ok(pane)
}

fn run(
    args: &[String],
    ctx: &Context,
    herdr: &dyn Herdr,
    rasterizer: &dyn Rasterizer,
) -> Result<String, Refusal> {
    let [arg] = args else {
        return Err(Refusal::Usage("使い方: show-image <画像のパス>".to_owned()));
    };
    let (path, kind) = resolve_image(arg)?;
    if ctx.herdr_env.as_deref() != Some("1") {
        return Err(Refusal::Unavailable(
            "herdr のペインの中ではないので、隣のペインを作れません".to_owned(),
        ));
    }
    let Some(from) = ctx.herdr_pane.as_deref() else {
        return Err(Refusal::Unavailable(
            "HERDR_PANE_ID がありません".to_owned(),
        ));
    };
    if !graphics_declared(&ctx.config_home) {
        return Err(Refusal::Unavailable(format!(
            "このホストは画像対応の端末を宣言していません(~/.config/{MARKER} が `{MARKER_VALUE}` ではありません)"
        )));
    }
    let image = match kind {
        Kind::Raster => path,
        Kind::Svg => rasterize(&path, ctx, rasterizer)?,
    };
    let pane = viewer_pane(ctx, from, herdr)?;
    herdr
        .run(&pane, &build_pane_command(&image))
        .map_err(Refusal::Unavailable)?;
    Ok(format!(
        "隣のペイン({pane})に描きました: {}",
        image.display()
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ctx = Context::from_env();
    let herdr = HerdrCli {
        bin: ctx.herdr_bin.clone(),
    };
    match run(&args, &ctx, &herdr, &RsvgConvert) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            eprintln!("show-image: {}", refusal.message());
            ExitCode::from(refusal.code())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeHerdr {
        existing: Vec<String>,
        next_id: RefCell<u32>,
        splits: RefCell<Vec<String>>,
        runs: RefCell<Vec<(String, String)>>,
    }

    impl Herdr for FakeHerdr {
        fn pane_exists(&self, pane: &str) -> bool {
            self.existing.iter().any(|p| p == pane)
        }
        fn split_right(&self, from: &str) -> Result<String, String> {
            self.splits.borrow_mut().push(from.to_owned());
            let mut n = self.next_id.borrow_mut();
            *n += 1;
            Ok(format!("w1:p{}", *n + 1))
        }
        fn run(&self, pane: &str, command: &str) -> Result<(), String> {
            self.runs
                .borrow_mut()
                .push((pane.to_owned(), command.to_owned()));
            Ok(())
        }
    }

    struct FakeRasterizer;

    impl Rasterizer for FakeRasterizer {
        fn svg_to_png(&self, _svg: &Path, png: &Path) -> Result<(), String> {
            fs::write(png, b"png").map_err(|e| e.to_string())
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        ctx: Context,
    }

    fn fixture(declared: bool, in_herdr: bool) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let config_home = dir.path().join("config");
        if declared {
            fs::create_dir_all(config_home.join("dotfiles")).unwrap();
            fs::write(config_home.join(MARKER), "kitty\n").unwrap();
        }
        let ctx = Context {
            herdr_pane: in_herdr.then(|| "w1:p1".to_owned()),
            herdr_env: in_herdr.then(|| "1".to_owned()),
            herdr_bin: "herdr".into(),
            socket: "/run/herdr/herdr.sock".into(),
            config_home,
            state_dir: dir.path().join("state"),
            cache_dir: dir.path().join("cache"),
        };
        Fixture { dir, ctx }
    }

    fn image(f: &Fixture, name: &str) -> String {
        let p = f.dir.path().join(name);
        fs::write(&p, b"x").unwrap();
        p.to_str().unwrap().to_owned()
    }

    fn args(a: &str) -> Vec<String> {
        vec![a.to_owned()]
    }

    #[test]
    fn parses_pane_id_from_split_output() {
        let json =
            r#"{"id":"cli:pane:split","result":{"pane":{"pane_id":"w1:p2"},"type":"pane_info"}}"#;
        assert_eq!(parse_split_output(json).unwrap(), "w1:p2");
        assert!(parse_split_output("{}").is_err());
        assert!(parse_split_output("not json").is_err());
    }

    #[test]
    fn rejects_wrong_argument_count() {
        let f = fixture(true, true);
        let herdr = FakeHerdr::default();
        for a in [vec![], vec!["a".to_owned(), "b".to_owned()]] {
            let e = run(&a, &f.ctx, &herdr, &FakeRasterizer).unwrap_err();
            assert_eq!(e.code(), 2);
        }
        assert!(herdr.runs.borrow().is_empty());
    }

    #[test]
    fn rejects_missing_file_and_unknown_extension() {
        let f = fixture(true, true);
        assert_eq!(resolve_image("/no/such/file.png").unwrap_err().code(), 2);
        let txt = image(&f, "note.txt");
        assert_eq!(resolve_image(&txt).unwrap_err().code(), 2);
        let dir = f.dir.path().join("dir.png");
        fs::create_dir(&dir).unwrap();
        assert_eq!(resolve_image(dir.to_str().unwrap()).unwrap_err().code(), 2);
    }

    #[test]
    fn accepts_known_extensions_case_insensitively() {
        let f = fixture(true, true);
        for (name, kind) in [
            ("a.png", Kind::Raster),
            ("b.JPG", Kind::Raster),
            ("c.webp", Kind::Raster),
            ("d.SVG", Kind::Svg),
        ] {
            let (_, k) = resolve_image(&image(&f, name)).unwrap();
            assert_eq!(k, kind, "{name}");
        }
    }

    #[test]
    fn rejects_paths_the_pane_shell_could_misread() {
        let f = fixture(true, true);
        for name in ["it's.png", "a\\b.png", "a\nb.png"] {
            let p = f.dir.path().join(name);
            fs::write(&p, b"x").unwrap();
            let e = resolve_image(p.to_str().unwrap()).unwrap_err();
            assert_eq!(e.code(), 2, "{name:?}");
        }
    }

    #[test]
    fn pane_command_is_fixed_except_for_the_quoted_path() {
        let c = build_pane_command(Path::new("/tmp/a b.png"));
        assert_eq!(
            c,
            "clear; chafa -f kitty '/tmp/a b.png'; printf '%s\\n' '/tmp/a b.png'"
        );
    }

    #[test]
    fn refuses_outside_herdr() {
        let f = fixture(true, false);
        let herdr = FakeHerdr::default();
        let e = run(&args(&image(&f, "a.png")), &f.ctx, &herdr, &FakeRasterizer).unwrap_err();
        assert_eq!(e.code(), 3);
        assert!(herdr.splits.borrow().is_empty());
    }

    #[test]
    fn refuses_when_the_host_does_not_declare_graphics() {
        let f = fixture(false, true);
        let herdr = FakeHerdr::default();
        let e = run(&args(&image(&f, "a.png")), &f.ctx, &herdr, &FakeRasterizer).unwrap_err();
        assert_eq!(e.code(), 3);
        assert!(herdr.splits.borrow().is_empty());
        assert!(herdr.runs.borrow().is_empty());
    }

    #[test]
    fn a_marker_with_another_value_is_not_a_declaration() {
        let f = fixture(true, true);
        fs::write(f.ctx.config_home.join(MARKER), "none\n").unwrap();
        assert!(!graphics_declared(&f.ctx.config_home));
    }

    #[test]
    fn draws_in_a_new_viewer_pane_then_reuses_it() {
        let f = fixture(true, true);
        let img = image(&f, "a.png");
        let herdr = FakeHerdr::default();
        run(&args(&img), &f.ctx, &herdr, &FakeRasterizer).unwrap();
        assert_eq!(herdr.splits.borrow().as_slice(), ["w1:p1"]);
        let first = herdr.runs.borrow()[0].0.clone();

        // 2 回目: 保存したペインが残っているので、分割しない。
        let herdr2 = FakeHerdr {
            existing: vec![first.clone()],
            ..FakeHerdr::default()
        };
        run(&args(&img), &f.ctx, &herdr2, &FakeRasterizer).unwrap();
        assert!(herdr2.splits.borrow().is_empty());
        assert_eq!(herdr2.runs.borrow()[0].0, first);

        // 3 回目: ペインが閉じられていたら、作り直す。
        let herdr3 = FakeHerdr::default();
        run(&args(&img), &f.ctx, &herdr3, &FakeRasterizer).unwrap();
        assert_eq!(herdr3.splits.borrow().len(), 1);
    }

    #[test]
    fn svg_is_rasterized_into_the_cache_and_that_png_is_drawn() {
        let f = fixture(true, true);
        let svg = image(&f, "my diagram.svg");
        let herdr = FakeHerdr::default();
        run(&args(&svg), &f.ctx, &herdr, &FakeRasterizer).unwrap();
        let (_, command) = herdr.runs.borrow()[0].clone();
        let expected = f.ctx.cache_dir.join("my_diagram.png");
        assert!(expected.is_file());
        assert!(command.contains(expected.to_str().unwrap()), "{command}");
        assert!(!command.contains(".svg"));
    }
}
