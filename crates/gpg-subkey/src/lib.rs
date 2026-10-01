//! gpg-subkey — card-backed primary 鍵の下に、機体ローカルの ed25519/cv25519 の [S]/[E]
//! (署名/暗号化)subkey を生成・ローテーションし、失効を監視し、リポジトリがコミット
//! する公開鍵 export を更新する(旧 `scripts/gpg-subkey`、ADR-0024 Stage 4e、#414)。
//!
//! 由来: アーカイブ済みの private な前身ツールの generate-machine-subkey.sh /
//! rotate-machine-subkey.sh を吸収したもの(dotfiles GPG tooling review、2026-09-19)。
//! その `--command-file --expert --edit-key` による addkey/revkey のバッチ手順を再利用
//! する(本番で検証済み — company identity の [S] subkey が 2025-12 と 2026-07 にこれで
//! ローテーションされた)が、git config を書く副作用は意図的に落とした。
//! [E] 対応(#252、ADR-0003 Amendment 4)は同じ addkey/revkey の仕組みをメニュー番号だけ
//! 変えて再利用する(sign-only ではなく encrypt-only ECC)。実際の GnuPG との対話で
//! (2026-09-20)確認してからここに符号化した。
//! `home/modules/gpg.nix` のヘッダと `hosts/<host>.nix` の `programs.git.signing.key` が、
//! Git がどの subkey で署名するかの唯一の宣言的な正本(ADR-0003)であり、このツールは
//! git config には決して触れず、次の手動手順を表示するだけ。
//!
//! 使い方:
//! ```text
//! gpg-subkey generate --key <primary-key> [--usage sign|encrypt] [--validity 1y] [--algorithm ed25519]
//! gpg-subkey rotate   --key <primary-key> [--usage sign|encrypt] [--validity 1y] [--revoke-old]
//! gpg-subkey revoke   --key <primary-key> --subkey <keyid>
//! gpg-subkey export   --repo <dotfiles-checkout> --identity <name>
//! gpg-subkey sync     --repo <dotfiles-checkout> --identity <name> [--fix] [--yes]
//! gpg-subkey status   [--repo <dotfiles-checkout>]
//! gpg-subkey remind   [--threshold 30] [--notify]
//! ```
//!
//! generate/rotate はカードの挿入が必要: primary 鍵自身の [C](certify)能力が新しい
//! subkey の binding signature に署名し、その操作は YubiKey 上にある(ADR-0003)。
//! `--key` は `gpg --list-secret-keys` がちょうど 1 つの primary 鍵に解決するもの
//! (fingerprint・keyid・メールの部分文字列)を受ける。`--usage` の既定は後方互換の
//! ため `sign`、`encrypt` は同じ addkey メニューで encrypt-only の cv25519 [E] を作る
//! (メニュー番号は 10 の代わりに 12 — どちらも同じ「どの楕円曲線か」の追問が出る。
//! 実 GnuPG 2.4.9 で確認済み、2026-09-20)。
//!
//! rotate の generate+revoke は唯一の不可逆な手順で、その後ろ — keys/<identity>.pub、
//! GitHub に登録された GPG 鍵、keys.openpgp.org — はローカル鍵束の状態の *コピー* に
//! すぎず、それぞれ独立に、またローカルともずれうる(実際に 1 度起きた)。`sync` は
//! ローカル鍵束との drift を検出し、`--fix` ですべてのコピーを収束させる。前回の試行が
//! どこで止まっていても再実行して安全。`hosts/<host>.nix` は決して編集しない — そこの
//! 不一致は報告するだけで、`export` が引く境界と同じ。
//!
//! rotate は同じ usage の card-backed subkey を決して revoke しない(#252、ADR-0003
//! Amendment 4 — [E] の元の card-backed subkey は、on-disk の [E] が日常の復号を担った
//! 後も fallback/災害復旧経路として意図的に live のまま残す)。「現在どの subkey が有効か」
//! の解決も rotate の revoke 候補収集も、on-disk の秘密素材(GnuPG `--with-colons` 欄 15
//! == "+"、`crates/sign-prewarm` の key_is_on_disk と同じ判定)に限る — card-backed
//! (token serial)と stub(#)は候補にならない。
//!
//! 機密: パスフレーズ・PIN は pinentry(本番)か、テスト用 fixture 鍵の loopback
//! (`GPG_SUBKEY_PINENTRY_MODE` / `GPG_SUBKEY_PASSPHRASE`)でしか渡らず、ここでは
//! ログにも stdout にも出さない。gpg 自身の出力は失敗時にだけ stderr へ流す(bash 版と同じ)。
//!
//! bash 版との差分(意図的、理由は各所のコメントと #414 の報告):
//! - `--selftest` は `tests/selftest.rs`(cargo test)へ移した。
//! - `set -e` のせいで到達不能だった「--key に一致する秘密鍵がありません」等の
//!   エラー経路を、コードの意図どおり動かす(bash 版は gpg の終了コード 2 のまま
//!   無言で落ちていた)。
//! - TSV を `read` で読む箇所の空欄潰れ(失効しない subkey で欄がずれる)を直した。

pub mod colon;

