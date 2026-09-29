//! pkexec-guard — agent セッションが `pkexec` で root 権限のコマンドを実行
//! するとき、polkit の認証ダイアログに全文が表示される範囲だけを通す
//! PreToolUse hook。設計と裁定の経緯は `docs/claude/pkexec-guard.md`
//! (PR #326 のグリルセッション、2026-09-28)。
//!
//! ## 採らなかった設計: SUDO_ASKPASS
//!
//! PR #326 は当初、`SUDO_ASKPASS` helper が pinentry に生の Assuan で
//! `GETPIN` を発行し、得たパスワードを stdout に平文で返す方式だった。
//! これは agent が読める同じ uid のパイプにパスワードを通す設計であり、
//! #348(pinentry への直接呼び出しが人間の操作なしに実パスワードを返した
//! 事故)→ #421(ADR-0003 Amendment 5: 同一 uid からの pinentry 直叩きは
//! gpg-agent 側では塞げない残余リスクと確定)を踏まえると、まさにその
//! 経路を正規の手順として使ってしまう。この hook は代わりに polkit を
//! 使う——パスワードは root 側の polkit-agent-helper-1 が検証するだけで
//! agent の出力には一切出ない。
//!
//! ## この hook が担う不変条件
//!
//! 1. **ダイアログに映った文字列 = 実際に root で動く argv。** pkexec(1)
//!    のダイアログは `cmdline_short`(80 文字を超えると中央を省略する、
//!    polkit-org/polkit `src/programs/pkexec.c` tag 124)を表示する。
//!    複合コマンド・展開・引用符・80 文字超はすべて deny する。
//! 2. **パスワードは同じ uid のパイプを通らない。** `sudo -A`/`-S`/
//!    `--askpass`/`--stdin`/`SUDO_ASKPASS` の再導入は agent を問わず deny
//!    する。
//! 3. **裸の `pkexec` は許さない。** `~/.local/bin`・`~/bin` は PATH の
//!    先頭にあり(`config/zsh/modules/10-path.zsh`)、ユーザーが書き込める
//!    場所に同名の偽 pkexec を置けてしまう。`pkexec` トークンは常に絶対
//!    パス `/usr/bin/pkexec` として書かれていることを要求し、その binary
//!    自身も root 所有・setuid・group/other 書き込み不可であることを実行
//!    時に確かめる。
//! 4. **root で実行してよい相手は閉じた許可リストのみ。** 対象パスと、
//!    引数中の絶対パスの各構成要素すべてが、root 所有かつ group/other
//!    書き込み不可であることも確かめる。
//! 5. **Codex / Copilot からは pkexec を一切許さない。** 明示的な ask
//!    ルール(`permissions.ask` の `Bash(/usr/bin/pkexec *)`)が auto mode
//!    でも確認を強制することを Claude Code 側で確認できているのは Claude
//!    のみ(公式ドキュメント参照、docs/claude/pkexec-guard.md)。二段承認
//!    が成立しない CLI では全 deny にする。

use hook_io::{shell, Agent, HookInput, PermissionDecision};
use std::path::Path;

/// 実行時に固定されている pkexec 自身の絶対パス。相対名 `pkexec` は許可
/// リストに乗らないよう常にこの定数と完全一致で照合する。
const PKEXEC_PATH: &str = "/usr/bin/pkexec";

/// root で実行してよい対象バイナリの閉じた許可リスト(絶対パス)。実装時に
/// このホスト(vega)で実在を確認済み。`tailscale` は `command -v tailscale`
/// が実装時に見つからず(このホストには未インストール)、実在しないパスを
/// 書かない方針(docs/claude/pkexec-guard.md)により今回は見送った——
/// 導入されたホストで実パスを確認したうえで、レビューを経て追加する。
const ALLOWED_TARGETS: &[&str] = &[
    "/usr/bin/apt-get",
    "/usr/bin/apt",
    "/usr/bin/dpkg",
    "/usr/bin/systemctl",
    "/usr/bin/journalctl",
    "/usr/bin/udevadm",
    "/usr/sbin/ufw",
];

/// pkexec のダイアログ(`cmdline_short`, `src/programs/pkexec.c` tag 124)が
/// 省略なしで全文を表示できる長さの上限。
const MAX_DISPLAY_LEN: usize = 80;

