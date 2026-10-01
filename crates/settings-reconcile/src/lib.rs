//! settings-reconcile — home-manager が配る「宣言」を、Claude Code / Codex /
//! Copilot 自身が実行時に書き換える設定ファイルへ冪等に反映する(ADR-0024、#414)。
//!
//! これらのファイルはツールの所有物(store symlink にできない)なので、activation
//! のたびに「宣言したものだけ」を合わせ、それ以外のキーは触らない。以前は
//! `home/modules/claude.nix` 等の `writeShellScript` に埋め込んだ jq と
//! `scripts/register-{codex,copilot}-hooks` に分かれていたものを、1 つの reconcile
//! 実装にまとめた。
//!
//! ```text
//! settings-reconcile claude-hooks       <settings.json> <spec-json>
//! settings-reconcile claude-permissions <settings.json> <spec-json>
//! settings-reconcile claude-statusline  <settings.json> <spec-json>
//! settings-reconcile claude-mcp-servers <claude.json>   <servers-json>
//! settings-reconcile codex-hooks   <hooks.json>     [--retire <event> <command>]... [--register] [<event> <matcher> <command> <timeout>]...
//! settings-reconcile copilot-hooks <settings.json>  [--retire <event> <command>]... [--register] [<event> <command> <timeoutSec>]...
//! ```
//!
//! spec の JSON スキーマは `hooks::HooksSpec` / `settings::PermissionsSpec` /
//! `settings::StatusLineSpec`。nix 側は `builtins.toJSON` で作って引数で渡す。

pub mod hooks;
pub mod json;
pub mod settings;

use hooks::{Flavor, HooksSpec, Register, Retire};
use json::J;
use std::path::Path;

/// 終了コード(0 成功 / 1 実行時エラー / 2 usage エラー)。
pub struct Failure {
    pub code: u8,
    pub message: String,
}

fn usage(message: impl Into<String>) -> Failure {
    Failure {
        code: 2,
        message: message.into(),
    }
}

fn fail(message: impl Into<String>) -> Failure {
    Failure {
        code: 1,
        message: message.into(),
    }
}

pub const USAGE: &str = "\
usage: settings-reconcile <subcommand> <file> ...
  claude-hooks <settings.json> <spec-json>
  claude-permissions <settings.json> <spec-json>
  claude-statusline <settings.json> <spec-json>
  claude-mcp-servers <claude.json> <servers-json>
  codex-hooks <hooks.json> [--retire <event> <command>]... [--register] <event> <matcher> <command> <timeout> [...]
  copilot-hooks <settings.json> [--retire <event> <command>]... [--register] <event> <command> <timeoutSec> [...]";

pub fn run(args: &[String]) -> Result<(), Failure> {
    let Some(sub) = args.first() else {
        return Err(usage(USAGE));
    };
    let rest = &args[1..];
    match sub.as_str() {
        "claude-hooks" => with_json_spec(rest, |root, spec: HooksSpec| {
            hooks::apply(root, Flavor::Nested, &spec)
        }),
        "claude-permissions" => {
            with_json_spec(rest, |root, spec| settings::permissions(root, &spec))
        }
        "claude-statusline" => with_json_spec(rest, |root, spec| settings::statusline(root, &spec)),
        "claude-mcp-servers" => mcp_servers(rest),
        "codex-hooks" => argv_hooks(rest, Flavor::Nested),
        "copilot-hooks" => argv_hooks(rest, Flavor::Flat),
        _ => Err(usage(USAGE)),
    }
}

fn edit(path: &Path, f: impl FnOnce(&mut J) -> Result<(), String>) -> Result<(), Failure> {
    let before = json::load(path).map_err(fail)?;
    let mut after = before.clone();
    f(&mut after).map_err(|e| fail(format!("{}: {e}", path.display())))?;
    json::store_if_changed(path, &before, &after).map_err(fail)
}

fn with_json_spec<S: serde::de::DeserializeOwned>(
    rest: &[String],
    f: impl FnOnce(&mut J, S) -> Result<(), String>,
) -> Result<(), Failure> {
    let [file, spec] = rest else {
        return Err(usage(USAGE));
    };
    let spec: S = serde_json::from_str(spec).map_err(|e| usage(format!("spec-json: {e}")))?;
    edit(Path::new(file), |root| f(root, spec))
}

fn mcp_servers(rest: &[String]) -> Result<(), Failure> {
    let [file, servers] = rest else {
        return Err(usage(USAGE));
    };
    let new = match json::parse(servers).map_err(|e| usage(format!("servers-json: {e}")))? {
        J::Obj(m) => m,
        _ => return Err(usage("servers-json: JSON オブジェクトが必要")),
    };
    edit(Path::new(file), |root| {
        // 削除は不可逆なので黙って消さない(野良サーバーで実験していた場合、
        // 何が消えたかがここにしか残らない)。
        for name in settings::mcp_servers(root, &new)? {
            eprintln!("settings-reconcile: 宣言外の MCP サーバーを削除します: {name}");
        }
        Ok(())
    })
}

/// codex-hooks / copilot-hooks の旧 bash と同じ argv 形式。
/// `<file> [--retire <event> <command>]... [--register] <tuples>...`。
/// retire は pair ごとに `--retire` トークンを付ける(複数 pair の前に 1 つだけ
/// 置く形は、後続の引数を 1 つずつずらして register を黙って no-op にした
/// 実害があった、#576)。tuple の個数が割り切れない場合は usage エラー(2)。
fn argv_hooks(rest: &[String], flavor: Flavor) -> Result<(), Failure> {
    let width = if flavor == Flavor::Nested { 4 } else { 3 };
    let name = if flavor == Flavor::Nested {
        "codex-hooks"
    } else {
        "copilot-hooks"
    };
    // 旧 bash の入口判定: `--retire` で始まるか、1 組以上の tuple が割り切れること。
    let n = rest.len();
    if !(n >= 1
        && (rest.get(1).map(String::as_str) == Some("--retire")
            || (n > width && (n - 1).is_multiple_of(width))))
    {
        return Err(usage(USAGE));
    }
    let file = &rest[0];
    let mut a = &rest[1..];
    let mut spec = HooksSpec::default();
    while a.first().map(String::as_str) == Some("--retire") {
        if a.len() < 3 {
            return Err(usage(format!(
                "{name}: --retire requires an <event> <command> pair"
            )));
        }
        spec.retire.push(Retire {
            event: a[1].clone(),
            command: a[2].clone(),
        });
        a = &a[3..];
    }
    if a.first().map(String::as_str) == Some("--register") {
        a = &a[1..];
    }
    if !a.is_empty() && !a.len().is_multiple_of(width) {
        return Err(usage(format!(
            "{name}: register tuples must come in groups of {width}, got {} leftover arg(s)",
            a.len()
        )));
    }
    for t in a.chunks(width) {
        let (matcher, command, timeout) = if flavor == Flavor::Nested {
            (Some(t[1].clone()), t[2].clone(), &t[3])
        } else {
            (None, t[1].clone(), &t[2])
        };
        let timeout: serde_json::Number = timeout
            .parse()
            .map_err(|_| usage(format!("{name}: timeout は数値: {timeout}")))?;
        spec.register.push(Register {
            event: t[0].clone(),
            matcher,
            command,
            timeout: Some(timeout),
            if_rule: None,
        });
    }
    edit(Path::new(file), |root| hooks::apply(root, flavor, &spec))
}
