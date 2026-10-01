//! claude-plan-model — Opus Plan Mode の「モデルのペア」を 1 コマンドで切り替える
//! (旧 `scripts/claude-plan-model`、ADR-0024 Stage 4e、#414)。
//!
//! `model: "opusplan"` は *エイリアスのペア* であって、モデルのペアではない:
//! Plan 側は `opus` エイリアス、実行側は `sonnet` エイリアスを解決する。各エイリアスが
//! どの具体モデルに解決されるかは、`~/.claude/settings.json` の `env` にある
//! `ANTHROPIC_DEFAULT_<FAMILY>_MODEL` で別途宣言する。だからここでのモードは常に
//! *ペア* で、ペアとして名前を付ける:
//!
//! ```text
//! fable/sonnet   plan = Fable, execute = Sonnet   (既定)
//! opus/sonnet    plan = Opus,  execute = Sonnet
//! fable/opus     plan = Fable, execute = Opus
//! ```
//!
//! opus エイリアスの上書きだけが Fable を Plan 側に置く唯一の手段であり、同時にこの
//! コマンドが存在する理由でもある — 上書きがあると `/model opus` は Opus に届かなく
//! なり、Fable 自身のレート制限が尽きても帯域内の戻り道が無い。
//!
//! `fallbackModel` はこの場合を覆わない(model_not_found / permission_denied /
//! server_error / overload でだけ発火し、"Usage limit reached" は別経路 = 窓の
//! リセット待ち)。切り替えは手動でなければならない。
//!
//! 状態の持ち主は意図的に 2 つに分かれる:
//!
//! - モード(ペア): このコマンドが持つ実行時状態。`hms` は決して上書きしない
//!   (`.model` に触れないのと同じ)。
//! - 具体モデル ID: 宣言が持つ。毎回、インストール済み claude バイナリに焼かれた
//!   カタログ(`latest_per_family`)から引き直す。何も pin しないので、新しい世代は
//!   Nix を編集せずに拾える。
//!
//! エイリアス文字列は env の値に使えない(値はそのまま渡され、API が `fable` を
//! `unrecognized_model` で拒否する)。具体 ID が必須であり、それがカタログ引きの
//! 必要な理由。
//!
//! `opus/sonnet` モードはキーの削除ではなく具体 Opus ID の *書き込み* で表す。キーの
//! 不在は「未初期化」を意味し続けなければならない — でないと `sync` が新規マシンと
//! 意図して選んだモードを区別できず、Fable の制限が尽きている間も `hms` のたびに
//! Fable へ引き戻してしまう。実行側は鏡像で、`sonnet` 実行は alias 既定なので
//! SONNET キーは `*/sonnet` の 2 モードでは *不在*、`fable/opus` のときだけ書く。
//!
//! 全体の根拠: docs/claude/opusplan-model-aliases.md
//!
//! bash 版との差分(意図的): `--selftest` は `tests/selftest.rs`(cargo test)へ移した。
//! jq 依存は無くなり、settings.json のキー順は `json` モジュールが保つ。

mod json;

use hook_io::jqfmt::J;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const PROG: &str = "claude-plan-model";
const DEFAULT_MODE: &str = "fable/sonnet";
const OPUS_KEY: &str = "ANTHROPIC_DEFAULT_OPUS_MODEL";
const SONNET_KEY: &str = "ANTHROPIC_DEFAULT_SONNET_MODEL";