/// group / other 書き込みビットのマスク(0o020 | 0o002)。
const GROUP_OR_OTHER_WRITABLE: u32 = 0o022;

/// setuid ビット。
const SETUID_BIT: u32 = 0o4000;

/// ファイルの所有者(uid)とモードを問い合わせる面。実運用は
/// [`RealFs`]、テストはフェイクを注入する(`hook_io::input::HookInput::
/// project_dir_with` と同じ、実ファイルシステムに依存しないテストの
/// ための注入パターン)。
trait PathMeta {
    /// 存在しなければ `None`。
    fn uid(&self, path: &Path) -> Option<u32>;
    /// 存在しなければ `None`。
    fn mode(&self, path: &Path) -> Option<u32>;
}

struct RealFs;

// `symlink_metadata`(シンボリックリンクを辿らない)を意図的に使う。
// 許可リストの各バイナリは実装時点(2026-09-28、vega)でいずれも通常
// ファイルであることを確認済み(シンボリックリンクではない)。将来
// ディストリ側の変更でどれかがシンボリックリンクに変わった場合、
// symlink_metadata はリンク自身のモード(典型的には `lrwxrwxrwx`
// = group/other 書き込みビットが立って見える)を返すため、
// `is_locked_down_chain` は自動的に deny 側へ倒れる——「解決先が
// 安全か分からない」ときに黙って信頼せず fail closed にするための
// 意図的な選択であり、シンボリックリンクを正しく検証できないことの
// 見落としではない。
impl PathMeta for RealFs {
    fn uid(&self, path: &Path) -> Option<u32> {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).ok().map(|m| m.uid())
    }

    fn mode(&self, path: &Path) -> Option<u32> {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).ok().map(|m| m.mode())
    }
}

/// `path` から `/` まで祖先をたどり、実在する構成要素がすべて root 所有
/// かつ group/other 書き込み不可であることを確かめる。存在しない構成要素
/// (これから作られるファイルの親)は読み飛ばす。1 つも実在する構成要素が
/// 無ければ、何も検証できていないので `false`。
fn is_locked_down_chain(path: &Path, fs: &dyn PathMeta) -> bool {
    let mut checked_any = false;
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let (Some(uid), Some(mode)) = (fs.uid(ancestor), fs.mode(ancestor)) else {
            continue;
        };
        checked_any = true;
        if uid != 0 || mode & GROUP_OR_OTHER_WRITABLE != 0 {
            return false;
        }
    }
    checked_any
}

/// `/usr/bin/pkexec` 自身が本物(root 所有・setuid・group/other 書き込み
/// 不可、かつ祖先ディレクトリも同様)であることを確かめる。
fn pkexec_binary_is_genuine(fs: &dyn PathMeta) -> bool {
    let path = Path::new(PKEXEC_PATH);
    is_locked_down_chain(path, fs) && fs.mode(path).is_some_and(|m| m & SETUID_BIT != 0)
}

/// 引数語 1 つの「パス部分」を取り出す。`--opt=/path` 形式なら `=` の
/// 右側、それ以外は語全体。
fn path_portion(word: &str) -> Option<&str> {
    let candidate = word.split_once('=').map_or(word, |(_, v)| v);
    candidate.contains('/').then_some(candidate)
}

