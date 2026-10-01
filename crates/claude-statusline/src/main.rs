//! claude-statusline — ペイン内の 1 行表示 + Herdr へのメトリクス横流し(#413、
//! 移植元 `config/claude/statusline/claude-statusline.sh`)。
//!
//! Claude Code の statusLine command。stdin の JSON からリポ名・モデル・
//! context 使用率・セッションコスト・effort を取り、Catppuccin Mocha の 1 行を
//! stdout に出す(表示例: `■ dotfiles · ◆ Fable 5 · ◐ 42% · $1.23 · ↯ low`)。
//! 設計は docs/claude/herdr-sidebar-metadata.md。
//!
//! Herdr への報告(`pane.report_metadata`)は表示をブロックしない: 表示を
//! stdout に書いて flush したあと、送るべきとき(値が変わり、前回送信から 2 秒以上)
//! だけ自分自身を `__herdr-report` で stdio を閉じた子プロセスとして起動し、
//! 待たずに終了する(bash 版の `( ... ) >/dev/null 2>&1 &` と同じ形)。

use claude_statusline::{
    parse_payload, posix_basename, posix_dirname, printf_2f, shell_int, Tokens,
};
use hook_io::herdr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const PEACH: &str = "\x1b[1;38;2;250;179;135m"; // repo       (Catppuccin Mocha peach)
const PINK: &str = "\x1b[1;38;2;245;194;231m"; // model      (Catppuccin Mocha pink)
const GREEN: &str = "\x1b[38;2;166;227;161m"; // ctx < 60%  (green)
const YELLOW: &str = "\x1b[38;2;249;226;175m"; // ctx 60-79% / fast (yellow)
const RED: &str = "\x1b[38;2;243;139;168m"; // ctx >= 80% (red)
const TEAL: &str = "\x1b[38;2;148;226;213m"; // cost       (teal)
const LAVENDER: &str = "\x1b[38;2;180;190;254m"; // effort     (lavender)
const OVERLAY: &str = "\x1b[38;2;108;112;134m"; // separators (overlay0)
const RESET: &str = "\x1b[0m";

/// `$(...)` と同じく末尾の改行を落とし、NUL を除く。
fn subst(s: &str) -> String {
    s.trim_end_matches('\n').replace('\0', "")
}