fn usage() -> String {
    format!(
        "Usage: {p} [--force] [fable/sonnet|opus/sonnet|fable/opus|status|sync]

Switch the pair of models behind Opus Plan Mode (plan side / execution side).

  {p}               cycle fable/sonnet -> opus/sonnet -> fable/opus -> ...
  {p} fable/opus    select a mode explicitly (idempotent)
  {p} status        report the current mode without changing anything
  {p} sync          keep the current mode, refresh the concrete model IDs
                        (run from home-manager activation)

Modes:
  fable/sonnet   plan = Fable, execute = Sonnet   (default; also accepts \"fable\")
  opus/sonnet    plan = Opus,  execute = Sonnet   (also accepts \"opus\")
  fable/opus     plan = Fable, execute = Opus

Options:
  --force   apply even when .model is not \"opusplan\" (see below)

The whole construction only does something while .model is \"opusplan\": that is
the one setting that splits plan from execution. Any other value makes this
switch either inert (a concrete model ID bypasses the aliases) or far too broad
(a bare alias like \"opus\" routes the *whole* session through the alias this
command overrides), so a mutating run stops instead of guessing.
",
        p = PROG
    )
}

fn out(s: &str) {
    let _ = writeln!(std::io::stdout(), "{PROG}: {s}");
}
fn warn(s: &str) {
    let _ = writeln!(std::io::stderr(), "{PROG}: {s}");
}
fn err_raw(s: &str) {
    let _ = write!(std::io::stderr(), "{s}");
}

// --- カタログ ---------------------------------------------------------------

/// カタログはインストール済みバイナリに
/// `latest_per_family:{fable:"claude-fable-5-1",opus:"claude-opus-5",...}` として
/// 焼かれている。空の `latest_per_family:{}` も現れるので、波括弧の中に少なくとも
/// 1 文字を要求する(`grep -aom1 'latest_per_family:{[^}]\+}'` と同じ)。`strings` を
/// 介さずバイナリを直接読むので binutils に依存しない。
///
/// `CLAUDE_PLAN_MODEL_CATALOG` はテスト用の継ぎ目: 状態機械の検査が、たまたまどの
/// claude が入っているかに依存してはならない。
fn read_catalog() -> Option<String> {
    if let Some(c) = std::env::var_os("CLAUDE_PLAN_MODEL_CATALOG") {
        if !c.is_empty() {
            return Some(c.to_string_lossy().into_owned());
        }
    }
    let bin = find_in_path("claude")?;
    let bin = fs::canonicalize(bin).ok()?;
    if !bin.is_file() {
        return None;
    }
    let data = fs::read(&bin).ok()?;
    scan_catalog(&data)
}

/// `command -v name`(PATH 上の実行可能ファイル)。
fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| {
            p.is_file()
                && fs::metadata(p)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        })
}

fn scan_catalog(data: &[u8]) -> Option<String> {
    const PREFIX: &[u8] = b"latest_per_family:{";
    let mut from = 0;
    while let Some(off) = data[from..].iter().position(|&b| b == b'l') {
        let at = from + off;
        from = at + 1;
        if !data[at..].starts_with(PREFIX) {
            continue;
        }
        let body = &data[at + PREFIX.len()..];
        // `[^}]\+}` — grep は行単位なので改行も打ち切り。
        let end = body.iter().position(|&b| b == b'}' || b == b'\n');
        if let Some(end) = end {
            if end >= 1 && body[end] == b'}' {
                return Some(
                    String::from_utf8_lossy(&data[at..at + PREFIX.len() + end + 1]).into_owned(),
                );
            }
        }
    }
    None
}

/// `sed -n "s/.*[{,]KEY:\"\([^\"]*\)\".*/\1/p"` — 貪欲な `.*` なので最後の一致を採る。
fn catalog_field(catalog: &str, key: &str) -> String {
    let mut best: Option<(usize, String)> = None;
    for lead in ['{', ','] {
        let pat = format!("{lead}{key}:\"");
        let mut start = 0;
        while let Some(i) = catalog[start..].find(&pat) {
            let at = start + i;
            let v = &catalog[at + pat.len()..];
            if let Some(q) = v.find('"') {
                if best.as_ref().is_none_or(|(b, _)| at > *b) {
                    best = Some((at, v[..q].to_string()));
                }
            }
            start = at + 1;
        }
    }
    best.map(|(_, v)| v).unwrap_or_default()
}

// --- settings.json ----------------------------------------------------------

struct Settings {
    path: PathBuf,
}