/// 許可された 1 コマンド `/usr/bin/pkexec <絶対パス> <引数…>` かどうかを
/// 検証する。満たさなければ deny 理由を返す。満たせば `Ok(())`——この
/// hook は allow/ask を一切出さない(deny-only)ので、`Ok(())` は「判定
/// しない(通常の permission フローに委ねる)」を意味する。
fn validate_pkexec(words: &[String], fs: &dyn PathMeta) -> Result<(), String> {
    if words.first().map(String::as_str) != Some(PKEXEC_PATH) {
        return Err(format!(
            "pkexec は絶対パス {PKEXEC_PATH} で書いてください(裸の `pkexec` は \
             PATH 先頭のユーザー書き込み可能なディレクトリで shadow されうるため deny)"
        ));
    }
    if !pkexec_binary_is_genuine(fs) {
        return Err(format!(
            "{PKEXEC_PATH} 自身が root 所有・setuid・group/other 書き込み不可の \
             条件を満たしていません(実行環境の異常、または偽装の疑い)"
        ));
    }

    let target = words
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| "pkexec に実行対象のコマンドが指定されていません".to_string())?;
    if target.starts_with('-') {
        return Err(
            "pkexec 自身のオプション(--user 等)は付けられません — 単純な \
             `/usr/bin/pkexec <絶対パス> <引数…>` のみ許可します"
                .to_string(),
        );
    }
    if !target.starts_with('/') {
        return Err(format!("実行対象は絶対パスで指定してください: {target}"));
    }
    if !ALLOWED_TARGETS.contains(&target) {
        return Err(format!(
            "{target} は pkexec の許可リストにありません(許可: {})",
            ALLOWED_TARGETS.join(", ")
        ));
    }
    if !is_locked_down_chain(Path::new(target), fs) {
        return Err(format!(
            "{target} またはその親ディレクトリが root 所有でない、または \
             group/other から書き込み可能です"
        ));
    }

    for arg in &words[2..] {
        let Some(p) = path_portion(arg) else {
            continue;
        };
        if !p.starts_with('/') {
            return Err(format!(
                "引数中のパスは絶対パスにしてください(相対パス経由での差し替えを \
                 防ぐため): {p}"
            ));
        }
        if !is_locked_down_chain(Path::new(p), fs) {
            return Err(format!(
                "引数のパス {p} の実在する構成要素のいずれかが root 所有でない、 \
                 または group/other から書き込み可能です"
            ));
        }
    }

    let display_len = words.iter().map(String::len).sum::<usize>() + words.len().saturating_sub(1);
    if display_len > MAX_DISPLAY_LEN {
        return Err(format!(
            "コマンド表示が {display_len} 文字あり、pkexec の認証ダイアログの \
             省略なし表示上限({MAX_DISPLAY_LEN} 文字)を超えています"
        ));
    }

    Ok(())
}

/// `sudo` の askpass/stdin 経路、または `SUDO_ASKPASS` 環境変数の再導入を
/// 検出する。#348/#421 で確定した「同じ uid のパイプにパスワードを通す
/// 経路は塞げない」という事実に基づき、この経路を静的解析の成否に関わらず
/// 常に deny する多層防御——主たる防御はこの仕組み自体を配備しないこと
/// (`SUDO_ASKPASS` helper をこのリポジトリに追加しない)であり、これは
/// その再導入を検出するだけの安全網(軸: 検出のみ)。
///
/// `SUDO_ASKPASS` の**単なる部分文字列**ではなく、実際に「環境変数として
/// 設定する」(`SUDO_ASKPASS=…`)か「展開して使う」(`$SUDO_ASKPASS`/
/// `${SUDO_ASKPASS}`)構文に見えるときだけ反応する——このドキュメント
/// 自身や commit メッセージのように、不採用にした設計を地の文で説明する
/// だけの箇所まで拾ってしまうと、この設計を書き残す作業そのものが
/// できなくなる(実装時に `git commit` の本文で実際に踏んだ回帰)。
fn sudo_askpass_reintroduction(cmd: &str) -> Option<&'static str> {
    if cmd.contains("SUDO_ASKPASS=")
        || cmd.contains("$SUDO_ASKPASS")
        || cmd.contains("${SUDO_ASKPASS")
    {
        return Some(
            "SUDO_ASKPASS の再導入は deny — パスワードを agent が読めるパイプに \
             通す設計は #348/#421 により不採用と裁定済み(docs/claude/pkexec-guard.md)",
        );
    }
    if let Some(words) = shell::split(cmd) {
        let has_sudo = words.iter().any(|w| w == "sudo");
        let has_askpass_flag = words
            .iter()
            .any(|w| matches!(w.as_str(), "-A" | "-S" | "--askpass" | "--stdin"));
        if has_sudo && has_askpass_flag {
            return Some(
                "sudo -A/-S/--askpass/--stdin の使用は deny — パスワードを agent が \
                 読めるパイプに通す設計は #348/#421 により不採用と裁定済み \
                 (docs/claude/pkexec-guard.md)",
            );
        }
    }
    None
}

