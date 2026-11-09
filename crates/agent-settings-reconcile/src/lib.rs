//! agent-settings-reconcile — home-manager が配る「宣言」を、Claude Code / Codex /
//! Copilot 自身が実行時に書き換える設定ファイルへ冪等に反映する(ADR-0024、#414)。
//!
//! これらのファイルはツールの所有物(store symlink にできない)なので、activation
//! のたびに「宣言したものだけ」を合わせ、それ以外のキーは触らない。以前は
//! `home/modules/claude.nix` 等の `writeShellScript` に埋め込んだ jq と
//! `scripts/register-{codex,copilot}-hooks` に分かれていたものを、1 つの reconcile
//! 実装にまとめた。
//!
//! ```text
//! agent-settings-reconcile claude-hooks       <settings.json> <spec-json>
//! agent-settings-reconcile codex-hooks-spec   <hooks.json>    <spec-json>
//! agent-settings-reconcile copilot-hooks-spec <settings.json> <spec-json>
//! agent-settings-reconcile claude-permissions <settings.json> <spec-json>
//! agent-settings-reconcile claude-statusline  <settings.json> <spec-json>
//! agent-settings-reconcile claude-mcp-servers <claude.json>   <servers-json>
//! agent-settings-reconcile codex-config       <config.toml>   <spec-json>
//! ```
//!
//! spec の JSON スキーマは `hooks::HooksSpec` / `settings::PermissionsSpec` /
//! `settings::StatusLineSpec`(`codex-config` は `codex_config` のドキュメント)。
//! nix 側は `builtins.toJSON` で作って引数で渡す。

pub mod agent_permissions;
pub mod codex_config;
pub mod hooks;
pub mod json;
pub mod managed;
pub mod settings;

use hooks::{Flavor, HooksSpec};
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
usage: agent-settings-reconcile <subcommand> <file> ...
  claude-hooks <settings.json> <spec-json>
  codex-hooks-spec <hooks.json> <spec-json>
  copilot-hooks-spec <settings.json> <spec-json>
  claude-permissions <settings.json> <spec-json>
  claude-statusline <settings.json> <spec-json>
  claude-mcp-servers <claude.json> <servers-json>
  codex-config <config.toml> <spec-json>
  codex-permissions <config.toml> <{settings: object, mcp_allow: array}>";

pub fn run(args: &[String]) -> Result<(), Failure> {
    let Some(sub) = args.first() else {
        return Err(usage(USAGE));
    };
    let rest = &args[1..];
    match sub.as_str() {
        "claude-hooks" | "codex-hooks-spec" => hooks_spec(rest, Flavor::Nested),
        "copilot-hooks-spec" => hooks_spec(rest, Flavor::Flat),
        "claude-permissions" => claude_permissions(rest),
        "claude-statusline" => with_json_spec(rest, |root, spec| settings::statusline(root, &spec)),
        "claude-mcp-servers" => mcp_servers(rest),
        "codex-config" | "codex-permissions" => {
            let [file, spec] = rest else {
                return Err(usage(USAGE));
            };
            if sub == "codex-permissions" {
                codex_config::permissions(Path::new(file), spec).map_err(fail)
            } else {
                codex_config::apply(Path::new(file), spec).map_err(fail)
            }
        }
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

/// `claude-hooks` / `codex-hooks-spec` / `copilot-hooks-spec` `<hooks ファイル> <spec-json>`。
/// 3 つは同じ spec を取り、ファイルの形だけが違う(`flavor`: Claude と Codex は入れ子、
/// Copilot は event 配列に直接並べる平らな形)。spec に `state` があれば、所有の記録
/// (`managed`)を使う: 前回書いたが今回の宣言に無い hook を消し、今回の宣言を記録する。
/// 記録は settings.json の書き込みが成功したあとにだけ更新する(失敗した適用を
/// 「書いた」ことにしない)。
fn hooks_spec(rest: &[String], flavor: Flavor) -> Result<(), Failure> {
    let [file, spec] = rest else {
        return Err(usage(USAGE));
    };
    let spec: HooksSpec =
        serde_json::from_str(spec).map_err(|e| usage(format!("spec-json: {e}")))?;
    let state = spec.state.as_deref().map(Path::new);
    let previous = state.and_then(managed::load);
    let mut declared = None;
    edit(Path::new(file), |root| {
        declared = Some(hooks::apply(root, flavor, &spec, previous.as_ref())?);
        Ok(())
    })?;
    if let (Some(path), Some(declared)) = (state, declared) {
        managed::store_if_changed(path, previous.as_ref(), &declared).map_err(fail)?;
    }
    Ok(())
}

/// `claude-permissions <settings.json> <spec-json>`。spec に `state` があれば、所有の記録を
/// 使う: 前回書いたが今回の宣言に無い `allow` を消し、今回の宣言を記録する。記録は
/// settings.json の書き込みが成功したあとにだけ更新する（`claude_hooks` と同じ）。
fn claude_permissions(rest: &[String]) -> Result<(), Failure> {
    let [file, spec] = rest else {
        return Err(usage(USAGE));
    };
    let spec: settings::PermissionsSpec =
        serde_json::from_str(spec).map_err(|e| usage(format!("spec-json: {e}")))?;
    let state = spec.state.as_deref().map(Path::new);
    let previous = state.and_then(managed::load);
    edit(Path::new(file), |root| {
        settings::permissions(root, &spec, previous.as_ref())
    })?;
    if let Some(path) = state {
        managed::store_if_changed(path, previous.as_ref(), &settings::declared_allow(&spec))
            .map_err(fail)?;
    }
    Ok(())
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
            eprintln!("agent-settings-reconcile: 宣言外の MCP サーバーを削除します: {name}");
        }
        Ok(())
    })
}