impl Settings {
    fn ensure(&self) {
        if !self.path.is_file() {
            if let Some(dir) = self.path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&self.path, "{}\n");
        }
    }

    /// jq が失敗したときの bash は `set -e` で jq の終了コード 2 のまま落ちる。
    fn load(&self) -> Result<J, String> {
        let text = fs::read_to_string(&self.path)
            .map_err(|e| format!("jq: error: Could not open {}: {e}", self.path.display()))?;
        if text.trim().is_empty() {
            // jq は空入力に何も出さない(`.x // empty` は空になる)。
            return Ok(J::Null);
        }
        json::parse(&text).map_err(|e| format!("jq: error (at {}): {e}", self.path.display()))
    }

    fn env_str(&self, key: &str) -> Result<String, String> {
        let root = self.load()?;
        env_lookup(&root, key)
    }

    fn model_key(&self) -> Result<String, String> {
        let root = self.load()?;
        match &root {
            J::Null | J::Obj(_) => Ok(as_raw(json::get(&root, "model"))),
            _ => Err("jq: error: Cannot index non-object with \"model\"".into()),
        }
    }
}

/// `jq -r '<path> // empty'`: null / false は空、文字列は生、その他は compact。
fn as_raw(v: Option<&J>) -> String {
    match v {
        None | Some(J::Null) | Some(J::Bool(false)) => String::new(),
        Some(J::Str(s)) => s.clone(),
        Some(other) => other.compact(),
    }
}

fn env_lookup(root: &J, key: &str) -> Result<String, String> {
    match root {
        J::Null | J::Obj(_) => {}
        _ => return Err("jq: error: Cannot index non-object with \"env\"".into()),
    }
    match json::get(root, "env") {
        None | Some(J::Null) => Ok(String::new()),
        Some(env @ J::Obj(_)) => Ok(as_raw(json::get(env, key))),
        Some(_) => Err(format!("jq: error: Cannot index non-object with \"{key}\"")),
    }
}

// --- モード判定 -------------------------------------------------------------

/// Plan 側。opus エイリアス上書きの値から:
/// uninit(キー不在)/ fable / opus / unknown(このコマンドが管理しない値)。
fn plan_of(v: &str) -> &'static str {
    if v.is_empty() {
        "uninit"
    } else if v.starts_with("claude-fable-") {
        "fable"
    } else if v.starts_with("claude-opus-") {
        "opus"
    } else {
        "unknown"
    }
}

/// 実行側。sonnet エイリアス上書きの値から。不在が *そのまま* sonnet モード
/// (alias 既定が欲しいものなので何も書かない)。具体 Sonnet ID は別の場所由来なので
/// 触らない — 認識できない Plan 側の値と同じ規則。
fn exec_of(v: &str) -> &'static str {
    if v.is_empty() {
        "sonnet"
    } else if v.starts_with("claude-opus-") {
        "opus"
    } else {
        "unknown"
    }
}

/// uninit | unknown | unmanaged | 3 モードのいずれか
fn detect_mode(plan: &str, exec: &str) -> String {
    let p = plan_of(plan);
    let e = exec_of(exec);
    if p == "uninit" {
        return "uninit".into();
    }
    if p == "unknown" || e == "unknown" {
        return "unknown".into();
    }
    match format!("{p}/{e}").as_str() {
        m @ ("fable/sonnet" | "opus/sonnet" | "fable/opus") => m.to_string(),
        _ => "unmanaged".into(),
    }
}

fn report_unmanaged(plan: &str, exec: &str, mode: &str) {
    let p = plan_of(plan);
    let e = exec_of(exec);
    if p == "unknown" {
        warn(&format!(
            ".env.{OPUS_KEY} is \"{plan}\", which this command does not manage."
        ));
    }
    if e == "unknown" {
        warn(&format!(
            ".env.{SONNET_KEY} is \"{exec}\", which this command does not manage."
        ));
    }
    if mode == "unmanaged" {
        warn(&format!(
            "the current pair (plan={p}, exec={e}) is not one of the modes this command manages."
        ));
    }
}

fn normalize_mode(a: &str) -> Option<String> {
    let a = a.replace('-', "/");
    match a.as_str() {
        "fable" => Some("fable/sonnet".into()),
        "opus" => Some("opus/sonnet".into()),
        "fable/sonnet" | "opus/sonnet" | "fable/opus" => Some(a),
        _ => None,
    }
}

fn next_mode(m: &str) -> Option<&'static str> {
    match m {
        "fable/sonnet" => Some("opus/sonnet"),
        "opus/sonnet" => Some("fable/opus"),
        "fable/opus" => Some("fable/sonnet"),
        _ => None,
    }
}