/// `[A-Za-z0-9_-]` かどうか(この crate 自身の名前 `pkexec-guard` のように
/// ハイフンで続く識別子を「同じ語の一部」として扱うための定義)。
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// `cmd` 中に、独立した語としての `pkexec` が現れるか(前後が
/// [`is_word_byte`] でない、または文字列の端)。単純な部分一致
/// (`cmd.contains("pkexec")`)だと、この crate 自身のパス
/// (`crates/pkexec-guard/...`)に触れるだけの無関係なコマンドまで
/// 複合コマンド扱いで毎回 deny してしまう(実装時に自分自身の crate を
/// 検証するコマンドで実際に踏んだ回帰)。
fn contains_pkexec_word(cmd: &str) -> bool {
    let bytes = cmd.as_bytes();
    let needle = b"pkexec";
    let mut start = 0;
    while let Some(rel) = cmd[start..].find("pkexec") {
        let i = start + rel;
        let left_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        let right = i + needle.len();
        let right_ok = right == bytes.len() || !is_word_byte(bytes[right]);
        if left_ok && right_ok {
            return true;
        }
        start = i + needle.len();
    }
    false
}

/// `pkexec` の呼び出しに見える語を含むかどうか。
///
/// 解析できた(`words`)場合は、いずれかの語が `pkexec`/`PKEXEC_PATH` と
/// **完全一致**するときだけ「呼び出しに見える」と判定する——`test("pkexec")`
/// のような文字列引数(語全体は一致しないがハイフンを挟まず隣接する
/// ケース)まで拾ってしまわないようにするため、[`contains_pkexec_word`]
/// の語境界判定よりさらに厳密にしている。
///
/// 解析できなかった(複合コマンド等で `words` が無い)場合は、語単位の
/// 完全一致を取れないので、[`contains_pkexec_word`] の語境界一致まで
/// 許容範囲を広げる——`cd /tmp && pkexec ...` のような compound を fail
/// closed 側に倒す(軸: 検出のみ)ための安全網であり、精度より安全側に
/// 倒す。
fn mentions_pkexec(cmd: &str, words: Option<&[String]>) -> bool {
    match words {
        Some(words) => words.iter().any(|w| w == "pkexec" || w == PKEXEC_PATH),
        None => contains_pkexec_word(cmd),
    }
}

const CODEX_COPILOT_DENY_REASON: &str =
    "pkexec は Codex CLI / Copilot CLI からは常に deny — 明示的な ask \
     ルールが auto 系モードでも確認を強制することを確認できているのは \
     Claude Code のみ(docs/claude/pkexec-guard.md)";

