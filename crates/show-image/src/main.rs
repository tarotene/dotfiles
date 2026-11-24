//! `show-image <画像のパス>` — 本人にその場で見せたい図・画像を、手元の別窓(Chrome の専用窓)に出す入口。
//!
//! 図・画像を見せるだけのために、Artifact などの Web ページを公開しない。エージェントの Bash には制御端末が
//! 無く、出力はハーネスがファイルに捕まえるので、画像は端末に届かない。画像は別の窓に出す(ADR-1694)。
//! 窓の出し方は `plan-view`(ExitPlanMode のプランの窓)と同じ `app-window` が持つ。
//!
//! 外側の端末(Alacritty など)に依らない。画像対応の端末をホストごとに宣言する仕組みは持たない。
//! GUI の Chrome は sandbox の中では窓を出せない(Unix ソケットを作れない。2026-11-24 に実測)ので、sandbox の
//! 除外と許可は、この CLI の形(`show-image *`)だけに与える。受け取るのは画像のパス 1 つで、起動するのは固定の
//! ブラウザ(環境変数では替えられない)、開くのは画像の拡張子を持つ実在のファイルだけである。
//!
//! 終了コード: 0 = 窓に出した / 2 = 使い方かパスの誤り / 3 = このホスト・この場所では出せない
//! (画面が無い、ブラウザが無い、SVG を PNG にできない)。3 のとき、呼び出し側は従来の経路に落ちる。
//!
//! SVG は、そのまま窓に渡さず `rsvg-convert` で PNG にしてから開く。SVG を文書として直接開くと、
//! スクリプトが走りうる(MDN "SVG as an image": 画像として使うときの制限は、直接開く場合には当たらない)。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// そのまま窓に渡す画像の拡張子。小文字で比べる。
const RASTER_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const SVG_EXTENSION: &str = "svg";
/// 窓の大きさ(`--window-size`)。固定。
const WINDOW_SIZE: &str = "1100,800";

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

/// 実行環境から読む値(試験では組み立てて渡す)。
struct Context {
    cache_dir: PathBuf,
}

impl Context {
    fn from_env() -> Context {
        Context {
            cache_dir: process_env::Xdg::Cache.resolve_lossy().join("show-image"),
        }
    }
}

/// 窓を出す(試験では偽物に替える)。
trait Windows {
    /// 窓を出せるか。出せないときは、その理由。
    fn check(&self) -> Result<(), String>;
    fn open(&self, file: &Path);
}

/// SVG を PNG にする(試験では偽物に替える)。
trait Rasterizer {
    fn svg_to_png(&self, svg: &Path, png: &Path) -> Result<(), String>;
}

struct ChromeWindows {
    window: app_window::AppWindow,
}

impl ChromeWindows {
    /// ブラウザ・窓の大きさ・darwin の .app は固定で、環境変数では替えられない(除外コマンドの口を広げない)。
    fn new() -> ChromeWindows {
        ChromeWindows {
            window: app_window::AppWindow {
                browser: "google-chrome".to_owned(),
                window_size: WINDOW_SIZE.to_owned(),
                darwin_chrome_app: "/Applications/Google Chrome.app".to_owned(),
                uname_override: String::new(),
            },
        }
    }
}

impl Windows for ChromeWindows {
    fn check(&self) -> Result<(), String> {
        if !self.window.browser_available() {
            return Err(if self.window.is_darwin() {
                format!(
                    "Google Chrome.app が見つかりません ({})",
                    self.window.darwin_chrome_app
                )
            } else {
                format!("ブラウザが見つかりません ({})", self.window.browser)
            });
        }
        if !self.window.has_display() {
            return Err(
                "DISPLAY / WAYLAND_DISPLAY がありません(画面が無いセッションです)".to_owned(),
            );
        }
        Ok(())
    }