/// リポ名(project_dir 毎にキャッシュ)。`--show-toplevel` は herdr worktree だと
/// worktree-xxx を返すので、`--git-common-dir` の親ディレクトリ名を使う。
fn repo_name(project_dir: &str) -> String {
    if project_dir.is_empty() {
        return String::new();
    }
    let cache = herdr::pane_state_file("claude-statusline-repo", project_dir);
    if cache.is_file() {
        return std::fs::read(&cache)
            .map(|b| subst(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
    }
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let common = match out {
        Ok(o) if o.status.success() => subst(&String::from_utf8_lossy(&o.stdout)),
        _ => String::new(),
    };
    if common.is_empty() {
        return String::new();
    }
    let repo = posix_basename(&posix_dirname(&common));
    let _ = std::fs::write(&cache, &repo);
    repo
}

fn render(p: &claude_statusline::Payload, repo: &str, cost_fmt: &str) -> String {
    let sep = format!(" {OVERLAY}·{RESET} ");
    let mut parts: Vec<String> = Vec::new();
    if !repo.is_empty() {
        parts.push(format!("■ {PEACH}{repo}{RESET}"));
    }
    if !p.model.is_empty() {
        parts.push(format!("◆ {PINK}{}{RESET}", p.model));
    }
    if !p.ctx.is_empty() {
        let n = shell_int(&p.ctx);
        let color = match n {
            Some(v) if v >= 80 => RED,
            Some(v) if v >= 60 => YELLOW,
            _ => GREEN,
        };
        parts.push(format!("◐ {color}{}%{RESET}", p.ctx));
    }
    // 狭いペインではモデルと context だけに畳む。
    let cols = std::env::var("COLUMNS")
        .ok()
        .filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or_else(|| "80".into());
    if matches!(shell_int(&cols), Some(c) if c >= 60) {
        if !p.cost.is_empty() {
            parts.push(format!("{TEAL}${cost_fmt}{RESET}"));
        }
        // effort は既定値(high)のときは出さない — 非デフォルト時のみ表示する定石。
        if !p.effort.is_empty() && p.effort != "high" {
            parts.push(format!("↯ {LAVENDER}{}{RESET}", p.effort));
        }
        if p.fast == "1" {
            parts.push(format!("{YELLOW}fast{RESET}"));
        }
    }
    parts.join(&sep)
}

/// python 版の抑止判定: 状態ファイルが前回と同じ fingerprint を持つ、または
/// 前回送信から 2 秒未満なら送らない。状態ファイルが無ければ送る。読めない
/// (権限・UTF-8 でない等)ときは python 版が例外で落ちるのと同じく送らない。
fn should_send(state: &Path, fingerprint: &str) -> bool {
    let meta = match std::fs::metadata(state) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    };
    let Ok(bytes) = std::fs::read(state) else {
        return false;
    };
    let Ok(prev) = String::from_utf8(bytes) else {
        return false;
    };
    // open(..., encoding="utf-8") の universal newlines
    let prev = prev.replace("\r\n", "\n").replace('\r', "\n");
    if prev == fingerprint {
        return false;
    }
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    match SystemTime::now().duration_since(mtime) {
        Ok(age) => age >= Duration::from_secs(2),
        Err(_) => false, // 未来の mtime: time.time() - mtime < 2.0
    }
}

/// 子プロセス側: 1 回送って、成功したら fingerprint を状態ファイルに書く。
fn report(socket: &Path, pane: &str, state: &Path, tokens: &Tokens) {
    let req = tokens.request(
        &herdr::request_id("claude-statusline"),
        pane,
        herdr::seq_ns(),
    );
    if herdr::send_line(socket, &req).is_ok() {
        let _ = std::fs::write(state, tokens.fingerprint());
    }
}

fn child_main(args: &[String]) {
    // __herdr-report <socket> <pane_id> <state> <model> <ctx> <cost> <effort>
    if args.len() != 7 {
        return;
    }
    let tokens = Tokens {
        model: args[3].clone(),
        ctx: args[4].clone(),
        cost: args[5].clone(),
        effort: args[6].clone(),
    };
    report(Path::new(&args[0]), &args[1], Path::new(&args[2]), &tokens);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("__herdr-report") {
        child_main(&args[1..]);
        return;
    }

    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    let input = String::from_utf8_lossy(&buf);
    let Ok(p) = parse_payload(&input) else {
        return;
    };

    let repo = repo_name(&p.project_dir);
    let cost_fmt = if p.cost.is_empty() {
        String::new()
    } else {
        printf_2f(&p.cost)
    };
    let line = render(&p, &repo, &cost_fmt);
    let mut out = std::io::stdout().lock();
    if out.write_all(format!("{line}\n").as_bytes()).is_err() || out.flush().is_err() {
        std::process::exit(1);
    }
    drop(out);

    // ---- ここから Herdr への報告(Herdr 外では何もしない) ----
    let Some((socket, pane)) = herdr::pane_env() else {
        return;
    };
    let state = herdr::pane_state_file("herdr-claude-status", &pane);
    let tokens = Tokens {
        model: p.model.clone(),
        ctx: if p.ctx.is_empty() {
            String::new()
        } else {
            format!("{}%", p.ctx)
        },
        cost: if p.cost.is_empty() {
            String::new()
        } else {
            format!("${cost_fmt}")
        },
        effort: p.effort.clone(),
    };
    if !should_send(&state, &tokens.fingerprint()) {
        return;
    }
    let spawned = std::env::current_exe().ok().and_then(|exe| {
        Command::new(exe)
            .arg("__herdr-report")
            .arg(&socket)
            .arg(&pane)
            .arg(&state)
            .args([&tokens.model, &tokens.ctx, &tokens.cost, &tokens.effort])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    });
    if spawned.is_none() {
        // 子プロセスを起こせないときだけ同期で送る(表示は flush 済み)。
        report(&socket, &pane, &state, &tokens);
    }
}