/// #548: 複合コマンド(`;`/`&&`/`||`/`|`/`&`/改行区切り)を文単位で判定する。
/// 単一コマンド(`shell::split_segments` が1文しか返さない場合)は旧来と
/// 完全に同じ経路(`shell::split` + [`mentions_pkexec`] + [`validate_pkexec`])
/// を通し、挙動を一切変えない。
///
/// 複合コマンドでは、解析できた文(`words: Some`)の語配列に `pkexec`/
/// `PKEXEC_PATH` が完全一致で現れる文があれば「実際の呼び出しに見える文が
/// 複合コマンドの中にある」として deny する(不変条件1: polkit の
/// ダイアログは pkexec に渡された argv しか表示せず、同じ Bash 呼び出し内の
/// 他の文は承認の目に入らず実行される)。解析できない文(`words: None`)が
/// 語境界一致で `pkexec` を含む場合も、リダイレクト等で偽装された呼び出し
/// である可能性を排除できないため同様に deny する(軸: 検出のみ、安全側を
/// 維持)。緩めるのは、複合コマンドの**どの文にもこの2種の mention が
/// 一切無い**場合だけ——地の文の言及がクォート内に収まって正しく解析できた
/// 文の一部になっているケース(#548 の実際の誤検知)がこれに当たる。
fn check_with(input: &HookInput, agent: Agent, fs: &dyn PathMeta) -> Option<PermissionDecision> {
    let cmd = input.bash_command()?;

    if let Some(reason) = sudo_askpass_reintroduction(cmd) {
        return Some(PermissionDecision::deny(reason));
    }

    let segments = shell::split_segments(cmd);

    if segments.len() <= 1 {
        let words = shell::split(cmd);
        if !mentions_pkexec(cmd, words.as_deref()) {
            return None;
        }
        if agent != Agent::Claude {
            return Some(PermissionDecision::deny(CODEX_COPILOT_DENY_REASON));
        }
        let Some(words) = words else {
            return Some(PermissionDecision::deny(
                "pkexec を含むコマンドが複合コマンド・展開・引用符・改行を含むため \
                 静的に解析できません。解析できない場合は fail closed で deny します \
                 — 単純な `/usr/bin/pkexec <絶対パス> <引数…>` のみ許可します \
                 (docs/claude/pkexec-guard.md)",
            ));
        };
        return match validate_pkexec(&words, fs) {
            Ok(()) => None,
            Err(reason) => Some(PermissionDecision::deny(reason)),
        };
    }

    let any_exact_mention = segments.iter().any(|seg| {
        seg.words
            .as_deref()
            .is_some_and(|words| words.iter().any(|w| w == "pkexec" || w == PKEXEC_PATH))
    });
    let any_fuzzy_mention = segments
        .iter()
        .any(|seg| seg.words.is_none() && contains_pkexec_word(&seg.text));

    if !any_exact_mention && !any_fuzzy_mention {
        return None;
    }

    if agent != Agent::Claude {
        return Some(PermissionDecision::deny(CODEX_COPILOT_DENY_REASON));
    }

    if any_exact_mention {
        return Some(PermissionDecision::deny(
            "複合コマンドの中に pkexec の呼び出しに見える文が含まれています。 \
             polkit の認証ダイアログは pkexec に渡された argv しか表示しないため、 \
             同じ Bash 呼び出し内の他の文が承認の目に入らず実行されます — deny \
             (docs/claude/pkexec-guard.md)。単独の Bash 呼び出しに分離してください。",
        ));
    }

    Some(PermissionDecision::deny(
        "複合コマンドの中に、pkexec を含むが静的に解析できない文(展開・ \
         リダイレクト等)があります。リダイレクトで偽装された呼び出しの \
         可能性を排除できないため fail closed で deny します \
         (docs/claude/pkexec-guard.md)。単独の Bash 呼び出しに分離してください。",
    ))
}