/// split | alias | flat — `.model` が、このコマンドの書くエイリアスとどう関係するか。
///
/// - split: `"opusplan"` — Plan/実行の分割が存在する。構造全体が前提にする唯一の値。
/// - alias: 素のエイリアス — 上書きがセッション *全体* に届く(片側だけでなく)。
///   `opus`/`sonnet` は自明、`haiku` も (plan=sonnet alias, exec=haiku alias) と
///   分割されるので該当、`best` はプロバイダ自身のエイリアス表で解決される。inert
///   ではなく over-broad。
/// - flat: 具体モデル ID、`fable*`(このコマンドが書かない FABLE エイリアス)、または
///   何も無し — エイリアスを迂回するので、切り替えは本当に inert。
fn classify_model_key(m: &str) -> &'static str {
    match m {
        "opusplan" | "opusplan[1m]" => "split",
        "opus" | "opus[1m]" | "sonnet" | "sonnet[1m]" | "haiku" | "haiku[1m]" | "best" => "alias",
        _ => "flat",
    }
}

/// `.model` の class が split でなければ false を返す(bash の `return 1`)。
fn report_model_key(model: &str, class: &str) -> bool {
    match class {
        "split" => return true,
        "alias" => {
            warn(&format!(
                "settings .model is \"{model}\" — a bare alias, and aliases are exactly what this command overrides."
            ));
            match model {
                "opus" | "opus[1m]" => {
                    warn("  the opus alias resolves the WHOLE session here, not just the plan side.")
                }
                "sonnet" | "sonnet[1m]" => warn(
                    "  the sonnet alias resolves the WHOLE session here, not just the execution side.",
                ),
                "haiku" | "haiku[1m]" => warn(
                    "  \"haiku\" splits as (plan=sonnet alias, exec=haiku alias), so the sonnet alias covers its plan side.",
                ),
                "best" => warn("  \"best\" resolves through the provider's own alias table."),
                _ => {}
            }
            warn("  switching would silently rewrite every request, not only plan mode.");
            warn("  select the split first: `/model opusplan` (or re-run with --force).");
        }
        _ => {
            if model.is_empty() {
                warn("settings has no .model; the plan/execution split only exists under \"opusplan\".");
            } else {
                warn(&format!(
                    "settings .model is \"{model}\", which bypasses the aliases — this switch would be inert."
                ));
            }
            warn("  select it with `/model opusplan` (or re-run with --force).");
        }
    }
    false
}

// --- 書き込み ---------------------------------------------------------------

struct Ids {
    fable: String,
    opus: String,
    sonnet: String,
}

enum Applied {
    Updated,
    Unchanged,
    Failed,
}