use colon::Sub;
use std::fs;
use std::io::{IsTerminal, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_VALIDITY: &str = "1y";
const DEFAULT_ALGORITHM: &str = "ed25519";
const DEFAULT_USAGE: &str = "sign";
const DEFAULT_THRESHOLD: i64 = 30;

const USAGE: &str = "usage:
  gpg-subkey generate --key <primary-key> [--usage sign|encrypt] [--validity 1y] [--algorithm ed25519]
  gpg-subkey rotate   --key <primary-key> [--usage sign|encrypt] [--validity 1y] [--revoke-old]
  gpg-subkey revoke   --key <primary-key> --subkey <keyid>
  gpg-subkey export   --repo <dotfiles-checkout> --identity <name>
  gpg-subkey sync     --repo <dotfiles-checkout> --identity <name> [--fix] [--yes]
  gpg-subkey status   [--repo <dotfiles-checkout>]
  gpg-subkey remind   [--threshold 30] [--notify]

generate/rotate require the card (the primary key's [C] capability signs the
new subkey). --usage defaults to sign ([S]); encrypt produces an [E]
encrypt-only subkey. rotate never revokes a card-backed subkey of the same
usage — only on-disk material is ever a revoke candidate. revoke is the
standalone form of that same on-disk-only revoke: use it to converge a
subkey that was already superseded without --revoke-old (or by a rotate
that failed partway) — the keyid to pass is whichever `status` still shows
as non-revoked but no longer the one hosts/<host>.nix declares. This tool
never writes git config — export/sync print the next manual step (update
programs.git.signing.key, PR, hms; for --usage encrypt, re-encrypt whatever
this identity's [E] protects, e.g. esa MCP's token.gpg). sync detects (and,
with --fix, converges) drift between the local keyring and keys/*.pub /
GitHub's registered GPG key ([S] only) / keys.openpgp.org ([S] and, once
present, [E]).
";

fn out(s: &str) {
    let _ = writeln!(std::io::stdout(), "{s}");
}
fn err(s: &str) {
    let _ = writeln!(std::io::stderr(), "{s}");
}

/// `$(...)` と同じく末尾の改行を落とす。
fn chomp(s: &str) -> &str {
    s.trim_end_matches('\n')
}

// --- 一時ファイル -----------------------------------------------------------

/// `mktemp` 相当(mode 600 で排他的に作り、Drop で消す)。バッチファイルは鍵の操作手順
/// (passphrase は含まない)だが、他ユーザーに読ませない。
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn create(contents: &[u8]) -> std::io::Result<TempFile> {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            ^ u64::from(std::process::id()).rotate_left(32);
        const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut last = None;
        for _ in 0..100 {
            let mut name = String::from("tmp.");
            for _ in 0..10 {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                name.push(CH[((seed >> 33) as usize) % CH.len()] as char);
            }
            let path = std::env::temp_dir().join(name);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut f) => {
                    let t = TempFile { path };
                    f.write_all(contents)?;
                    return Ok(t);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no temp name")))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// --- gpg 呼び出し -----------------------------------------------------------

/// 設定(環境変数から)。`gpg_bin` / `herdr_bin` はテスト用の継ぎ目。
#[derive(Debug, Clone)]
pub struct Cfg {
    pub gpg_bin: String,
    pub herdr_bin: String,
    /// `GPG_SUBKEY_PINENTRY_MODE`
    pub pinentry_mode: Option<String>,
    /// `GPG_SUBKEY_PASSPHRASE`(空文字も有効 — bash の `[[ -v ... ]]`)
    pub passphrase: Option<String>,
}

impl Cfg {
    pub fn from_env() -> Cfg {
        let nonempty = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Cfg {
            gpg_bin: nonempty("GPG_SUBKEY_GPG_BIN").unwrap_or_else(|| "gpg".into()),
            herdr_bin: nonempty("GPG_SUBKEY_HERDR_BIN").unwrap_or_else(|| "herdr".into()),
            pinentry_mode: nonempty("GPG_SUBKEY_PINENTRY_MODE"),
            passphrase: std::env::var("GPG_SUBKEY_PASSPHRASE").ok(),
        }
    }

    /// `--edit-key` 呼び出しに足す gpg フラグ。本番では空: 実 primary 鍵の
    /// passphrase/PIN は、操作者が設定した pinentry(`home/modules/gpg.nix` 経由の
    /// pinentry-gnome3)を通って対話的に渡る。`--selftest`(今は tests/selftest.rs)が
    /// `GPG_SUBKEY_PINENTRY_MODE=loopback` + `GPG_SUBKEY_PASSPHRASE=''` で使い捨ての
    /// 空 passphrase fixture 鍵を非対話に解錠する — 他の場面ではどちらの env も未設定な
    /// のでこの切り替えは発火しない。
    ///
    /// bash 版には「空配列でも `printf '%s\n' "${args[@]}"` が空行を 1 つ出して、
    /// `--command-file` の前に空文字の位置引数が入り gpg が以降のオプションを認識しなく
    /// なる」落とし穴があった(まさに本番経路)。Vec を直接返すのでそもそも起きない。
    pub fn edit_key_extra_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(m) = &self.pinentry_mode {
            args.push("--pinentry-mode".to_string());
            args.push(m.clone());
        }
        if let Some(p) = &self.passphrase {
            args.push("--passphrase".to_string());
            args.push(p.clone());
        }
        args
    }

    /// gpg を走らせて stdout を得る(stderr は捨てる)。終了コードは見ない — bash 版の
    /// `2>/dev/null | awk` と同じく、読めた分だけを使う。
    fn gpg_stdout(&self, args: &[&str]) -> String {
        Command::new(&self.gpg_bin)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }

    fn list_secret(&self, selector: &str) -> String {
        let mut args = vec!["--list-secret-keys", "--with-colons", "--with-fingerprint"];
        if !selector.is_empty() {
            args.push(selector);
        }
        self.gpg_stdout(&args)
    }

    fn resolve_primary_key(&self, selector: &str) -> Vec<String> {
        colon::primary_fprs(&self.list_secret(selector))
    }

    fn primary_uid(&self, fpr: &str) -> String {
        colon::primary_uid(&self.list_secret(fpr))
    }

    /// `usage_subkeys <cap-char> [selector] [on-disk]`。`selector` が空なら全 secret
    /// primary を走査する。
    pub fn usage_subkeys(&self, cap: char, selector: &str, ondisk_only: bool) -> Vec<Sub> {
        colon::usage_subkeys(&self.list_secret(selector), cap, ondisk_only)
    }

    /// その capability の、最も新しく作られた revoke されていない on-disk subkey の
    /// keyid。`>=`(`>` ではない)なので、同じ秒内(generate の直後の rotate など、どちらも
    /// `date +%s` 精度)に作られた 2 本は、gpg が最後に列挙した方 = 実際に最後に足された方に
    /// 倒れる。
    ///
    /// on-disk 限定の理由(#252, R1-B-1): 「現行」を card-backed/stub まで含めて解決すると、
    /// generate/rotate 直後の新規鍵の特定を誤ったり、sync がカード専用の [E] を「現行」と
    /// 誤認したりする。card-backed な同一 usage の subkey(元 [E] 等)は意図して残置される
    /// 既知の状態であり、これを「現行」扱いしないことは設計そのもの(ADR-0003 Amendment 4)。
    pub fn extract_latest_usage_keyid(&self, cap: char, selector: &str) -> String {
        latest_keyid(&self.usage_subkeys(cap, selector, true))
    }

    fn export_armor(&self, fpr: &str) -> String {
        chomp(&self.gpg_stdout(&["--armor", "--export", fpr])).to_string()
    }

    /// `gpg --batch <extra> --command-file <batch> --expert --edit-key <fpr>`。stdin は
    /// 親から引き継ぐ(pinentry が tty を使う経路のため)。失敗時は出力(stdout+stderr)を
    /// 返す。
    fn edit_key(&self, fpr: &str, batch: &str) -> Result<(), String> {
        let file = TempFile::create(batch.as_bytes()).map_err(|e| format!("mktemp: {e}"))?;
        let o = Command::new(&self.gpg_bin)
            .arg("--batch")
            .args(self.edit_key_extra_args())
            .arg("--command-file")
            .arg(&file.path)
            .args(["--expert", "--edit-key", fpr])
            .stdin(Stdio::inherit())
            .output()
            .map_err(|e| format!("{}: {e}", self.gpg_bin))?;
        if o.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ))
        }
    }
}

fn latest_keyid(rows: &[Sub]) -> String {
    let mut best: Option<(i64, &str)> = None;
    for r in rows.iter().filter(|r| !r.revoked) {
        let c = r.created_epoch();
        if best.is_none_or(|(m, _)| c >= m) {
            best = Some((c, &r.keyid));
        }
    }
    best.map(|(_, k)| k.to_string()).unwrap_or_default()
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 切り上げの除算。失効なし(空 or 0)は None("inf")。
pub fn days_until(expire: &str, now: i64) -> Option<i64> {
    if expire.is_empty() || expire == "0" {
        return None;
    }
    let e: i64 = expire.parse().unwrap_or(0);
    Some((e - now + 86399) / 86400)
}

// --- 引数 -------------------------------------------------------------------

/// 値を取るオプションの値を読む。bash 版は `$2: unbound variable`(終了コード 1)で落ちて
/// いたので、同じ終了コードで分かる文言にする。
fn opt_val(args: &[String], i: &mut usize, cmd: &str) -> Result<String, i32> {
    let name = &args[*i];
    match args.get(*i + 1) {
        Some(v) => {
            *i += 2;
            Ok(v.clone())
        }
        None => {
            err(&format!("gpg-subkey {cmd}: {name} に値が必要です"));
            Err(1)
        }
    }
}

fn usage_cap(cmd: &str, usage_kind: &str) -> Result<(char, &'static str), i32> {
    match usage_kind {
        "sign" => Ok(('s', "10")),
        "encrypt" => Ok(('e', "12")),
        _ => {
            err(&format!(
                "gpg-subkey {cmd}: usage={usage_kind} は未対応(sign|encrypt のみ)"
            ));
            Err(2)
        }
    }
}

// --- generate / rotate / revoke ---------------------------------------------

fn cmd_generate(cfg: &Cfg, args: &[String]) -> i32 {
    let (mut key, mut validity, mut algorithm, mut usage_kind) = (
        String::new(),
        DEFAULT_VALIDITY.to_string(),
        DEFAULT_ALGORITHM.to_string(),
        DEFAULT_USAGE.to_string(),
    );
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--key" => opt_val(args, &mut i, "generate").map(|v| key = v),
            "--validity" => opt_val(args, &mut i, "generate").map(|v| validity = v),
            "--algorithm" => opt_val(args, &mut i, "generate").map(|v| algorithm = v),
            "--usage" => opt_val(args, &mut i, "generate").map(|v| usage_kind = v),
            other => {
                err(&format!("gpg-subkey generate: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    if algorithm != "ed25519" {
        err(&format!(
            "gpg-subkey generate: algorithm={algorithm} は未対応(ed25519 のみ)"
        ));
        return 2;
    }
    let (cap, choice) = match usage_cap("generate", &usage_kind) {
        Ok(v) => v,
        Err(rc) => return rc,
    };
    if key.is_empty() {
        err("gpg-subkey generate: --key が必要です");
        return 2;
    }
    match generate_on(cfg, &key, &validity, cap, choice) {
        Ok(keyid) => {
            out(&keyid);
            0
        }
        Err(()) => 1,
    }
}

/// primary を解決し、検証済みのコマンドファイルのメニュー回答で ECC subkey を addkey
/// し、新しい subkey の keyid を返す(呼び出し側が stdout に出す)。addkey メニュー番号
/// 10 = "ECC (sign only)" → ed25519、12 = "ECC (encrypt only)" → cv25519。どちらも同一の
/// 「楕円曲線の選択」追問が出て 1 = "Curve 25519" — この数字を決め打ちする前に実 GnuPG
/// 2.4.9 で対話的に確認済み(2026-09-20)。
///
/// bash 版は `candidates="$(resolve_primary_key ...)"` が set -e で gpg の終了コード 2 の
/// まま無言で落ち、下のメッセージはどれも到達不能だった。ここでは意図どおりに出す。
fn generate_on(
    cfg: &Cfg,
    selector: &str,
    validity: &str,
    cap: char,
    choice: &str,
) -> Result<String, ()> {
    let candidates = cfg.resolve_primary_key(selector);
    if candidates.is_empty() {
        err(&format!(
            "gpg-subkey: --key {selector} に一致する秘密鍵がありません"
        ));
        return Err(());
    }
    if candidates.len() > 1 {
        err(&format!(
            "gpg-subkey: --key {selector} が複数の秘密鍵に一致しました。fingerprint で指定してください:"
        ));
        err(&candidates.join("\n"));
        return Err(());
    }
    let fpr = &candidates[0];
    err(&format!(
        "gpg-subkey: primary={fpr} ({}) に [{}] subkey を追加します(YubiKey のタッチ/PIN が必要です)",
        cfg.primary_uid(fpr),
        cap.to_ascii_uppercase()
    ));

    let batch = format!("addkey\n{choice}\n1\n{validity}\ny\nsave\nquit\n");
    if let Err(output) = cfg.edit_key(fpr, &batch) {
        err(chomp(&output));
        err("gpg-subkey: subkey 生成に失敗しました");
        return Err(());
    }

    let new_keyid = cfg.extract_latest_usage_keyid(cap, fpr);
    if new_keyid.is_empty() {
        err(&format!(
            "gpg-subkey: 生成後の新しい [{}] subkey を特定できませんでした",
            cap.to_ascii_uppercase()
        ));
        return Err(());
    }
    if cap == 'e' {
        err("gpg-subkey: 次の手順(このツールはファイルには触れません): この identity の [E]");
        err(&format!(
            "  で暗号化している対象(例: esa MCP の token.gpg)を、新しい [E] subkey {new_keyid}"
        ));
        err("  宛に再暗号化してください(docs/claude/esa-mcp.md 参照)");
    }
    Ok(new_keyid)
}

fn cmd_rotate(cfg: &Cfg, args: &[String]) -> i32 {
    let (mut key, mut validity, mut revoke_old, mut usage_kind) = (
        String::new(),
        DEFAULT_VALIDITY.to_string(),
        false,
        DEFAULT_USAGE.to_string(),
    );
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--key" => opt_val(args, &mut i, "rotate").map(|v| key = v),
            "--validity" => opt_val(args, &mut i, "rotate").map(|v| validity = v),
            "--usage" => opt_val(args, &mut i, "rotate").map(|v| usage_kind = v),
            "--revoke-old" => {
                revoke_old = true;
                i += 1;
                Ok(())
            }
            other => {
                err(&format!("gpg-subkey rotate: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    if key.is_empty() {
        err("gpg-subkey rotate: --key が必要です");
        return 2;
    }
    let (cap, choice) = match usage_cap("rotate", &usage_kind) {
        Ok(v) => v,
        Err(rc) => return rc,
    };

    let fprs = cfg.resolve_primary_key(&key);
    if fprs.len() != 1 {
        err(&format!(
            "gpg-subkey: --key {key} が primary key を一意に特定できません"
        ));
        return 1;
    }
    let fpr = &fprs[0];
    // on-disk 限定(#252, R1-B-1): card-backed な同 usage の subkey(例: [E] の
    // カード上の元鍵)を revoke 対象に含めない。カード鍵の残置は設計そのもの
    // (ADR-0003 Amendment 4)であり、rotate が誤って失効させてはならない。
    let old_keyids: Vec<String> = cfg
        .usage_subkeys(cap, fpr, true)
        .into_iter()
        .filter(|s| !s.revoked)
        .map(|s| s.keyid)
        .collect();

    let new_keyid = match generate_on(cfg, fpr, &validity, cap, choice) {
        Ok(k) => k,
        Err(()) => return 1,
    };
    out(&new_keyid);

    // 新しい鍵はどちらにせよ作られている — 下の revoke の失敗を全体の成功に呑み込ませて
    // はならない。さもないと `rotate --revoke-old` が live な [S] subkey を 2 本残した
    // まま「ローテーション完了」を名乗る(この穴は既に 1 度出荷された: かつて
    // revoke_subkey が失敗時に `return 0` していた)。
    let mut revoke_failed = false;
    if revoke_old && !old_keyids.is_empty() {
        for old in &old_keyids {
            if revoke_subkey(cfg, fpr, old, "gpg-subkey rotate").is_err() {
                revoke_failed = true;
            }
        }
    } else if revoke_old {
        err(&format!(
            "gpg-subkey: revoke 対象の旧 on-disk [{}] subkey がありませんでした",
            cap.to_ascii_uppercase()
        ));
    }
    if revoke_failed {
        err(&format!(
            "gpg-subkey: 新 subkey {new_keyid} は生成済みですが、旧 subkey の revoke に失敗したものがあります(要手動確認: gpg-subkey status)"
        ));
        return 1;
    }
    0
}

fn revoke_subkey(cfg: &Cfg, fpr: &str, keyid: &str, reason: &str) -> Result<(), ()> {
    let batch = format!("key {keyid}\nrevkey\ny\n0\n{reason}\n\ny\nsave\nquit\n");
    match cfg.edit_key(fpr, &batch) {
        Ok(()) => {
            err(&format!("gpg-subkey: 旧 subkey {keyid} を revoke しました"));
            Ok(())
        }
        Err(output) => {
            err(chomp(&output));
            err(&format!(
                "gpg-subkey: 旧 subkey {keyid} の revoke に失敗しました"
            ));
            Err(())
        }
    }
}

/// `gpg-subkey revoke --key <primary-key> --subkey <keyid>`
///
/// `rotate --revoke-old` は、置き換えの生成と同時にしか旧 subkey を revoke しない。
/// それだと、`--revoke-old` 無しで既にローテーション済みの subkey(や途中で失敗した
/// 前回の rotate)を、revoke 経路を発火させるためだけにもう 1 本新しく作らずには
/// 収束させられない — このコマンドがそこを直接埋める。
fn cmd_revoke(cfg: &Cfg, args: &[String]) -> i32 {
    let (mut key, mut subkey) = (String::new(), String::new());
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--key" => opt_val(args, &mut i, "revoke").map(|v| key = v),
            "--subkey" => opt_val(args, &mut i, "revoke").map(|v| subkey = v),
            other => {
                err(&format!("gpg-subkey revoke: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    if key.is_empty() || subkey.is_empty() {
        err("gpg-subkey revoke: --key と --subkey が必要です");
        return 2;
    }

    let fprs = cfg.resolve_primary_key(&key);
    if fprs.len() != 1 {
        err(&format!(
            "gpg-subkey: --key {key} が primary key を一意に特定できません"
        ));
        return 1;
    }
    let fpr = &fprs[0];

    let Some((revoked, _cap, loc)) = colon::find_subkey_row(&cfg.list_secret(fpr), &subkey) else {
        err(&format!(
            "gpg-subkey revoke: subkey {subkey} が primary {fpr} の下に見つかりません"
        ));
        return 1;
    };
    if revoked {
        err(&format!(
            "gpg-subkey revoke: subkey {subkey} は既に revoked です"
        ));
        return 1;
    }
    // rotate と同じ on-disk 限定(#252, R1-B-1 の判定を踏襲): card-backed な subkey の
    // 残置は ADR-0003 Amendment 4 の意図的な設計であり、このツールの revoke 対象に
    // 含めない。
    if loc != "+" {
        err(&format!(
            "gpg-subkey revoke: subkey {subkey} は on-disk ではありません(card-backed/stub は rotate と同じくこのツールの revoke 対象外 — ADR-0003 Amendment 4 の意図的な残置設計)"
        ));
        return 1;
    }
    match revoke_subkey(cfg, fpr, &subkey, "gpg-subkey revoke") {
        Ok(()) => 0,
        Err(()) => 1,
    }
}

// --- export / sync ----------------------------------------------------------

/// keys/<identity>.pub の primary fingerprint。読めなければ stderr に理由を出して Err。
fn repo_identity_fpr(cfg: &Cfg, repo: &str, identity: &str) -> Result<String, ()> {
    let pubfile = format!("{repo}/keys/{identity}.pub");
    if !Path::new(&pubfile).is_file() {
        err(&format!("gpg-subkey: {pubfile} がありません"));
        return Err(());
    }
    let listing = cfg.gpg_stdout(&["--with-colons", "--show-keys", &pubfile]);
    match colon::pubkey_fpr(&listing).filter(|f| !f.is_empty()) {
        Some(f) => Ok(f),
        None => {
            err(&format!(
                "gpg-subkey: {pubfile} から primary fingerprint を読めませんでした"
            ));
            Err(())
        }
    }
}

fn read_chomped(path: &str) -> String {
    fs::read_to_string(path)
        .map(|s| chomp(&s).to_string())
        .unwrap_or_default()
}

fn cmd_export(cfg: &Cfg, args: &[String]) -> i32 {
    let (mut repo, mut identity) = (String::new(), String::new());
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--repo" => opt_val(args, &mut i, "export").map(|v| repo = v),
            "--identity" => opt_val(args, &mut i, "export").map(|v| identity = v),
            other => {
                err(&format!("gpg-subkey export: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    if repo.is_empty() || identity.is_empty() {
        err("gpg-subkey export: --repo と --identity が必要です");
        return 2;
    }
    let pubfile = format!("{repo}/keys/{identity}.pub");
    let Ok(fpr) = repo_identity_fpr(cfg, &repo, &identity) else {
        return 1;
    };
    let new_export = cfg.export_armor(&fpr);
    if new_export.is_empty() {
        err(&format!(
            "gpg-subkey export: {fpr} のローカル鍵束から export できませんでした"
        ));
        return 1;
    }
    if new_export == read_chomped(&pubfile) {
        out(&format!("gpg-subkey export: {pubfile} に変更はありません"));
    } else {
        if let Err(e) = fs::write(&pubfile, format!("{new_export}\n")) {
            err(&format!("gpg-subkey export: {pubfile} を書けません: {e}"));
            return 1;
        }
        out(&format!("gpg-subkey export: {pubfile} を更新しました"));
    }
    let new_keyid = cfg.extract_latest_usage_keyid('s', &fpr);
    out("\n次の手順(このツールは git config / nix ファイルには触れません):");
    out(&format!(
        "  1. hosts/<host>.nix の programs.git.signing.key を {} に更新",
        if new_keyid.is_empty() {
            "<subkey-id>"
        } else {
            &new_keyid
        }
    ));
    out("  2. commit → push → PR");
    out("  3. hms でこのホストに適用");
    0
}

/// ホスト名(`hostname(1)`、無ければカーネルの値)。
fn hostname() -> String {
    let from_cmd = Command::new("hostname")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| chomp(&String::from_utf8_lossy(&o.stdout)).to_string())
        .filter(|h| !h.is_empty());
    from_cmd
        .or_else(|| {
            fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default()
}

fn gh_user_gpg_keys() -> Option<serde_json::Value> {
    let o = Command::new("gh")
        .args(["api", "user/gpg_keys"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&o.stdout);
    if text.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&text).ok()
}

/// `jq -c --arg k K '[.[] | select(.key_id==$k)][0] // empty'`
fn gh_entry<'a>(
    gh_json: &'a serde_json::Value,
    primary_short: &str,
) -> Option<&'a serde_json::Value> {
    gh_json
        .as_array()?
        .iter()
        .find(|e| e["key_id"].as_str() == Some(primary_short))
}

/// `.subkeys[]? | select(.key_id==$k)` の最初の一致。
fn gh_subkey<'a>(entry: &'a serde_json::Value, keyid: &str) -> Option<&'a serde_json::Value> {
    entry["subkeys"]
        .as_array()?
        .iter()
        .find(|s| s["key_id"].as_str() == Some(keyid))
}

/// `grep -qF NEEDLE <(grep KEYID <<<"$colons")`: keyid を含む行のどれかが needle を含む。
fn keyserver_has(colons: &str, keyid: &str, needle: &str) -> bool {
    colons
        .lines()
        .any(|l| l.contains(keyid) && l.contains(needle))
}

/// `sync` — `rotate`(ローカル GnuPG の generate+revoke)はローテーションで唯一の本当に
/// 不可逆な手順で、その下流 — リポジトリの keys/*.pub export、GitHub に登録された GPG 鍵、
/// keys.openpgp.org の公開コピー — はその状態の *コピー* にすぎず、コピーは独立に
/// ずれうる(実際に出荷された: `gh -f key=@file` が再アップロードを黙って壊し、コミット済み
/// keys/personal.pub が手で気付くまで古いままだった)。GnuPG + GitHub API + 独立した
/// keyserver をまたぐ真の原子性は共有のトランザクション調停者が無いので得られない — なので
/// `sync` は現実的な代案を採る: ローカル鍵束(正本)に対する drift を検出し、下流のコピーを
/// すべてそこへ収束させる。前回の試行がどこで止まっていても再実行して安全。
/// `home/hosts/<host>.nix` の `programs.git.signing.key` は報告するだけで編集しない
/// — `export` が引く境界と同じ。
///
/// [S] と [E] で監査対象が違う(#252, ADR-0003 Amendment 4): GitHub の GPG 登録は署名検証
/// 用途なので [S] のみ照合する。hosts/*.nix の programs.git.signing.key も [S] の宣言専用。
/// export(主鍵の armored 一括 export)と keys.openpgp.org は [S]/[E] 両方を含む/照合する
/// — 主鍵の export は元々両方の subkey を含むため usage で分岐する必要がない。on-disk [E]
/// がまだ無いホスト(#252 ロールアウト前)では [E] 関連チェックを無音でスキップする —
/// カードのみの運用も引き続き妥当な状態のため。
fn cmd_sync(cfg: &Cfg, args: &[String]) -> i32 {
    let (mut repo, mut identity, mut fix, mut yes) = (String::new(), String::new(), false, false);
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--repo" => opt_val(args, &mut i, "sync").map(|v| repo = v),
            "--identity" => opt_val(args, &mut i, "sync").map(|v| identity = v),
            "--fix" => {
                fix = true;
                i += 1;
                Ok(())
            }
            "--yes" => {
                yes = true;
                i += 1;
                Ok(())
            }
            other => {
                err(&format!("gpg-subkey sync: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    if repo.is_empty() || identity.is_empty() {
        err("gpg-subkey sync: --repo と --identity が必要です");
        return 2;
    }
    let Ok(fpr) = repo_identity_fpr(cfg, &repo, &identity) else {
        return 1;
    };

    let sign_active_keyid = cfg.extract_latest_usage_keyid('s', &fpr);
    if sign_active_keyid.is_empty() {
        err(&format!(
            "gpg-subkey sync: {fpr} に有効な on-disk [S] subkey がありません"
        ));
        return 1;
    }
    let sign_active_fpr = cfg
        .usage_subkeys('s', &fpr, false)
        .into_iter()
        .find(|s| s.keyid == sign_active_keyid)
        .map(|s| s.subkey_fpr)
        .unwrap_or_default();

    let encrypt_active_keyid = cfg.extract_latest_usage_keyid('e', &fpr);

    // revoked の収集はロケーションで絞らない — 失効は保管場所(on-disk/card)に関わらず
    // 全掲載箇所へ反映されるべきであり、drift 検査の対象はそこにある。
    let (mut sign_revoked, mut encrypt_revoked): (Vec<String>, Vec<String>) = (vec![], vec![]);
    let all = cfg
        .usage_subkeys('s', &fpr, false)
        .into_iter()
        .chain(cfg.usage_subkeys('e', &fpr, false));
    for s in all {
        if s.keyid.is_empty() || !s.revoked {
            continue;
        }
        if s.cap.contains('s') {
            sign_revoked.push(s.keyid);
        } else if s.cap.contains('e') {
            encrypt_revoked.push(s.keyid);
        }
    }

    let mut problems: Vec<&str> = Vec::new();

    // 1. repo keys/<identity>.pub — 主鍵の armored export は [S]/[E] 両方を含むため、
    // usage で分けず 1 回の比較で両方のドリフトを検出できる。
    let pubfile = format!("{repo}/keys/{identity}.pub");
    let current_export = cfg.export_armor(&fpr);
    if current_export != read_chomped(&pubfile) {
        problems.push("export");
    }

    // 2. GitHub — [S] のみ(GitHub の GPG 登録は署名検証用途)。primary 鍵ごとに 1 登録で、
    // 短い keyid で引く。
    let primary_short: String = fpr
        .chars()
        .skip(fpr.chars().count().saturating_sub(16))
        .collect();
    let gh_json = gh_user_gpg_keys();
    match &gh_json {
        None => err("gpg-subkey sync: GitHub API に到達できませんでした(gh 未認証/ネットワーク不通) — スキップ"),
        Some(json) => {
            let mut github_drift = false;
            match gh_entry(json, &primary_short) {
                None => github_drift = true,
                Some(entry) => {
                    let active_ok = gh_subkey(entry, &sign_active_keyid)
                        .map(|s| s["can_sign"].as_bool() == Some(true) && s["revoked"].as_bool() != Some(true))
                        .unwrap_or(false);
                    if !active_ok {
                        github_drift = true;
                    }
                    for rk in &sign_revoked {
                        // 掲載が無い(空)か revoked == true なら良い。
                        if let Some(s) = gh_subkey(entry, rk) {
                            if s["revoked"].as_bool() != Some(true) {
                                github_drift = true;
                            }
                        }
                    }
                }
            }
            if github_drift {
                problems.push("github");
            }
        }
    }

    // 3. keys.openpgp.org — [S] は必須、[E] はこのホストに on-disk [E] がある場合のみ検査
    // 対象に加える。変数ではなく一時ファイル経由にするのは、VKS の応答が binary の OpenPGP
    // 鍵素材で、`$(...)` のコマンド置換は binary(埋め込み NUL、末尾改行の除去)を壊し
    // うるため。
    let mut keyserver_drift = false;
    let ks_file = TempFile::create(b"");
    let fetched = ks_file.as_ref().ok().is_some_and(|f| {
        Command::new("curl")
            .args([
                "-sf",
                &format!("https://keys.openpgp.org/vks/v1/by-fingerprint/{fpr}"),
                "-o",
            ])
            .arg(&f.path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            && fs::metadata(&f.path).map(|m| m.len() > 0).unwrap_or(false)
    });
    if !fetched {
        keyserver_drift = true;
    } else if let Ok(f) = &ks_file {
        let colons = cfg.gpg_stdout(&["--show-keys", "--with-colons", &f.path.to_string_lossy()]);
        if !keyserver_has(&colons, &sign_active_keyid, "sub:u:") {
            keyserver_drift = true;
        }
        for rk in &sign_revoked {
            if !keyserver_has(&colons, rk, "sub:r:") {
                keyserver_drift = true;
            }
        }
        if !encrypt_active_keyid.is_empty() {
            if !keyserver_has(&colons, &encrypt_active_keyid, "sub:u:") {
                keyserver_drift = true;
            }
            for rk in &encrypt_revoked {
                if !keyserver_has(&colons, rk, "sub:r:") {
                    keyserver_drift = true;
                }
            }
        }
    }
    drop(ks_file);
    if keyserver_drift {
        problems.push("keyserver");
    }

    // 4. THIS host の hosts/<host>.nix のみ — [S] のみ、report-only、このツールは nix
    // ファイルには触れない。[S] は per-machine(ADR-0003 Amendment §1): 第二のホストが同じ
    // identity を共有していても(例: altair と vega)、そのホストは自分自身の独立した [S]
    // subkey を持つのが期待される挙動であり、ここでの鍵束には存在すらしない — 「このマシン」
    // の sign_active_fpr で他ホストの hosts/*.nix を全部チェックすると、単に別の(同様に
    // 正当な)subkey を持つだけの他ホストを毎回 stale と誤報する。「このホスト」の解決は
    // scripts/hms.sh の resolve_host() と同じ規則: dotfiles/host マーカー、無ければ
    // `hostname`。
    let mut stale_hosts: Vec<String> = Vec::new();
    let mut this_host_source = "dotfiles/host マーカー";
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    let marker = format!("{xdg}/dotfiles/host");
    let mut this_host = fs::read_to_string(&marker)
        .ok()
        .map(|s| {
            s.lines()
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
        })
        .unwrap_or_default();
    if this_host.is_empty() {
        this_host = hostname();
        this_host_source = "hostname フォールバック(dotfiles/host マーカー未設置)";
    }
    let hf = format!("{repo}/home/hosts/{this_host}.nix");
    if Path::new(&hf).is_file() {
        let text = fs::read_to_string(&hf).unwrap_or_default();
        if text.contains(&format!("identities/{identity}.nix")) {
            let hf_key = text
                .lines()
                .find_map(signing_key_of_line)
                .unwrap_or_default();
            if hf_key != sign_active_fpr {
                stale_hosts.push(hf.clone());
            }
        }
        // hf が存在してもこの identity を宣言していなければ、このホストはそもそもこの
        // identity を使わない(正当なスキップ、drift ではない)。
    } else {
        // #423: hf 自体が無い(マーカー未設置ホスト、または改名前のホスト名)場合、以前は
        // ここを黙って skip していた — signing key の stale 判定を一度も検査せずに `sync`
        // が exit 0(同期済み)を返してしまう穴だった(gpg-subkey-rotation skill の「sync が
        // exit 0 を報告するまで繰り返す」契約と矛盾する)。hostname フォールバック自体は
        // ADR-0019 が保護する正当な挙動なので外さず、「検査できなかった」ことを drift と
        // して報告する側に倒す(Saltzer & Schroeder の fail-safe defaults)。
        problems.push("hostfile");
    }

    if problems.is_empty() && stale_hosts.is_empty() {
        if !encrypt_active_keyid.is_empty() {
            out(&format!(
                "gpg-subkey sync: {identity} は同期済みです(現行 [S] subkey {sign_active_keyid}、現行 [E] subkey {encrypt_active_keyid})"
            ));
        } else {
            out(&format!(
                "gpg-subkey sync: {identity} は同期済みです(現行 [S] subkey {sign_active_keyid})"
            ));
        }
        return 0;
    }

    out(&format!("gpg-subkey sync: {identity} の drift:"));
    for p in &problems {
        match *p {
            "export" => out(&format!("  - keys/{identity}.pub がローカル鍵束より古い")),
            "github" => out(&format!(
                "  - GitHub の GPG 登録が現行 [S] subkey {sign_active_keyid} を反映していない"
            )),
            "keyserver" => {
                out(&format!(
                    "  - keys.openpgp.org が現行 [S] subkey {sign_active_keyid} を反映していない可能性があります"
                ));
                if !encrypt_active_keyid.is_empty() {
                    out(&format!(
                        "  - keys.openpgp.org が現行 [E] subkey {encrypt_active_keyid} を反映していない可能性があります"
                    ));
                }
            }
            "hostfile" => out(&format!(
                "  - このホスト({this_host}、解決元: {this_host_source})に {hf} が無く、signing key の stale 判定を検査できません(マーカーを設置するか home/hosts/ にホストを追加してください)"
            )),
            _ => {}
        }
    }
    for h in &stale_hosts {
        out(&format!(
            "  - {h} の programs.git.signing.key が現行 [S] subkey ({sign_active_fpr}) と異なる(手動更新が必要)"
        ));
    }

    if !fix {
        return 1;
    }

    if !yes {
        if !std::io::stdin().is_terminal() {
            err("gpg-subkey sync --fix: 非対話実行では --yes が必要です");
            return 2;
        }
        let _ = write!(
            std::io::stderr(),
            "上記のうち keys.pub/GitHub/keyserver を現行状態へ反映しますか? [y/N] "
        );
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        let a = answer.trim_end_matches(['\n', '\r']);
        if a != "y" && a != "Y" {
            return 1;
        }
    }

    for p in &problems {
        match *p {
            "export" => {
                if let Err(e) = fs::write(&pubfile, format!("{current_export}\n")) {
                    err(&format!("gpg-subkey sync: {pubfile} を書けません: {e}"));
                } else {
                    out(&format!("gpg-subkey sync: {pubfile} を更新しました"));
                }
            }
            "github" => {
                if let Some(json) = &gh_json {
                    fix_github(json, &primary_short, &current_export);
                }
            }
            "keyserver" => {
                let ok = Command::new(&cfg.gpg_bin)
                    .args([
                        "--keyserver",
                        "hkps://keys.openpgp.org",
                        "--send-keys",
                        &fpr,
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    out("gpg-subkey sync: keys.openpgp.org へ送信しました");
                } else {
                    err("gpg-subkey sync: keys.openpgp.org への送信に失敗しました");
                }
            }
            _ => {}
        }
    }
    0
}

/// `sed -n 's/.*programs\.git\.signing\.key = "\([^"]*\)".*/\1/p'` の 1 行分。貪欲な
/// 先頭 `.*` なので、同じ行に複数あれば最後の一致を採る。BSD grep(darwin)に -P が無い
/// ので bash 版も sed を使っていた。
fn signing_key_of_line(line: &str) -> Option<String> {
    const PAT: &str = "programs.git.signing.key = \"";
    let mut best = None;
    let mut start = 0;
    while let Some(i) = line[start..].find(PAT) {
        let at = start + i + PAT.len();
        if let Some(q) = line[at..].find('"') {
            best = Some(line[at..at + q].to_string());
        }
        start += i + 1;
    }
    best
}

/// GitHub の登録を現行 export で置き換える(DELETE → POST)。
fn fix_github(gh_json: &serde_json::Value, primary_short: &str, current_export: &str) {
    let old_id = gh_entry(gh_json, primary_short).and_then(|e| match &e["id"] {
        serde_json::Value::Null | serde_json::Value::Bool(false) => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    });
    if let Some(id) = old_id.filter(|s| !s.is_empty()) {
        let _ = Command::new("gh")
            .args(["api", "-X", "DELETE", &format!("user/gpg_keys/{id}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    // `jq -n --arg key "$current_export" '{armored_public_key: $key}'` と同じ JSON。
    let payload = format!(
        "{}\n",
        hook_io::jqfmt::J::obj(vec![(
            "armored_public_key",
            hook_io::jqfmt::J::str(current_export)
        )])
        .pretty()
    );
    let Ok(file) = TempFile::create(payload.as_bytes()) else {
        err("gpg-subkey sync: GitHub の GPG 登録更新に失敗しました");
        return;
    };
    let ok = Command::new("gh")
        .args(["api", "user/gpg_keys", "-X", "POST", "--input"])
        .arg(&file.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        out("gpg-subkey sync: GitHub の GPG 登録を更新しました");
    } else {
        err("gpg-subkey sync: GitHub の GPG 登録更新に失敗しました");
    }
}

// --- status / remind --------------------------------------------------------

/// [S]/[E] 両方の行。repo が空なら全 secret 鍵、あれば keys/*.pub の fingerprint に限る。
fn status_rows(cfg: &Cfg, repo: &str) -> Vec<Sub> {
    if repo.is_empty() {
        return ['s', 'e']
            .into_iter()
            .flat_map(|c| cfg.usage_subkeys(c, "", false))
            .collect();
    }
    let mut files: Vec<PathBuf> = fs::read_dir(format!("{repo}/keys"))
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "pub") && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut rows = Vec::new();
    for f in files {
        let listing = cfg.gpg_stdout(&["--with-colons", "--show-keys", &f.to_string_lossy()]);
        let Some(fpr) = colon::pubkey_fpr(&listing).filter(|x| !x.is_empty()) else {
            continue;
        };
        for c in ['s', 'e'] {
            rows.extend(cfg.usage_subkeys(c, &fpr, false));
        }
    }
    rows
}

/// gpg の生の capability 欄を ADR-0003 の括弧表記へ。
fn usage_label(cap: &str) -> String {
    if cap.contains('s') {
        "[S]".into()
    } else if cap.contains('e') {
        "[E]".into()
    } else {
        format!("[{cap}]")
    }
}

fn cmd_status(cfg: &Cfg, args: &[String]) -> i32 {
    let mut repo = String::new();
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--repo" => opt_val(args, &mut i, "status").map(|v| repo = v),
            other => {
                err(&format!("gpg-subkey status: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    let now = now_epoch();
    let rows: Vec<Sub> = status_rows(cfg, &repo)
        .into_iter()
        .filter(|r| !r.keyid.is_empty())
        .collect();
    if rows.is_empty() {
        out("gpg-subkey status: [S]/[E] subkey が見つかりません");
        return 0;
    }
    for r in rows {
        let label = usage_label(&r.cap);
        let head = format!(
            "{} ({}) sub={} {label}",
            r.primary_uid, r.primary_fpr, r.keyid
        );
        if r.revoked {
            out(&format!("{head}: revoked"));
        } else {
            match days_until(&r.expires, now) {
                None => out(&format!("{head}: 無期限")),
                Some(d) => out(&format!("{head}: 残り {d} 日")),
            }
        }
    }
    0
}

fn cmd_remind(cfg: &Cfg, args: &[String]) -> i32 {
    let mut threshold = DEFAULT_THRESHOLD;
    let mut notify = false;
    let mut i = 0;
    while i < args.len() {
        let r = match args[i].as_str() {
            "--threshold" => opt_val(args, &mut i, "remind").and_then(|v| match v.parse() {
                Ok(n) => {
                    threshold = n;
                    Ok(())
                }
                Err(_) => {
                    err(&format!(
                        "gpg-subkey remind: --threshold {v} は整数ではありません"
                    ));
                    Err(1)
                }
            }),
            "--notify" => {
                notify = true;
                i += 1;
                Ok(())
            }
            other => {
                err(&format!("gpg-subkey remind: 不明な引数 {other}"));
                return 2;
            }
        };
        if let Err(rc) = r {
            return rc;
        }
    }
    let now = now_epoch();
    let rows = ['s', 'e']
        .into_iter()
        .flat_map(|c| cfg.usage_subkeys(c, "", false));
    let mut due: Vec<String> = Vec::new();
    for r in rows {
        if r.keyid.is_empty() || r.revoked {
            continue;
        }
        let Some(days) = days_until(&r.expires, now) else {
            continue;
        };
        if days <= threshold {
            due.push(format!(
                "{} sub={} {} 残り{days}日で失効",
                r.primary_uid,
                r.keyid,
                usage_label(&r.cap)
            ));
        }
    }
    if due.is_empty() {
        out(&format!(
            "gpg-subkey remind: {threshold} 日以内に失効する [S]/[E] subkey はありません"
        ));
        return 0;
    }
    for line in &due {
        err(&format!("gpg-subkey remind: {line}"));
    }
    if notify {
        let body: String = due.join("\n").chars().take(230).collect();
        let _ = Command::new(&cfg.herdr_bin)
            .args([
                "notification",
                "show",
                "GPG [S]/[E] subkey の失効が近づいています",
                "--body",
                &body,
                "--sound",
                "request",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    1
}

// --- エントリポイント -------------------------------------------------------

/// 終了コードを返す。
pub fn run(args: &[String]) -> i32 {
    let cfg = Cfg::from_env();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let rest = args.get(1..).unwrap_or(&[]);
    match mode {
        "generate" => cmd_generate(&cfg, rest),
        "rotate" => cmd_rotate(&cfg, rest),
        "revoke" => cmd_revoke(&cfg, rest),
        "export" => cmd_export(&cfg, rest),
        "sync" => cmd_sync(&cfg, rest),
        "status" => cmd_status(&cfg, rest),
        "remind" => cmd_remind(&cfg, rest),
        "-h" | "--help" | "" => {
            let _ = write!(std::io::stdout(), "{USAGE}");
            if mode.is_empty() {
                1
            } else {
                0
            }
        }
        other => {
            err(&format!("gpg-subkey: 不明なコマンド {other}"));
            let _ = write!(std::io::stderr(), "{USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_extra_args_are_empty() {
        // 本番(env 未設定)経路の回帰: --selftest が両方 export する fixture 鍵では通らない
        // 分岐であり、実利用で壊れた分岐そのもの。
        let cfg = Cfg {
            gpg_bin: "gpg".into(),
            herdr_bin: "herdr".into(),
            pinentry_mode: None,
            passphrase: None,
        };
        assert!(cfg.edit_key_extra_args().is_empty());
        let cfg = Cfg {
            pinentry_mode: Some("loopback".into()),
            passphrase: Some(String::new()),
            ..cfg
        };
        assert_eq!(
            cfg.edit_key_extra_args(),
            vec!["--pinentry-mode", "loopback", "--passphrase", ""]
        );
    }

    #[test]
    fn days_until_ceils_and_handles_no_expiry() {
        assert_eq!(days_until("", 0), None);
        assert_eq!(days_until("0", 0), None);
        assert_eq!(days_until("86401", 0), Some(2));
        assert_eq!(days_until("86400", 0), Some(1));
    }

    #[test]
    fn signing_key_line() {
        assert_eq!(
            signing_key_of_line(r#"  programs.git.signing.key = "ABCD";"#).as_deref(),
            Some("ABCD")
        );
        assert_eq!(signing_key_of_line("nothing here"), None);
    }

    #[test]
    fn latest_keyid_ties_go_to_last_listed() {
        let mk = |k: &str, c: &str, rev: bool| Sub {
            primary_fpr: String::new(),
            primary_uid: String::new(),
            keyid: k.into(),
            subkey_fpr: String::new(),
            created: c.into(),
            expires: String::new(),
            revoked: rev,
            cap: "s".into(),
        };
        let rows = vec![
            mk("A", "100", false),
            mk("B", "100", false),
            mk("C", "200", true),
        ];
        assert_eq!(latest_keyid(&rows), "B");
        assert_eq!(latest_keyid(&[]), "");
    }
}