/// PreToolUse: deny するなら理由付きの判定を返す。判定しない(deny しない)
/// なら `None`——この hook は allow/ask を一切出さない(deny-only)。承認
/// された `pkexec` コマンドは、`permissions.ask` の `Bash(/usr/bin/pkexec *)`
/// ルールに委ねて改めて人間の確認を取る。
pub fn check(input: &HookInput, agent: Agent) -> Option<PermissionDecision> {
    check_with(input, agent, &RealFs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    struct FakeFs(HashMap<&'static str, (u32, u32)>);

    impl FakeFs {
        fn new() -> Self {
            let mut m = HashMap::new();
            // 素の root ロックダウン済みツリー: / , /usr , /usr/bin
            for d in ["/", "/usr", "/usr/bin", "/usr/sbin", "/etc"] {
                m.insert(d, (0, 0o755));
            }
            // pkexec 自身: root + setuid + 0755
            m.insert(PKEXEC_PATH, (0, 0o755 | SETUID_BIT));
            for t in ALLOWED_TARGETS {
                m.insert(*t, (0, 0o755));
            }
            FakeFs(m)
        }

        fn with(mut self, path: &'static str, uid: u32, mode: u32) -> Self {
            self.0.insert(path, (uid, mode));
            self
        }
    }

    impl PathMeta for FakeFs {
        fn uid(&self, path: &Path) -> Option<u32> {
            self.0.get(path.to_str()?).map(|(u, _)| *u)
        }
        fn mode(&self, path: &Path) -> Option<u32> {
            self.0.get(path.to_str()?).map(|(_, m)| *m)
        }
    }

    fn input(cmd: &str) -> HookInput {
        let v = json!({
            "hook_event_name": "PreToolUse",
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": cmd},
        });
        HookInput::parse(&v.to_string()).unwrap()
    }

    fn check_claude(cmd: &str, fs: &dyn PathMeta) -> Option<PermissionDecision> {
        check_with(&input(cmd), Agent::Claude, fs)
    }

    #[test]
    fn allows_wellformed_allowlisted_command() {
        let fs = FakeFs::new();
        assert!(check_claude("/usr/bin/pkexec /usr/bin/systemctl --version", &fs).is_none());
        assert!(check_claude("/usr/bin/pkexec /usr/bin/apt-get update", &fs).is_none());
    }

    #[test]
    fn denies_bare_pkexec_even_if_first_word() {
        // R1-A-1(Copilot プランレビュー): 裸の pkexec は PATH 先頭の
        // ユーザー書き込み可能ディレクトリで shadow されうるため、絶対パス
        // 以外は常に deny。
        let fs = FakeFs::new();
        assert!(check_claude("pkexec /usr/bin/systemctl --version", &fs).is_some());
    }

    #[test]
    fn denies_pkexec_own_options() {
        let fs = FakeFs::new();
        assert!(check_claude("/usr/bin/pkexec --user root /usr/bin/apt", &fs).is_some());
    }

    #[test]
    fn denies_relative_or_non_allowlisted_target() {
        let fs = FakeFs::new();
        assert!(check_claude("/usr/bin/pkexec systemctl --version", &fs).is_some());
        assert!(check_claude("/usr/bin/pkexec /usr/bin/bash -c true", &fs).is_some());
        assert!(check_claude("/usr/bin/pkexec /usr/bin/true", &fs).is_some());
    }

    #[test]
    fn denies_when_target_path_is_writable_or_unowned() {
        let fs = FakeFs::new().with("/usr/bin/apt-get", 1000, 0o755);
        assert!(check_claude("/usr/bin/pkexec /usr/bin/apt-get update", &fs).is_some());

        let fs = FakeFs::new().with("/usr/bin/apt-get", 0, 0o775);
        assert!(check_claude("/usr/bin/pkexec /usr/bin/apt-get update", &fs).is_some());
    }

    #[test]
    fn denies_when_ancestor_directory_is_writable() {
        let fs = FakeFs::new().with("/usr/bin", 0, 0o777);
        assert!(check_claude("/usr/bin/pkexec /usr/bin/systemctl --version", &fs).is_some());
    }

    #[test]
    fn denies_when_pkexec_binary_itself_is_not_genuine() {
        let fs = FakeFs::new().with(PKEXEC_PATH, 0, 0o755); // setuid ビットなし
        assert!(check_claude("/usr/bin/pkexec /usr/bin/systemctl --version", &fs).is_some());
    }

    #[test]
    fn denies_relative_path_argument() {
        let fs = FakeFs::new();
        assert!(check_claude("/usr/bin/pkexec /usr/bin/dpkg -i ./x.deb", &fs).is_some());
        assert!(check_claude("/usr/bin/pkexec /usr/bin/dpkg -i ~/x.deb", &fs).is_some());
    }

    #[test]
    fn allows_absolute_path_argument_in_locked_down_dir() {
        let fs = FakeFs::new()
            .with("/var", 0, 0o755)
            .with("/var/cache", 0, 0o755);
        assert!(check_claude("/usr/bin/pkexec /usr/bin/dpkg -i /var/cache/x.deb", &fs).is_none());
    }

    #[test]
    fn denies_absolute_path_argument_under_writable_dir() {
        let fs = FakeFs::new()
            .with("/var", 0, 0o755)
            .with("/var/cache", 0, 0o777);
        assert!(check_claude("/usr/bin/pkexec /usr/bin/dpkg -i /var/cache/x.deb", &fs).is_some());
    }

    #[test]
    fn allows_flag_equals_form_with_safe_path() {
        let fs = FakeFs::new().with("/etc/foo", 0, 0o644);
        assert!(check_claude(
            "/usr/bin/pkexec /usr/bin/systemctl --root=/etc/foo status",
            &fs
        )
        .is_none());
    }

    #[test]
    fn denies_command_exceeding_display_length() {
        let fs = FakeFs::new();
        let long_arg = "-".to_string() + &"x".repeat(80);
        let cmd = format!("/usr/bin/pkexec /usr/bin/systemctl {long_arg}");
        assert!(cmd.len() > 80);
        assert!(check_claude(&cmd, &fs).is_some());
    }

    #[test]
    fn allows_command_at_exact_length_limit() {
        let fs = FakeFs::new();
        // "/usr/bin/pkexec /usr/bin/systemctl " は 36 文字、残り 44 文字を
        // ちょうど 80 文字に収める安全な引数で埋める。
        let base = "/usr/bin/pkexec /usr/bin/systemctl ";
        let filler = "a".repeat(80 - base.len());
        let cmd = format!("{base}{filler}");
        assert_eq!(cmd.len(), 80);
        assert!(check_claude(&cmd, &fs).is_none());
    }

    #[test]
    fn denies_compound_commands_and_expansions() {
        let fs = FakeFs::new();
        for c in [
            "/usr/bin/pkexec /usr/bin/systemctl status && rm -rf /",
            "/usr/bin/pkexec /usr/bin/systemctl status; echo done",
            "/usr/bin/pkexec /usr/bin/systemctl \"$(echo status)\"",
            "echo pkexec | cat",
        ] {
            assert!(check_claude(c, &fs).is_some(), "{c:?}");
        }
    }

    #[test]
    fn allows_quoted_argument_without_expansion() {
        // `hook_io::shell::split` はクォートを展開なしの語に正しく解決する
        // (`'status'` → `status`)。pkexec のダイアログは実行時の実際の argv
        // (クォート解決済み)を表示するので、判定もその argv に対して行う —
        // クォートの有無そのものを deny 理由にはしない(Plan 時点では
        // 「引用符を含めば deny」を想定していたが、実装時に shell::split が
        // 既にクォート内の展開を拒否しつつ非展開クォートは正しく解決する
        // ことを確認し、この形に変更した)。
        let fs = FakeFs::new();
        assert!(check_claude("/usr/bin/pkexec /usr/bin/systemctl 'status'", &fs).is_none());
        assert!(check_claude("/usr/bin/pkexec /usr/bin/systemctl \"status\"", &fs).is_none());
    }

    #[test]
    fn allows_pkexec_mention_fully_inside_a_parseable_quoted_segment() {
        // #548 で仕様変更: クォート内の "pkexec" は、その文自体が(展開・
        // リダイレクト無しで)正しく解析できるなら、複合コマンドであっても
        // fail closed にしない——引用符全体が1つの語になり "pkexec" 単体とは
        // 完全一致しないため、実際の呼び出しに見える文が無いと判定できる。
        // (旧テスト名 denies_pkexec_mention_inside_unrelated_but_unparseable_
        // command。実装時に shell::split_segments を導入した結果、この文は
        // そもそも「解析不能」ではなく「解析できたが完全一致しない」に
        // 分類が変わったため、期待値も allow に反転した。)
        let fs = FakeFs::new();
        assert!(check_claude("echo 'never call pkexec' && true", &fs).is_none());
    }

    #[test]
    fn still_denies_when_the_unparseable_segment_is_the_only_one_in_a_compound() {
        // any_fuzzy のみ(解析できた文には mention 無し、解析できない文が
        // 語境界一致で pkexec を含む)場合は、リダイレクト等で偽装された
        // 呼び出しを排除できないため、依然 fail closed で deny する。
        let fs = FakeFs::new();
        assert!(check_claude("echo hi; /usr/bin/pkexec>file", &fs).is_some());
    }

    #[test]
    fn allows_compound_when_no_segment_mentions_pkexec_at_all() {
        let fs = FakeFs::new();
        assert!(check_claude("echo a; echo b && echo c", &fs).is_none());
    }

    #[test]
    fn still_denies_compound_with_a_genuine_pkexec_invocation_segment() {
        // #548 は誤検知の解消であり、不変条件1(複合コマンド中の実際の
        // pkexec 呼び出しはダイアログの目に入らない他の文を隠しうる)は
        // 変えない——validate_pkexec が通る文でも、複合の中にある限り deny。
        let fs = FakeFs::new();
        assert!(
            check_claude("/usr/bin/pkexec /usr/bin/systemctl status; echo done", &fs).is_some()
        );
    }

    #[test]
    fn ignores_commands_without_pkexec_mention() {
        let fs = FakeFs::new();
        assert!(check_claude("git status", &fs).is_none());
        assert!(check_claude("echo hello && rm -rf /", &fs).is_none());
    }

    #[test]
    fn denies_pkexec_from_codex_and_copilot_regardless_of_shape() {
        let fs = FakeFs::new();
        for agent in [Agent::Codex, Agent::Copilot] {
            let d = check_with(
                &input("/usr/bin/pkexec /usr/bin/systemctl --version"),
                agent,
                &fs,
            );
            assert!(d.is_some(), "{agent:?}");
        }
    }

    #[test]
    fn denies_sudo_askpass_reintroduction_regardless_of_agent() {
        let fs = FakeFs::new();
        for agent in [Agent::Claude, Agent::Codex, Agent::Copilot] {
            assert!(check_with(&input("echo $SUDO_ASKPASS"), agent, &fs).is_some());
            assert!(check_with(&input("sudo -A whoami"), agent, &fs).is_some());
            assert!(check_with(&input("sudo --askpass whoami"), agent, &fs).is_some());
            assert!(check_with(&input("sudo -S whoami"), agent, &fs).is_some());
            assert!(check_with(&input("sudo --stdin whoami"), agent, &fs).is_some());
        }
    }

    #[test]
    fn allows_plain_sudo_without_askpass_flags() {
        // sudo 自体は本 guard の対象外(tty のある対話端末では従来どおり
        // sudo のプロンプトが機能する) — askpass/stdin 系フラグの再導入
        // だけを狙い撃ちする。
        let fs = FakeFs::new();
        assert!(check_claude("sudo whoami", &fs).is_none());
    }

    #[test]
    fn allows_prose_mention_of_sudo_askpass_without_actual_usage_syntax() {
        // 実機(vega、`hms .` で適用したセッション)で `git commit` の本文
        // (このドキュメントを書き残す commit メッセージ)が "SUDO_ASKPASS"
        // という語を含むだけで deny されてしまった回帰。環境変数としての
        // 代入(`=`)・展開(`$`)構文を伴わない地の文の言及は対象にしない。
        let fs = FakeFs::new();
        assert!(check_claude(
            "echo 'SUDO_ASKPASS の再導入は deny — #348/#421 により不採用'",
            &fs
        )
        .is_none());
        assert!(check_claude("grep -rn SUDO_ASKPASS docs/", &fs).is_none());
    }

    #[test]
    fn ignores_non_bash_tools_and_empty_commands() {
        let fs = FakeFs::new();
        let i = HookInput::parse(
            &json!({"tool_name": "Read", "tool_input": {"file_path": "x"}}).to_string(),
        )
        .unwrap();
        assert!(check_with(&i, Agent::Claude, &fs).is_none());
    }

    // 実装中に実機(vega、`hms .` で適用したセッション)で実際に踏んだ2つの
    // 誤検知の回帰テスト。どちらも「'pkexec' の部分一致」を検出条件にして
    // いたことが原因で、この crate 自身に触れる無関係なコマンドまで
    // 毎回 deny してしまっていた。

    #[test]
    fn allows_parseable_command_mentioning_pkexec_only_as_a_substring() {
        // 単純コマンド(複合演算子なし)なら、語単位の完全一致だけを見る。
        // grep/jq のパターン文字列内の "pkexec" や、この crate 自身のパス
        // (crates/pkexec-guard/...)はどちらも exact word match しない。
        let fs = FakeFs::new();
        assert!(check_claude(
            "jq -r '.command | test(\"pkexec\")' crates/pkexec-guard/src/lib.rs",
            &fs
        )
        .is_none());
        assert!(check_claude("wc -l crates/pkexec-guard/src/lib.rs", &fs).is_none());
        assert!(check_claude("cargo test -p pkexec-guard", &fs).is_none());
    }

    #[test]
    fn denies_unparseable_command_only_when_pkexec_is_a_standalone_word() {
        // 複合コマンド(split できない)でも、"pkexec-guard" のようにハイフンで
        // 続く語は "pkexec" 単体の出現とは扱わない——crate 名を含むだけの
        // 無関係なパイプ済みコマンドまで fail closed にしないため。
        let fs = FakeFs::new();
        assert!(check_claude("cargo test -p pkexec-guard 2>&1 | tail -30", &fs).is_none());
        assert!(check_claude("echo crates/pkexec-guard/src/lib.rs | cat", &fs).is_none());
        // 一方、"pkexec" が独立した語として現れる compound は引き続き
        // fail closed で deny する(安全網、軸: 検出のみ)。
        assert!(check_claude("cd /tmp && pkexec /usr/bin/apt-get update", &fs).is_some());
    }
}