/// モードの両半分を 1 回の読み書きと 1 回の rename で書く。Claude Code 本体
/// (や herdr-agent-state.sh)との lost update の窓をキーごとに開き直さないため
/// (#61)。
///
/// `*/sonnet` の 2 モードでは SONNET キーを(残さず)削除する: 古い実行側上書きを
/// 残すとモードが嘘になる。
///
/// fallbackModel はモードごとに決まり、そのモードが既に使うモデルとは決して等しくない
/// (Claude Code は "Fallback model cannot be the same as the main model." で拒否する):
///
/// - fable/sonnet -> 最新 Opus(劣化した Plan モデルからの逃げ道)
/// - opus/sonnet  -> 削除(Opus *が* Plan モデル。既に劣化した Plan モデルから黙って
///   さらに下げるのは割に合わない)
/// - fable/opus   -> 最新 Sonnet(このモードが押し出した実行モデル)
///
/// 既にその状態なら(定常状態はファイルに mtime すら触れてはならない)Unchanged。
fn apply_mode(st: &Settings, mode: &str, ids: &Ids) -> Applied {
    let (plan_id, exec_id, fallback): (&str, &str, Vec<&str>) = match mode {
        "fable/sonnet" => (&ids.fable, "", vec![ids.opus.as_str()]),
        "opus/sonnet" => (&ids.opus, "", vec![]),
        "fable/opus" => (&ids.fable, &ids.opus, vec![ids.sonnet.as_str()]),
        _ => {
            warn(&format!("internal error: unknown mode {mode}"));
            return Applied::Failed;
        }
    };

    let orig = match st.load() {
        Ok(j) => j,
        Err(e) => {
            warn(&e);
            return Applied::Failed;
        }
    };
    let mut pairs = match orig.clone() {
        J::Obj(p) => p,
        J::Null => Vec::new(),
        _ => {
            warn("jq: error: Cannot index non-object with \"env\"");
            return Applied::Failed;
        }
    };
    // `.env.X = v`: env が無ければ末尾に作る。
    let mut env_pairs = match json::get(&J::Obj(pairs.clone()), "env") {
        None | Some(J::Null) => Vec::new(),
        Some(J::Obj(p)) => p.clone(),
        Some(_) => {
            warn("jq: error: Cannot index non-object with \"env\"");
            return Applied::Failed;
        }
    };
    json::set(&mut env_pairs, OPUS_KEY, J::str(plan_id));
    if exec_id.is_empty() {
        json::del(&mut env_pairs, SONNET_KEY);
    } else {
        json::set(&mut env_pairs, SONNET_KEY, J::str(exec_id));
    }
    json::set(&mut pairs, "env", J::Obj(env_pairs));
    if fallback.is_empty() {
        json::del(&mut pairs, "fallbackModel");
    } else {
        json::set(
            &mut pairs,
            "fallbackModel",
            J::Arr(fallback.iter().map(|s| J::str(*s)).collect()),
        );
    }
    let new = J::Obj(pairs);

    // jq の `==` と同じく、キー順に依らない構造比較。
    let same = match (
        serde_json::from_str::<serde_json::Value>(&orig.compact()),
        serde_json::from_str::<serde_json::Value>(&new.compact()),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same {
        return Applied::Unchanged;
    }

    let tmp = match make_tmp(&st.path) {
        Ok(p) => p,
        Err(e) => {
            warn(&format!("mktemp: {e}"));
            return Applied::Failed;
        }
    };
    let body = format!("{}\n", new.pretty());
    if fs::write(&tmp, body).is_err() {
        let _ = fs::remove_file(&tmp);
        return Applied::Failed;
    }
    // `chmod --reference` が駄目なら 600。
    let perms = fs::metadata(&st.path).map(|m| m.permissions());
    if perms.and_then(|p| fs::set_permissions(&tmp, p)).is_err() {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    if fs::rename(&tmp, &st.path).is_err() {
        let _ = fs::remove_file(&tmp);
        return Applied::Failed;
    }
    Applied::Updated
}

/// `mktemp "${settings}.hm.XXXXXX"` 相当(mode 600 で排他的に作る)。
fn make_tmp(settings: &Path) -> std::io::Result<PathBuf> {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ u64::from(std::process::id()).rotate_left(32);
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut last = None;
    for _ in 0..100 {
        let mut suffix = String::new();
        for _ in 0..6 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            suffix.push(CH[((seed >> 33) as usize) % CH.len()] as char);
        }
        let mut name = settings.as_os_str().to_owned();
        name.push(format!(".hm.{suffix}"));
        let p = PathBuf::from(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)
        {
            Ok(_) => return Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no temp name")))
}

/// モードが何に解決され、切り替えがどこまで届くか。env は claude の起動時にプロセス環境
/// へ実体化されるので、切り替えは既に動いているセッションには届かない — 黙って no-op に
/// せず毎回そう言う。
fn report_effect(st: &Settings, mode: &str, ids: &Ids) {
    let plan = st.env_str(OPUS_KEY).unwrap_or_default();
    let exec = st.env_str(SONNET_KEY).unwrap_or_default();
    out(&format!("  plan = {plan}"));
    if !exec.is_empty() {
        out(&format!("  exec = {exec}"));
    } else if !ids.sonnet.is_empty() {
        out(&format!("  exec = {} (sonnet alias default)", ids.sonnet));
    } else {
        out("  exec = the sonnet alias default");
    }
    if mode == "fable/opus" {
        out("  note: the sonnet alias now resolves to Opus everywhere — `/model sonnet`");
        out("        and any subagent pinned to `sonnet` run on Opus too.");
    }
    out("this takes effect in newly started sessions only.");
    out(&format!(
        "  running session: restart with `claude --continue`, or use `/model {plan}`"
    ));
}

// --- エントリポイント -------------------------------------------------------

/// 終了コードを返す。
pub fn run(args: &[String]) -> i32 {
    let mut force = false;
    let mut action = String::new();
    let mut have_action = false;

    for a in args {
        match a.as_str() {
            "-h" | "--help" | "help" => {
                let _ = write!(std::io::stdout(), "{}", usage());
                return 0;
            }
            "--force" => force = true,
            s if s.starts_with('-') => {
                warn(&format!("unknown option: {s}"));
                err_raw(&usage());
                return 1;
            }
            s => {
                if have_action && !action.is_empty() {
                    warn(&format!("unexpected argument: {s}"));
                    err_raw(&usage());
                    return 1;
                }
                action = s.to_string();
                have_action = true;
            }
        }
    }

    let mut target_arg = String::new();
    match action.as_str() {
        "" | "status" | "sync" => {}
        other => match normalize_mode(other) {
            Some(m) => target_arg = m,
            None => {
                warn(&format!("unknown argument: {other}"));
                err_raw(&usage());
                return 1;
            }
        },
    }

    let settings_path = std::env::var_os("CLAUDE_PLAN_MODEL_SETTINGS")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".claude/settings.json")
        });
    let st = Settings {
        path: settings_path,
    };
    st.ensure();

    let catalog = match read_catalog() {
        Some(c) if !c.is_empty() => c,
        _ => {
            // claude が無い、またはこのパターンがもう合わないバイナリ。settings.json は
            // そのまま残す: 入っていないプログラムのための env は無害で、書きかけの
            // モデル設定は有害。
            warn("could not read the model catalog from the installed claude binary; leaving settings.json unchanged.");
            return if action == "sync" { 0 } else { 1 };
        }
    };

    let ids = Ids {
        fable: catalog_field(&catalog, "fable"),
        opus: catalog_field(&catalog, "opus"),
        sonnet: catalog_field(&catalog, "sonnet"),
    };
    if ids.fable.is_empty() || ids.opus.is_empty() {
        warn("model catalog is missing a fable/opus entry; leaving settings.json unchanged.");
        return if action == "sync" { 0 } else { 1 };
    }

    let (cur_plan, cur_exec, model) =
        match (st.env_str(OPUS_KEY), st.env_str(SONNET_KEY), st.model_key()) {
            (Ok(p), Ok(e), Ok(m)) => (p, e, m),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                warn(&e);
                return 2;
            }
        };
    let mode = detect_mode(&cur_plan, &cur_exec);
    let model_class = classify_model_key(&model);

    // fable/opus は sonnet ID を要する唯一のモード(fallbackModel のため)。
    let require_sonnet_id = || -> bool {
        if !ids.sonnet.is_empty() {
            return true;
        }
        warn(
            "model catalog is missing the sonnet entry, which fable/opus needs for fallbackModel;",
        );
        warn("  leaving settings.json unchanged.");
        false
    };

    match action.as_str() {
        "status" => {
            match mode.as_str() {
                "uninit" => out("mode = uninitialised (nothing written yet; both aliases fall through to the catalog default)"),
                "unknown" | "unmanaged" => out("mode = unmanaged"),
                m => out(&format!("mode = {m}")),
            }
            let plan_disp = if cur_plan.is_empty() {
                format!("<absent> -> {} (catalog default)", ids.opus)
            } else {
                cur_plan.clone()
            };
            let exec_disp = if cur_exec.is_empty() {
                let s = if ids.sonnet.is_empty() {
                    "catalog default"
                } else {
                    &ids.sonnet
                };
                format!("<absent> -> {s}")
            } else {
                cur_exec.clone()
            };
            out(&format!("  plan (opus alias)   = {plan_disp}"));
            out(&format!("  exec (sonnet alias) = {exec_disp}"));
            let fb = st
                .load()
                .ok()
                .and_then(|r| json::get(&r, "fallbackModel").cloned())
                .filter(|v| !matches!(v, J::Null | J::Bool(false)))
                .map(|v| v.compact())
                .unwrap_or_else(|| "\"none\"".to_string());
            out(&format!("fallbackModel: {fb}"));
            out(&format!(
                "latest known to this claude: fable={} opus={} sonnet={}",
                ids.fable,
                ids.opus,
                if ids.sonnet.is_empty() {
                    "?"
                } else {
                    &ids.sonnet
                }
            ));
            if matches!(mode.as_str(), "unknown" | "unmanaged") {
                report_unmanaged(&cur_plan, &cur_exec, &mode);
            }
            report_model_key(&model, model_class);
            return 0;
        }
        "sync" => {
            // 意図的に .model を見ない: activation は人間の操作ではなく、ここで拒否すると
            // このコマンドが持たない実行時の選択のせいで `hms` が壊れる。
            let target = match mode.as_str() {
                "unknown" | "unmanaged" => {
                    report_unmanaged(&cur_plan, &cur_exec, &mode);
                    warn("  left untouched.");
                    return 0;
                }
                "uninit" => DEFAULT_MODE.to_string(),
                m => m.to_string(),
            };
            if target == "fable/opus" && !require_sonnet_id() {
                return 0;
            }
            return match apply_mode(&st, &target, &ids) {
                Applied::Updated => {
                    let new_exec = st.env_str(SONNET_KEY).unwrap_or_default();
                    let exec_disp = if new_exec.is_empty() {
                        format!("{} via the sonnet alias", ids.sonnet)
                    } else {
                        new_exec
                    };
                    out(&format!(
                        "mode {target}: model config updated (plan={} exec={exec_disp})",
                        st.env_str(OPUS_KEY).unwrap_or_default()
                    ));
                    0
                }
                Applied::Unchanged => 0,
                Applied::Failed => 1,
            };
        }
        _ => {}
    }

    // 書き込み経路: 引数なしのトグル、または明示したモード。
    if matches!(mode.as_str(), "unknown" | "unmanaged") {
        report_unmanaged(&cur_plan, &cur_exec, &mode);
        warn(&format!(
            "  refusing to guess a direction; pick one explicitly, e.g. `{PROG} {DEFAULT_MODE}`."
        ));
        return 1;
    }

    let target = if !target_arg.is_empty() {
        target_arg
    } else if mode == "uninit" {
        DEFAULT_MODE.to_string()
    } else {
        next_mode(&mode).unwrap_or(DEFAULT_MODE).to_string()
    };

    // 書く前に: 切り替えが効かなかったと言いながら、全リクエストのモデルを書き換える
    // のでは本末転倒。
    if model_class != "split" {
        report_model_key(&model, model_class);
        if !force {
            return 1;
        }
        warn("  --force given: applying anyway.");
    }

    if target == "fable/opus" && !require_sonnet_id() {
        return 1;
    }

    let from = if mode == "uninit" {
        "uninitialised".to_string()
    } else {
        mode.clone()
    };
    match apply_mode(&st, &target, &ids) {
        Applied::Updated => out(&format!("mode changed: [{from}] => [{target}]")),
        Applied::Unchanged => out(&format!("mode unchanged: [{target}]")),
        Applied::Failed => return 1,
    }
    report_effect(&st, &target, &ids);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_scan_skips_empty_and_takes_last_field() {
        let blob = b"\0junk latest_per_family:{} more latest_per_family:{fable:\"claude-fable-5-1\",opus:\"claude-opus-5\"}\0";
        let c = scan_catalog(blob).unwrap();
        assert_eq!(
            c,
            "latest_per_family:{fable:\"claude-fable-5-1\",opus:\"claude-opus-5\"}"
        );
        assert_eq!(catalog_field(&c, "fable"), "claude-fable-5-1");
        assert_eq!(catalog_field(&c, "opus"), "claude-opus-5");
        assert_eq!(catalog_field(&c, "sonnet"), "");
        assert_eq!(scan_catalog(b"latest_per_family:{}"), None);
    }

    #[test]
    fn normalize() {
        assert_eq!(normalize_mode("fable-opus").as_deref(), Some("fable/opus"));
        assert_eq!(normalize_mode("fable").as_deref(), Some("fable/sonnet"));
        assert_eq!(normalize_mode("nope"), None);
    }
}