    fn open(&self, file: &Path) {
        self.window.open_file(file);
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

/// 引数 1 つを、開ける画像として読む。パスは正規化した絶対パスにする。
fn resolve_image(arg: &str) -> Result<(PathBuf, Kind), Refusal> {
    let path = fs::canonicalize(arg)
        .map_err(|e| Refusal::Usage(format!("画像を開けません: {arg} ({e})")))?;
    if !path.is_file() {
        return Err(Refusal::Usage(format!(
            "ファイルではありません: {}",
            path.display()
        )));
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

fn run(
    args: &[String],
    ctx: &Context,
    windows: &dyn Windows,
    rasterizer: &dyn Rasterizer,
) -> Result<String, Refusal> {
    let [arg] = args else {
        return Err(Refusal::Usage("使い方: show-image <画像のパス>".to_owned()));
    };
    let (path, kind) = resolve_image(arg)?;
    windows.check().map_err(Refusal::Unavailable)?;
    let image = match kind {
        Kind::Raster => path,
        Kind::Svg => rasterize(&path, ctx, rasterizer)?,
    };
    windows.open(&image);
    Ok(format!("別窓に出しました: {}", image.display()))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ctx = Context::from_env();
    match run(&args, &ctx, &ChromeWindows::new(), &RsvgConvert) {
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

    struct FakeWindows {
        refuse: Option<String>,
        opened: RefCell<Vec<PathBuf>>,
    }

    impl FakeWindows {
        fn ok() -> FakeWindows {
            FakeWindows {
                refuse: None,
                opened: RefCell::new(Vec::new()),
            }
        }
    }

    impl Windows for FakeWindows {
        fn check(&self) -> Result<(), String> {
            match &self.refuse {
                Some(m) => Err(m.clone()),
                None => Ok(()),
            }
        }
        fn open(&self, file: &Path) {
            self.opened.borrow_mut().push(file.to_path_buf());
        }
    }

    struct FakeRasterizer;

    impl Rasterizer for FakeRasterizer {
        fn svg_to_png(&self, _svg: &Path, png: &Path) -> Result<(), String> {
            fs::write(png, b"png").map_err(|e| e.to_string())
        }
    }

    struct FailingRasterizer;

    impl Rasterizer for FailingRasterizer {
        fn svg_to_png(&self, _svg: &Path, _png: &Path) -> Result<(), String> {
            Err("SVG を PNG にできません: broken".to_owned())
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        ctx: Context,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context {
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
    fn rejects_wrong_argument_count() {
        let f = fixture();
        let windows = FakeWindows::ok();
        for a in [vec![], vec!["a".to_owned(), "b".to_owned()]] {
            let e = run(&a, &f.ctx, &windows, &FakeRasterizer).unwrap_err();
            assert_eq!(e.code(), 2);
        }
        assert!(windows.opened.borrow().is_empty());
    }

    #[test]
    fn rejects_missing_file_directory_and_unknown_extension() {
        let f = fixture();
        assert_eq!(resolve_image("/no/such/file.png").unwrap_err().code(), 2);
        let txt = image(&f, "note.txt");
        assert_eq!(resolve_image(&txt).unwrap_err().code(), 2);
        let dir = f.dir.path().join("dir.png");
        fs::create_dir(&dir).unwrap();
        assert_eq!(resolve_image(dir.to_str().unwrap()).unwrap_err().code(), 2);
    }

    #[test]
    fn accepts_known_extensions_case_insensitively() {
        let f = fixture();
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
    fn a_path_with_odd_characters_is_accepted_because_the_url_is_escaped() {
        // 以前はペインのシェルに打つので `'`・`\`・制御文字を拒否した。窓は URL で開くので、拒否しない。
        let f = fixture();
        for name in ["it's.png", "a b#c.png", "図.png"] {
            assert!(resolve_image(&image(&f, name)).is_ok(), "{name}");
        }
    }

    #[test]
    fn opens_a_raster_image_as_it_is() {
        let f = fixture();
        let img = image(&f, "a.png");
        let windows = FakeWindows::ok();
        run(&args(&img), &f.ctx, &windows, &FakeRasterizer).unwrap();
        let opened = windows.opened.borrow();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0], fs::canonicalize(&img).unwrap());
    }

    #[test]
    fn refuses_with_3_when_there_is_no_window_to_open_and_opens_nothing() {
        let f = fixture();
        let windows = FakeWindows {
            refuse: Some("DISPLAY / WAYLAND_DISPLAY がありません".to_owned()),
            opened: RefCell::new(Vec::new()),
        };
        let e = run(
            &args(&image(&f, "a.png")),
            &f.ctx,
            &windows,
            &FakeRasterizer,
        )
        .unwrap_err();
        assert_eq!(e.code(), 3);
        assert!(windows.opened.borrow().is_empty());
    }

    #[test]
    fn a_usage_error_wins_over_an_unavailable_window() {
        // 画面が無いホストでも、パスの誤りは 2 で返す(呼び出し側が従来の経路に落ちる前に、直せる誤りを直す)。
        let f = fixture();
        let windows = FakeWindows {
            refuse: Some("画面が無い".to_owned()),
            opened: RefCell::new(Vec::new()),
        };
        let e = run(
            &args("/no/such/file.png"),
            &f.ctx,
            &windows,
            &FakeRasterizer,
        )
        .unwrap_err();
        assert_eq!(e.code(), 2);
    }

    #[test]
    fn svg_is_rasterized_into_the_cache_and_that_png_is_opened() {
        let f = fixture();
        let svg = image(&f, "my diagram.svg");
        let windows = FakeWindows::ok();
        run(&args(&svg), &f.ctx, &windows, &FakeRasterizer).unwrap();
        let expected = f.ctx.cache_dir.join("my_diagram.png");
        assert!(expected.is_file());
        assert_eq!(windows.opened.borrow().as_slice(), [expected]);
    }

    #[test]
    fn svg_is_never_opened_as_a_document() {
        let f = fixture();
        let svg = image(&f, "x.svg");
        let windows = FakeWindows::ok();
        run(&args(&svg), &f.ctx, &windows, &FakeRasterizer).unwrap();
        assert!(windows
            .opened
            .borrow()
            .iter()
            .all(|p| p.extension().is_some_and(|e| e == "png")));
    }

    #[test]
    fn a_failed_svg_conversion_opens_nothing_and_is_3() {
        let f = fixture();
        let svg = image(&f, "x.svg");
        let windows = FakeWindows::ok();
        let e = run(&args(&svg), &f.ctx, &windows, &FailingRasterizer).unwrap_err();
        assert_eq!(e.code(), 3);
        assert!(windows.opened.borrow().is_empty());
    }
}
