//! hms — home-manager switch、正準の適用手順(docs/operations.md)。bash 版
//! `scripts/hms.sh` の移植(ADR-0024 / #389 Stage 4e、#414)。
//!
//! 使い方: `hms [flake-ref] [--public-only]`
//!
//! ```text
//! hms          pushed main(github:tarotene/dotfiles)を適用。private wrapper
//!              flake が登録されていればそちらを適用(ADR-0034)
//! hms .        現在の checkout/worktree を適用(push 前の検証)。wrapper 登録
//!              ホストでは wrapper 経由で `dotfiles` をこの checkout に
//!              override して適用する(ADR-0034 Amendment 2026-09-27)
//! hms <path>   任意のローカル checkout(`hms .` と同じ経路)
//! hms . --public-only
//!              wrapper ホストでも checkout 単体を適用する
//! ```
//!
//! シェル状態(cd・export)を呼び出し元に残す処理は無い(`~/.local/bin/hms` に
//! 配備された独立の実行ファイルで、zsh 関数ではない)ので、全体を Rust に移せる。
//! bash 版の `--selftest` は無い(純関数の単体テスト + スタブ PATH の統合テストに移した)。
//!
//! 1 コマンド = 適用 runbook 全体:
//!
//! 1. home-manager switch --flake `<ref>`#`<host>` -b backup
//! 2. systemctl --user daemon-reload
//! 3. 生成された fcitx5 autostart unit を restart — switch で ExecStart の store
//!    path は動くが、daemon-reload だけでは generated unit は再起動されず旧バイナリが
//!    動き続ける
//! 4. restart 後に unit が active で MainPID が生きていることを検証
//!
//! 既定 ref はリモート main(どのローカル checkout のブランチ・dirty 状態にも依存
//! しない)。worktree の適用は明示の `hms .` だけ。
//!
//! リモート適用(`hms`)は、適用対象 flake が pin する bleep(`flake = false` の
//! source tree 入力)も main の最新に追従させる: `nix flake prefetch --refresh` で
//! main の revision を解決し、lock の pin と違えば `--override-input bleep …`(wrapper
//! 経由は `dotfiles/bleep`)を付けて **事前に `nix build` で activationPackage を
//! 組む**。配備された hook の canary(home.checks)を通れば override を採用し、通ら
//! なければ(bleep 側の破壊的変更に dotfiles の配線が未追従)警告して lock の pin の
//! まま適用する。`hms .` / `hms <path>` は checkout の pin をそのまま検証する場なので
//! override しない。
//!
//! nix は github: 形式の flake ref の解決を tarball-ttl(既定 1h)キャッシュする。
//! マージ直後に `hms` が 1 時間前の main を黙って適用して "Done." を出す(#48)ので、
//! 非ローカル ref は switch 前に `--refresh` し、実際に適用する revision を表示して
//! 古い適用が痕跡なしで終わらないようにする。
//!
//! `hms .` / `hms <path>` が wrapper 登録ホストで PUBLIC checkout 単体を適用すると、
//! wrapper が足す private value module(bleep 自身の denylist config=orgs.txt/
//! repos.txt を含む、ADR-0034 Amendment 2026-09-24)を無警告で落とし、安全機構を
//! 切ってしまう。実際に起きた(2026-09-27、直前の `hms` が再生成した bleep config を
//! 手動 `hms .` が落とし、欠けた orgs.txt を見たエージェントが古い backup から手で
//! 復元した)ため、`local_apply_plan` が既定で wrapper 経由にする:
//!
//! ```text
//! home-manager switch --flake <private-hub-ref>#`<host>` \
//!   --override-input dotfiles path:<abs-path> --no-write-lock-file -b backup
//! ```
//!
//! `--public-only` で従来の checkout 単体適用に戻せる。
//!
//! wrapper の flake.lock が `dotfiles` を古い rev に pin している問題(#48 と同形)は
//! 1 層下にもある: #48 の refresh は wrapper 自身の ref しか再解決せず、lock 内の
//! `dotfiles` input は触らない。適用対象 flake の lock が `dotfiles` input を持つ
//! (wrapper、リモートでもローカルでも)なら、lock を信用せず pushed main の解決済み
//! revision で override する(ADR-0034 Amendment 2026-09-25)。
//!
//! 上の 2 経路はどちらも `--override-input` を渡す。nix 自身の `--help` どおりこれは
//! `--no-write-lock-file` を含意し、毎回 `warning: not writing modified lock file of
//! flake '<ref>':` と input の差分を出す — override が効いている痕跡で異常ではなく、
//! この警告だけを消す nix のフラグは無い。`lock_warning_expected` がこの場合を検出し、
//! nix の出力の前にその旨を言う。
//!
//! 適用は同時に 1 つだけ(排他ロック `~/.local/state/hms/lock`)。別の worktree や
//! セッションが同じユーザー環境へ同時に switch すると、先の適用が後の適用に戻される
//! (#1025)。2 つ目の `hms` は先の適用が終わるまで待つ。ロックは `File::lock`
//! (flock)なので、hms が落ちても OS が解放する。
//!
//! switch が成功し、世代の照合(`verify`)も通ったら、その世代をどの経路で作ったかを
//! `~/.local/state/hms/last-apply.json` に記録する(`ApplyMode`)。後から世代の rev だけで
//! 経路を推測しない — squash-merge では、ブランチのコミットが main に無いことからしか
//! 検証用の世代と分からない。

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

mod verify;

pub const DEFAULT_REF: &str = "github:tarotene/dotfiles";
pub const FCITX5_UNIT: &str = "app-fcitx5@autostart.service";

// ---------------------------------------------------------------------------
// 純関数(I/O なし。bash 版 selftest の 16 ケースは tests モジュール)
// ---------------------------------------------------------------------------

/// `local_apply_plan` の結果。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ApplyPlan {
    /// wrapper 経由で `dotfiles` をローカルパスに override して適用する。
    Route,
    /// 与えられた ref をそのまま適用する(従来の挙動)。
    AsIs,
}

/// ローカルパスの適用を、そのパス単体ではなく登録済み wrapper flake 経由にするか。
///
/// - `wrapper_ref`: resolve_default_ref の結果
/// - `default_ref`: DEFAULT_REF("wrapper 未登録"の検出用)
/// - `ref_is_local`: 適用する ref がディスク上のパスか
/// - `ref_is_wrapper`: 適用する ref 自身が wrapper flake か(flake.lock が `dotfiles` input
///   を pin している=既存の wrapper-lock override 経路が扱う)
/// - `public_only`: `--public-only` が渡されたか
pub fn local_apply_plan(
    wrapper_ref: &str,
    default_ref: &str,
    ref_is_local: bool,
    ref_is_wrapper: bool,
    public_only: bool,
) -> ApplyPlan {
    if public_only {
        return ApplyPlan::AsIs;
    }
    if wrapper_ref == default_ref {
        return ApplyPlan::AsIs; // このホストに wrapper は登録されていない
    }
    if !ref_is_local {
        return ApplyPlan::AsIs; // リモート ref は既に wrapper 経由(か明示指定)
    }
    if ref_is_wrapper {
        return ApplyPlan::AsIs; // 明示的な wrapper checkout — 既存の lock-override 経路が扱う
    }
    ApplyPlan::Route
}

/// これから走らせる `home-manager switch` が `--override-input` を持ち、nix が
/// "not writing modified lock file" 警告を出すはずか。
pub fn lock_warning_expected<S: AsRef<str>>(extra_opts: &[S]) -> bool {
    extra_opts.iter().any(|a| a.as_ref() == "--override-input")
}

/// jq の `-r '… // empty'`: null / false / 欠如は空文字、文字列はそのまま、
/// それ以外は JSON 表記。
fn jq_r_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `nix flake metadata --json` の `.revision // empty`。
pub fn metadata_revision(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .map(|v| jq_r_or_empty(v.get("revision")))
        .unwrap_or_default()
}

/// `nix flake metadata --json` の文書から、ルート flake の `dotfiles` input が
/// 素の(follows でない)input — つまり適用対象が ADR-0034 の private wrapper flake —
/// のとき、その input が現在 pin されている locked rev を返す。`dotfiles` input が
/// 無い、または `follows` チェーン(ノード名の文字列でなく JSON 配列)のときは空文字:
/// どちらも wrapper 自身の直接 pin ではないので override する対象が無い。
pub fn wrapper_locked_dotfiles_rev(json: &str) -> String {
    let Ok(doc) = serde_json::from_str::<Value>(json) else {
        return String::new();
    };
    let nodes = &doc["locks"]["nodes"];
    match nodes["root"]["inputs"].get("dotfiles") {
        Some(Value::String(node)) => jq_r_or_empty(nodes[node.as_str()]["locked"].get("rev")),
        _ => String::new(),
    }
}

/// `dotfiles` input を wrapper 自身の lock ではなく pushed main の解決済み revision
/// に pin する `home-manager switch` 引数。revision が空なら何も返さない
/// (呼び出し側は revision を解決できたときだけこれに来る)。
pub fn dotfiles_override_opts(main_rev: &str) -> Vec<String> {
    if main_rev.is_empty() {
        return Vec::new();
    }
    vec![
        "--override-input".into(),
        "dotfiles".into(),
        format!("github:tarotene/dotfiles/{main_rev}"),
        "--no-write-lock-file".into(),
    ]
}

pub const BLEEP_REF: &str = "github:tarotene/bleep";

/// 適用対象 flake の lock が pin している bleep(`flake = false` の source tree 入力)。
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BleepPin {
    /// `--override-input` に渡す入力名。public 単体は `bleep`、wrapper 経由は
    /// `dotfiles/bleep`(wrapper が `dotfiles` を入力に持ち、その先に bleep がある)。
    pub input: String,
    pub rev: String,
}

/// `nix flake metadata --json` の lock から bleep の pin を読む。ルート直下の `bleep`
/// を先に、無ければ `dotfiles` ノード配下の `bleep` を見る。どちらも素のノード参照
/// (文字列)でないとき(`follows` の配列・欠如)は None — override する対象が無い。
pub fn bleep_pin(json: &str) -> Option<BleepPin> {
    let doc: Value = serde_json::from_str(json).ok()?;
    let nodes = &doc["locks"]["nodes"];
    let rev_of = |node: &str| -> Option<String> {
        let r = jq_r_or_empty(nodes[node]["locked"].get("rev"));
        (!r.is_empty()).then_some(r)
    };
    if let Some(Value::String(node)) = nodes["root"]["inputs"].get("bleep") {
        return rev_of(node).map(|rev| BleepPin {
            input: "bleep".into(),
            rev,
        });
    }
    if let Some(Value::String(dot)) = nodes["root"]["inputs"].get("dotfiles") {
        if let Some(Value::String(node)) = nodes[dot.as_str()]["inputs"].get("bleep") {
            return rev_of(node).map(|rev| BleepPin {
                input: "dotfiles/bleep".into(),
                rev,
            });
        }
    }
    None
}

/// `nix flake prefetch --json` の `.locked.rev`(`nix flake metadata` は flake.nix の
/// 無い bleep では失敗するので、source tree 入力の main の解決にはこちらを使う)。
pub fn prefetch_revision(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .map(|v| jq_r_or_empty(v["locked"].get("rev")))
        .unwrap_or_default()
}

/// bleep を main の解決済み revision に pin する `home-manager switch` / `nix build` 引数。
pub fn bleep_override_opts(input: &str, main_rev: &str) -> Vec<String> {
    vec![
        "--override-input".into(),
        input.into(),
        format!("{BLEEP_REF}/{main_rev}"),
        "--no-write-lock-file".into(),
    ]
}

/// 今回の適用が、直前の generation が既に deploy していた wrapper の private value
/// module を無警告で落とすものか(#567)。true なら中止。
///
/// - `had_marker_prev_gen`: 直前の generation の home-files が
///   `.config/dotfiles/private-hub` を deploy していたか(=wrapper 経由で適用済み、
///   home/modules/private-hub.nix)
/// - `wrapper_ref`: 今回の resolve_default_ref の結果
/// - `default_ref`: DEFAULT_REF(「今は wrapper 未登録」の検出用)
/// - `public_only`: `--public-only` が渡されたか
pub fn private_hub_downgrade_abort(
    had_marker_prev_gen: bool,
    wrapper_ref: &str,
    default_ref: &str,
    public_only: bool,
) -> bool {
    if public_only {
        return false; // 明示的な opt-out — 呼び出し側は分かってやっている
    }
    if wrapper_ref != default_ref {
        return false; // 今回も wrapper 登録済み — 降格ではない
    }
    had_marker_prev_gen // 前世代で登録済み、今回未登録 → 無警告の降格
}

/// 世代をどの経路で適用したか(`last-apply.json` の `mode`)。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ApplyMode {
    /// 引数なしの `hms`(pushed main、wrapper 登録ホストでは wrapper 経由の main)。
    Default,
    /// 引数で ref を渡した、または `--public-only` を付けた適用(`hms .`、
    /// `hms <path>`、`hms github:…/<branch または rev>`)。検証用の世代として扱う。
    Explicit,
}

impl ApplyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ApplyMode::Default => "default",
            ApplyMode::Explicit => "explicit",
        }
    }
}

/// 適用の経路を、引数から決める。
///
/// ローカルかリモートかでは決めない(`ref_is_local` はパスが存在するかだけの判定)。
/// `hms github:tarotene/dotfiles/<branch>` もリモートだが main ではないので、
/// 明示した ref はすべて `Explicit` にする。`--public-only` は wrapper が足す private
/// value module を意図して落とす適用なので、ref が無くても `Explicit`。
///
/// - `ref_given`: 位置引数で ref を渡したか
/// - `public_only`: `--public-only` が渡されたか
pub fn apply_mode(ref_given: bool, public_only: bool) -> ApplyMode {
    if ref_given || public_only {
        ApplyMode::Explicit
    } else {
        ApplyMode::Default
    }
}

/// `last-apply.json` の内容(整形済みの JSON 1 件、末尾改行つき)。
///
/// - `rev`: 適用した世代の dotfiles の rev(`generation_rev`。読めなければ null)
/// - `generation`: 適用後の profile の指す store path
/// - `applied_at`: 適用を終えた時刻(unix 秒)
pub fn apply_record_json(
    mode: ApplyMode,
    rev: Option<&str>,
    generation: &str,
    applied_at: u64,
) -> String {
    let mut text = serde_json::to_string_pretty(&serde_json::json!({
        "mode": mode.as_str(),
        "rev": rev,
        "generation": generation,
        "applied_at": applied_at,
    }))
    .expect("a json! object always serializes");
    text.push('\n');
    text
}

/// `last-apply.json` を読んだもの(`apply_record_json` の逆)。
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ApplyRecord {
    pub mode: ApplyMode,
    pub rev: Option<String>,
    pub generation: String,
}

/// `last-apply.json` を読む。壊れている・`mode` が未知の値のときは `None`
/// (記録が無いのと同じ扱い — 経路が分からない世代には追従しない)。
pub fn parse_apply_record(json: &str) -> Option<ApplyRecord> {
    let v: Value = serde_json::from_str(json).ok()?;
    let mode = match v.get("mode")?.as_str()? {
        "default" => ApplyMode::Default,
        "explicit" => ApplyMode::Explicit,
        _ => return None,
    };
    Some(ApplyRecord {
        mode,
        rev: v.get("rev").and_then(Value::as_str).map(str::to_string),
        generation: v.get("generation")?.as_str()?.to_string(),
    })
}

/// 自動追従(`--follow`)が main へ追従しない理由。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StopReason {
    /// この世代に適用の記録が無い(新しい hms で当てる前の世代)。
    NoRecord,
    /// 記録の世代といまの世代が違う(rollback や、hms を通さない switch)。
    ForeignGeneration,
    /// `hms .` など、ref を明示して当てた検証用の世代。
    ExplicitGeneration,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FollowAction {
    /// 何もしない(main を解決できない・追従済み・前回失敗したコミット)。
    Idle,
    /// 追従しない理由を知らせる。
    Stop(StopReason),
    /// main へ追従する(許可ポリシーの確認は、この後の switch の直前)。
    Apply,
}

/// `--follow` の判断に使う事実。
pub struct FollowFacts<'a> {
    /// pushed main の先頭(解決できなければ `None` — オフライン)。
    pub main_rev: Option<&'a str>,
    /// いまの世代の dotfiles の rev(`~/.local/share/dotfiles/rev`)。
    pub current_rev: Option<&'a str>,
    /// いまの profile が指す store path(無ければ空)。
    pub current_generation: &'a str,
    pub record: Option<&'a ApplyRecord>,
    /// 前回の自動追従が失敗した main のコミット。
    pub failed_rev: Option<&'a str>,
}

/// main へ追従するか、止めるか、何もしないかを決める。
///
/// 追従済みかを先に見る: 検証用の世代でも、たまたま main の先頭と同じなら、止める
/// 理由を知らせる必要は無い。追従するのは、記録の世代がいまの世代と一致し、かつ
/// 引数なしの `hms` が作った世代(`ApplyMode::Default`)だけ。
pub fn follow_decision(f: &FollowFacts) -> FollowAction {
    let Some(main_rev) = f.main_rev else {
        return FollowAction::Idle;
    };
    if f.current_rev == Some(main_rev) {
        return FollowAction::Idle;
    }
    let Some(record) = f.record else {
        return FollowAction::Stop(StopReason::NoRecord);
    };
    if record.generation.is_empty() || record.generation != f.current_generation {
        return FollowAction::Stop(StopReason::ForeignGeneration);
    }
    if record.mode == ApplyMode::Explicit {
        return FollowAction::Stop(StopReason::ExplicitGeneration);
    }
    if f.failed_rev == Some(main_rev) {
        return FollowAction::Idle;
    }
    FollowAction::Apply
}

/// 本人へ知らせること。同じ `key` は 1 回だけ知らせる(`FollowState::notified`)。
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Notice {
    NoRecord {
        main_rev: String,
    },
    ForeignGeneration {
        main_rev: String,
        generation: String,
    },
    ExplicitGeneration {
        main_rev: String,
        generation: String,
    },
    PolicyChanged {
        main_rev: String,
    },
    Failed {
        main_rev: String,
        rc: i32,
    },
}

fn short_rev(rev: &str) -> &str {
    rev.get(..7).unwrap_or(rev)
}

impl Notice {
    /// 重複を抑えるキー。原因が変われば(世代が変わる・main が進む)別のキーになり、
    /// 改めて知らせる。
    pub fn key(&self) -> String {
        match self {
            Notice::NoRecord { .. } => "no-record".into(),
            Notice::ForeignGeneration { generation, .. } => format!("foreign:{generation}"),
            Notice::ExplicitGeneration { generation, .. } => format!("explicit:{generation}"),
            Notice::PolicyChanged { main_rev } => format!("policy:{main_rev}"),
            Notice::Failed { main_rev, .. } => format!("failed:{main_rev}"),
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Notice::NoRecord { .. } => "hms の自動追従: 一度 hms を当ててください",
            Notice::ForeignGeneration { .. } => "hms の自動追従を止めています(世代の不一致)",
            Notice::ExplicitGeneration { .. } => "hms の自動追従を止めています(検証用の世代)",
            Notice::PolicyChanged { .. } => "hms の自動追従を止めています(許可ポリシーの変更)",
            Notice::Failed { .. } => "hms の自動追従に失敗しました",
        }
    }

    pub fn body(&self) -> String {
        match self {
            Notice::NoRecord { main_rev } => format!(
                "main が {} に進みましたが、この世代には適用の記録がありません。hms を 1 回実行すると、以後は自動で追従します。",
                short_rev(main_rev)
            ),
            Notice::ForeignGeneration { main_rev, .. } => format!(
                "いまの世代は hms の記録と一致しません(rollback や、hms を通さない switch)。main {} へ戻すには hms を実行してください。",
                short_rev(main_rev)
            ),
            Notice::ExplicitGeneration { main_rev, .. } => format!(
                "hms . などで ref を明示して当てた世代なので、main {} へは追従しません。戻すには hms を実行してください。",
                short_rev(main_rev)
            ),
            Notice::PolicyChanged { main_rev } => format!(
                "main {} のネイティブ権限設定に変更があるか、設定を読み取れません。Claude と Codex の設定差分を確認し、手動で hms を実行してください(docs/agents/agent-permissions.md)。",
                short_rev(main_rev)
            ),
            Notice::Failed { main_rev, rc } => format!(
                "main {} の適用が終了コード {rc} で失敗しました。journalctl --user -u hms-follow(macOS は ~/Library/Logs/hms-follow.log)を見てください。同じコミットは再試行しません。",
                short_rev(main_rev)
            ),
        }
    }
}

/// 自動追従が覚えておく状態(`follow-state.json`)。
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct FollowState {
    /// 最後に知らせた `Notice::key`。同じキーは知らせ直さない。
    pub notified: Option<String>,
    /// 追従に失敗した main のコミット。main が進むまで再試行しない。
    pub failed_rev: Option<String>,
}

/// 読めない・壊れているときは空の状態(最悪でも、もう 1 回知らせるだけ)。
pub fn parse_follow_state(json: &str) -> FollowState {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return FollowState::default();
    };
    let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    FollowState {
        notified: field("notified"),
        failed_rev: field("failed_rev"),
    }
}

pub fn follow_state_json(state: &FollowState) -> String {
    let mut text = serde_json::to_string_pretty(&serde_json::json!({
        "notified": state.notified,
        "failed_rev": state.failed_rev,
    }))
    .expect("a json! object always serializes");
    text.push('\n');
    text
}

/// herdr の `notification show` の応答から、通知が実際に表示されたか。
/// 応答の形(`{"result":{"shown":true,…}}`)は入れ子なので、`shown` の真偽値を再帰で探す。
/// 見つからなければ `false`(表示されたと言い切れないので、次回また知らせる側に倒す)。
pub fn notification_shown(json: &str) -> bool {
    fn find(v: &Value) -> Option<bool> {
        match v {
            Value::Object(map) => match map.get("shown") {
                Some(Value::Bool(b)) => Some(*b),
                _ => map.values().find_map(find),
            },
            Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| find(&v))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// 環境の解決
// ---------------------------------------------------------------------------

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

/// マーカーファイルの 1 行目から空白をすべて除いたもの(`head -n1 | tr -d '[:space:]'`)。
/// マーカーが読めない・空なら None。
fn read_marker(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(config_home().join("dotfiles").join(name)).ok()?;
    let first = text.lines().next().unwrap_or("");
    let v: String = first.chars().filter(|c| !c.is_whitespace()).collect();
    (!v.is_empty()).then_some(v)
}

/// 既定の flake ref を解決する: マーカーファイルが先、DEFAULT_REF がフォールバック
/// (ADR-0034。resolve_host・docs/agents/writing-style.md の style-hub マーカーと同じ
/// 間接参照)。マーカーにより 1 ホストの `hms`(ref 無し)を、この repo のホスト
/// module の上に private value module を重ねる private wrapper flake へ向けられる —
/// この repo のソースがその flake を名指しすることなく。home-manager で宣言すると
/// 同じ絶対パスが管理下(store symlink)のファイルに戻ってしまい間接参照が無意味に
/// なるので、このマーカーは手置きで home-manager 管理にしない。
fn resolve_default_ref() -> String {
    read_marker("private-hub").unwrap_or_else(|| DEFAULT_REF.to_string())
}

fn hostname() -> String {
    Command::new("hostname")
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|s| s.trim_end().to_string())
        })
        .unwrap_or_default()
}

/// 論理ホスト名を解決する: マーカーファイルが先、`hostname` がフォールバック
/// (ADR-0019)。マーカーでホストが星の codename("altair"、"vega")を持てる。解決が
/// OS の hostname に依存することはない。Linux の rename runbook
/// (docs/cutover-runbook.md)は OS hostname も同じ codename にするので、マーカー未配置
/// でも `hostname` フォールバックが正しく解決する。各 codename ホストの module は同じ
/// switch で `xdg.configFile` によりマーカーを宣言し、以後は home-manager 管理。
fn resolve_host() -> String {
    read_marker("host").unwrap_or_else(hostname)
}

fn command_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| {
            p.is_file()
                && std::fs::metadata(p)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        })
}

/// `readlink -f` 相当(失敗は空)。
fn readlink_f(p: &Path) -> String {
    std::fs::canonicalize(p)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Debug)]
/// [`try_capture`] の失敗。`detail` は診断用の 1 行(プログラム名と、起動失敗の
/// 理由、または終了状態と stderr の要点)。
struct CaptureFailure {
    /// 子を起動できなかった(コマンドが無い・exec 失敗など)。非ゼロ終了ではない。
    spawn_failed: bool,
    detail: String,
}

/// 成功した子の stdout(末尾改行除去)を返す。失敗は理由つきで返す(#1036)。
fn try_capture(cmd: &mut Command) -> Result<String, CaptureFailure> {
    let prog = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| CaptureFailure {
            spawn_failed: true,
            detail: format!("{prog}: {e}"),
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(3)
            .collect();
        let tail = if tail.is_empty() {
            String::new()
        } else {
            format!(": {}", tail.join(" / "))
        };
        return Err(CaptureFailure {
            spawn_failed: false,
            detail: format!("{prog}: {}{tail}", out.status),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string())
}

/// 成功した子の stdout(末尾改行除去)。失敗・起動不能は None(呼び出し側が縮退する)。
/// 非ゼロ終了は通常の縮退(offline など)なので黙るが、子を起動できなかったときは
/// 理由を stderr に 1 行出す — 縮退の原因が環境の異常(exec 失敗など)だと見えない
/// 断続的な失敗の診断のため(#1036)。
fn capture(cmd: &mut Command) -> Option<String> {
    match try_capture(cmd) {
        Ok(s) => Some(s),
        Err(f) => {
            if f.spawn_failed {
                eprintln!("hms: {}", f.detail);
            }
            None
        }
    }
}

fn status_ok(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 子を素通しで実行し終了コードを返す。起動できなければ(シェルの command not found と
/// 同じ)127。
fn run_inherit(cmd: &mut Command) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match cmd.status() {
        Ok(st) => st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)),
        Err(e) => {
            eprintln!("hms: {}: {e}", cmd.get_program().to_string_lossy());
            127
        }
    }
}

// ---------------------------------------------------------------------------
// 永続状態: 排他ロックと適用の記録(状態のパスは hms が所有する)
// ---------------------------------------------------------------------------

fn state_dir() -> PathBuf {
    home().join(".local/state/hms")
}

/// 排他ロックの保持者。drop(またはプロセスの終了)で解放される。
/// `File` は O_CLOEXEC で開かれるので、子の nix・home-manager には継承されない。
struct ApplyLock {
    _file: std::fs::File,
}

/// hms 全体で 1 つの適用ロックを取る。
///
/// 取れなければ、`wait` なら待つ旨を言ってから取れるまで待ち、そうでなければ
/// `Ok(None)`(誰かが適用中)。ロックファイルを開けない(状態ディレクトリを作れない
/// など)のは `Err` — ロックなしで進むと、同時適用を防ぐという目的が崩れる。
fn acquire_apply_lock(wait: bool) -> std::io::Result<Option<ApplyLock>> {
    use std::fs::TryLockError;
    let dir = state_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(ApplyLock { _file: file })),
        Err(TryLockError::WouldBlock) => {
            if !wait {
                return Ok(None);
            }
            eprintln!(
                "==> 別の hms が適用中です。終わるまで待ちます({})",
                path.display()
            );
            file.lock()?;
            Ok(Some(ApplyLock { _file: file }))
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn profile_link() -> PathBuf {
    home().join(".local/state/nix/profiles/home-manager")
}

/// 状態ディレクトリのファイルを丸ごと書く。一時ファイルに書いてから rename するので、
/// 途中で落ちても読み手は完全な旧内容か完全な新内容のどちらかを見る。
fn write_state_file(name: &str, text: &str) -> std::io::Result<()> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{name}.tmp.{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join(name)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn read_state_file(name: &str) -> Option<String> {
    std::fs::read_to_string(state_dir().join(name)).ok()
}

/// switch に成功した世代の記録を書く。
fn write_apply_record(mode: ApplyMode) -> std::io::Result<()> {
    let applied_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let text = apply_record_json(
        mode,
        generation_rev().as_deref(),
        &readlink_f(&profile_link()),
        applied_at,
    );
    write_state_file("last-apply.json", &text)
}

// ---------------------------------------------------------------------------
// 診断(warn-only)
// ---------------------------------------------------------------------------

/// herdr server の陳腐化チェック(warn-only、#200): switch は `herdr` の store path を
/// 差し替えるが、動いている `herdr server` は再起動しない — 誰かが外から kill して
/// 起動し直すまで旧バイナリがメモリに載り続ける(docs/operations.md「Restarting herdr
/// after a switch...」)。hms は通常 herdr の pane の中で動くので自分のホストプロセスは
/// 再起動できず、warn するだけにする(check_generation_consistency と同じ形)。
fn check_herdr_staleness() {
    let Some(herdr) = command_path("herdr") else {
        return;
    };
    let current_bin = readlink_f(&herdr);
    let running_pid = capture(Command::new("pgrep").args(["-x", "herdr"]))
        .and_then(|s| s.lines().next().map(str::to_string))
        .unwrap_or_default();
    if current_bin.is_empty() || running_pid.is_empty() {
        return;
    }
    let running_bin = readlink_f(Path::new(&format!("/proc/{running_pid}/exe")));
    if running_bin.is_empty() {
        return;
    }
    // 両方が nix store に解決されるときだけ比較する — それ以外(wrapper script、
    // 非 nix のインストール)は陳腐化のシグナルではない。
    if !(current_bin.starts_with("/nix/store/") && running_bin.starts_with("/nix/store/")) {
        return;
    }
    if current_bin != running_bin {
        eprintln!("Warning: the running herdr server is still on the old binary.");
        eprintln!("  current generation: {current_bin}");
        eprintln!("  running (pid {running_pid}): {running_bin}");
        eprintln!("  Any change to herdr's server-side behavior will not take effect");
        eprintln!("  until you kill and relaunch it from outside herdr (see");
        eprintln!("  docs/operations.md, 'Restarting herdr after a switch...').");
    }
}

/// home-manager は `nix-env --profile --set`(current generation を進める)を、
/// activation script の本体が走る前に実行する。activation が途中で失敗する
/// (checkLinkTargets 等、#62/#63)と、current generation は新しい store path を指すのに
/// gcroots/current-home 配下の実際の home-files symlink は旧 generation のまま —
/// 「generation は進んだが世界は古い」状態になり、hms は検出しない(#65)。両者が
/// 食い違うときに警告する(失敗にはしない)。
///
/// `switch_already_failed`: この呼び出しの時点で *今回自身の* switch が既に失敗して
/// いるか(失敗後の呼び出しが true を渡す)— そうでないと締めの文が、今まさに失敗した
/// switch について「この switch が解消する」と言ってしまう。
fn check_generation_consistency(switch_already_failed: bool) {
    let profile_link = home().join(".local/state/nix/profiles/home-manager");
    let current_home_link = home().join(".local/state/home-manager/gcroots/current-home");
    if !(profile_link.exists() && current_home_link.exists()) {
        return;
    }
    let profile_target = readlink_f(&profile_link);
    let current_home_target = readlink_f(&current_home_link);
    if profile_target != current_home_target {
        eprintln!("Warning: home-manager generation/reality mismatch detected.");
        eprintln!("  profile ({}): {profile_target}", profile_link.display());
        eprintln!(
            "  current-home ({}): {current_home_target}",
            current_home_link.display()
        );
        eprintln!("  A previous activation likely failed partway through, leaving the");
        eprintln!("  generation pointer ahead of what is actually applied.");
        if switch_already_failed {
            eprintln!("  This switch also failed, so the mismatch remains — fix the error");
            eprintln!("  above and rerun; a successful switch will then resolve it.");
        } else {
            eprintln!("  A successful switch (this one) will resolve it.");
        }
    }
}

// ---------------------------------------------------------------------------
// メインの流れ
// ---------------------------------------------------------------------------

fn print_help(r#ref: &str, wrapper_ref: &str) {
    println!("Usage: hms [flake-ref] [--public-only]");
    println!();
    println!("Apply the home-manager configuration for this host.");
    println!("  hms          apply pushed main ({})", r#ref);
    println!("               bleep is taken at its main, verified by a pre-build; if that");
    println!("               fails, the locked pin is applied instead.");
    println!("  hms .        apply the current checkout/worktree (pre-push verification).");
    if wrapper_ref != DEFAULT_REF {
        println!("               Routed through {wrapper_ref} with dotfiles overridden to");
        println!("               this checkout — pass --public-only to apply it alone instead.");
    }
    println!("  hms <path>   apply an arbitrary local checkout (same routing as `hms .`)");
    println!("  hms . --public-only   apply the checkout alone, dropping private value modules");
    println!(
        "  hms --follow    apply pushed main only if this generation was made by a bare `hms`"
    );
    println!("                  and main has moved on (run by the hms-follow timer; otherwise it");
    println!("                  just says why it is not following)");
    println!(
        "  hms --version   print the dotfiles commit of the generation that installed this hms"
    );
}

/// `nix flake metadata --refresh --json <flake>` の revision(解決できなければ None)。
fn refreshed_revision(flake: &str) -> Option<(String, String)> {
    let json =
        capture(Command::new("nix").args(["flake", "metadata", "--refresh", "--json", flake]))?;
    let rev = metadata_revision(&json);
    (!rev.is_empty()).then_some((json, rev))
}

/// bleep main の解決済み revision(`nix flake prefetch --refresh`。解決できなければ None)。
fn refreshed_bleep_revision() -> Option<String> {
    let json =
        capture(Command::new("nix").args(["flake", "prefetch", "--refresh", "--json", BLEEP_REF]))?;
    let rev = prefetch_revision(&json);
    (!rev.is_empty()).then_some(rev)
}

/// いま動いているバイナリが入った generation の rev(`~/.local/share/dotfiles/rev`、
/// flake.nix の `mkHome` が置く 1 行)。読めなければ `None`。コンパイル時に埋め込むと
/// コミットごとにワークスペース全体が再ビルドされるので、実行時に読む(#807)。
fn generation_rev() -> Option<String> {
    let text = std::fs::read_to_string(home().join(".local/share/dotfiles/rev")).ok()?;
    let rev = text.trim();
    (!rev.is_empty()).then(|| rev.to_string())
}

/// `--version` の 1 行(#807)。古い版の `hms` で適用して気づかないのを防ぐ。
fn version_line(rev: Option<&str>) -> String {
    format!(
        "hms {} (generation {})",
        env!("CARGO_PKG_VERSION"),
        rev.unwrap_or("unknown")
    )
}

/// 引数(`argv[0]` を除く)を処理して終了コードを返す。
pub fn run(args: &[String]) -> i32 {
    // ロックを待つ間に別の適用が世代を切り替えたかを、switch の前に見分けるための起点(#1025)。
    let generation_at_start = verify::current_profile_generation();
    let wrapper_ref = resolve_default_ref();
    let mut r#ref = wrapper_ref.clone();
    let mut public_only = false;
    let mut ref_given = false;
    let mut follow_main = false;

    for a in args {
        match a.as_str() {
            "--help" | "-h" => {
                print_help(&r#ref, &wrapper_ref);
                return 0;
            }
            "--version" | "-V" => {
                println!("{}", version_line(generation_rev().as_deref()));
                return 0;
            }
            "--public-only" => public_only = true,
            "--follow" => follow_main = true,
            s if s.starts_with('-') => {
                eprintln!("Error: Unknown option: {s}");
                return 1;
            }
            s => {
                r#ref = s.to_string();
                ref_given = true;
            }
        }
    }

    if follow_main {
        if ref_given || public_only {
            eprintln!("Error: --follow は ref や --public-only と一緒には使えません。");
            return 1;
        }
        return follow(&wrapper_ref);
    }

    // 適用は同時に 1 つだけ(#1025)。別の hms が適用中なら、終わるまでここで待つ。
    // 以降の読み取り(世代の整合・降格 guard)も、先の適用が終わった後の状態を見る。
    let _lock = match acquire_apply_lock(true) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "Error: 適用ロック({})を取れません: {e}",
                state_dir().join("lock").display()
            );
            return 1;
        }
    };

    let req = ApplyRequest {
        wrapper_ref,
        r#ref,
        public_only,
        ref_given,
        generation_at_start,
        argv: args.to_vec(),
    };
    match apply(&req, &mut |_| Ok(())) {
        ApplyOutcome::Done(rc) => rc,
        // preflight が常に Ok の呼び出しでは起きない。
        ApplyOutcome::Stopped(_) => {
            eprintln!("Error: 内部エラー: 通常の適用が事前確認で止まりました");
            1
        }
    }
}

/// 適用の要求(引数から決まる)。
struct ApplyRequest {
    /// `resolve_default_ref()`(wrapper 登録ホストでは wrapper の ref)。
    wrapper_ref: String,
    /// 適用する ref(引数なしなら `wrapper_ref` と同じ)。
    r#ref: String,
    public_only: bool,
    /// 位置引数で ref を渡したか(`ApplyMode` の判定に使う)。
    ref_given: bool,
    /// 適用を始めた時点(ロックを待つ前)の profile が指す世代。待つ間に別の適用が世代を
    /// 切り替えたかの起点(#1025)。
    generation_at_start: String,
    /// 競合の報告が「再実行する」コマンドとして示す、`hms` への引数。
    argv: Vec<String>,
}

/// switch の直前に確定した適用内容。
struct Prepared {
    apply_ref: String,
    host: String,
    extra_opts: Vec<String>,
}

/// `preflight` が switch を止めた理由(`--follow` の事前確認)。
enum PreflightStop {
    /// 新しい世代のネイティブ権限設定が配備済みと違う、欠けている、または読めない。
    PolicyChanged,
    /// 新しい世代の事前ビルドが失敗した(終了コード)。
    BuildFailed(i32),
}

enum ApplyOutcome {
    /// 適用を最後まで進めた、または途中で失敗した。終了コード。
    Done(i32),
    /// switch の直前に `preflight` が止めた。
    Stopped(PreflightStop),
}

/// 適用の本体: 世代の整合・降格 guard・ref の解決・wrapper と bleep の override・
/// switch・後処理・適用の記録。呼び出し側が適用ロックを持っていること。
///
/// `preflight` は、switch の直前(ref と override がすべて確定した後)に呼ばれる。
/// `Err` を返すと switch せずに止まる。
fn apply(
    req: &ApplyRequest,
    preflight: &mut dyn FnMut(&Prepared) -> Result<(), PreflightStop>,
) -> ApplyOutcome {
    let wrapper_ref = req.wrapper_ref.clone();
    let r#ref = req.r#ref.clone();
    let public_only = req.public_only;
    let ref_given = req.ref_given;

    let host = resolve_host();

    check_generation_consistency(false);

    // private-hub 降格 guard (#567): このホストの直前の generation が既に wrapper
    // 経由で適用済み(home-files に .config/dotfiles/private-hub が存在する)なのに、
    // 今回の resolve_default_ref() が DEFAULT_REF(未登録)に縮退していて、かつ
    // --public-only の明示指定も無いなら、wrapper が配る私的な value module
    // (bleep の denylist config 等)を無警告で撤去する適用になる。実際にこの手順で
    // 起きた事故(2026-09-29、#567)を機械的に検知して止める。
    let profile_link = home().join(".local/state/nix/profiles/home-manager");
    let had_marker_prev_gen = profile_link.exists()
        && PathBuf::from(readlink_f(&profile_link))
            .join("home-files/.config/dotfiles/private-hub")
            .exists();
    if private_hub_downgrade_abort(had_marker_prev_gen, &wrapper_ref, DEFAULT_REF, public_only) {
        eprintln!("Error: 直前の generation は private wrapper flake 経由で適用済みでしたが、");
        eprintln!("  今回は ~/.config/dotfiles/private-hub マーカーが見つからず、public");
        eprintln!("  単体({DEFAULT_REF})に縮退します。このまま進めると wrapper が配った");
        eprintln!("  私的な value module(bleep の denylist config 等)が無警告で撤去されます");
        eprintln!("  (#567、2026-09-29 に実際に発生)。");
        eprintln!("  意図的な public 単体適用なら --public-only を明示してください。");
        eprintln!("  wrapper へ戻すなら次でマーカーを復旧してから再実行してください:");
        eprintln!(
            "    mkdir -p ~/.config/dotfiles && printf '%s\\n' '<wrapper flake ref>' > ~/.config/dotfiles/private-hub"
        );
        return ApplyOutcome::Done(1);
    }

    let mut extra_opts: Vec<String> = Vec::new();

    // リモートの flake ref(github:、git+ssh: …)は nix がキャッシュするもの。ローカル
    // パス(`.` や checkout ディレクトリ)は常に現在のツリーを読むので refresh は不要。
    let ref_is_local = Path::new(&r#ref).exists();
    let ref_meta_json: String;
    if !ref_is_local {
        println!("==> nix flake metadata --refresh {}", r#ref);
        match refreshed_revision(&r#ref) {
            Some((json, rev)) => {
                println!("==> applying revision {rev}");
                ref_meta_json = json;
            }
            None => {
                eprintln!(
                    "==> could not resolve a revision for {} (offline?); continuing with whatever switch resolves",
                    r#ref
                );
                ref_meta_json = String::new();
            }
        }
    } else {
        // `hms .` はセッション途中でほぼ必ず dirty なローカル checkout/worktree を適用する
        // — それが push 前検証の目的そのもの。nix は毎回 "warning: Git tree '<path>' has
        // uncommitted changes" を繰り返す。ここ(ローカルパス分岐)だけで抑え、nix.conf で
        // マシン全体には効かせない(#149)— 非ローカル ref ではこの警告は出ない。
        extra_opts = vec!["--option".into(), "warn-dirty".into(), "false".into()];
        ref_meta_json = capture(Command::new("nix").args(["flake", "metadata", "--json", &r#ref]))
            .unwrap_or_default();
    }

    let ref_is_wrapper =
        !ref_meta_json.is_empty() && !wrapper_locked_dotfiles_rev(&ref_meta_json).is_empty();
    let plan = local_apply_plan(
        &wrapper_ref,
        DEFAULT_REF,
        ref_is_local,
        ref_is_wrapper,
        public_only,
    );

    // ローカル適用の routing(ADR-0034 Amendment 2026-09-27): wrapper 登録ホストでの
    // ローカルパス適用は、そのパス単体を適用して private value module(bleep 自身の
    // denylist config を含む)をすべて落とす代わりに、`dotfiles` をそのパスへ override
    // した wrapper 経由にする。
    let mut apply_ref = r#ref.clone();
    if plan == ApplyPlan::Route {
        // `realpath -m`(存在しなくても解決。ここでは ref_is_local なので存在する)
        let abs = std::fs::canonicalize(&r#ref)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(&r#ref));
        apply_ref = wrapper_ref.clone();
        extra_opts.extend([
            "--override-input".into(),
            "dotfiles".into(),
            format!("path:{}", abs.display()),
            "--no-write-lock-file".into(),
        ]);
    }

    // wrapper-lock override(ADR-0034 Amendment 2026-09-25): 適用対象 flake が
    // `dotfiles` input を持つ(wrapper、リモート/ローカル)なら、wrapper の lock を
    // 信用せず pushed main の解決済み revision で pin する。上の routing とは排他 —
    // routing は意図的に `dotfiles` をローカルパスに pin 済みで、pushed main の
    // revision の出番は無い。
    if plan != ApplyPlan::Route && !ref_meta_json.is_empty() {
        let locked = wrapper_locked_dotfiles_rev(&ref_meta_json);
        if !locked.is_empty() {
            match refreshed_revision(DEFAULT_REF) {
                Some((_, main_rev)) => {
                    if main_rev == locked {
                        println!(
                            "==> dotfiles revision {main_rev} (wrapper's lock already at this revision)"
                        );
                    } else {
                        println!(
                            "==> dotfiles revision {main_rev} (overriding the wrapper's lock, which pins {locked})"
                        );
                    }
                    extra_opts.extend(dotfiles_override_opts(&main_rev));
                }
                None => eprintln!(
                    "==> could not resolve {DEFAULT_REF} (offline?); applying the wrapper's lock as-is (dotfiles {locked})"
                ),
            }
        }
    }

    // bleep を main の最新に追従させる(リモート適用のみ — `hms .` は checkout の pin を
    // そのまま検証する場)。flake.lock の pin は「検証済みの既知良好版」で、ここでの
    // override は適用時だけ。bleep 側の破壊的変更で dotfiles の配線と噛み合わなく
    // なる版は、配備された hook の canary(home.checks、home/modules/claude.nix)で
    // generation の build 自体が失敗するので、事前 build で確かめて pin に戻す。
    if !ref_is_local && plan != ApplyPlan::Route {
        if let Some(pin) = bleep_pin(&ref_meta_json) {
            match refreshed_bleep_revision() {
                Some(main_rev) if main_rev == pin.rev => {
                    println!(
                        "==> bleep revision {main_rev} (lock already at this revision)"
                    );
                }
                Some(main_rev) => {
                    let opts = bleep_override_opts(&pin.input, &main_rev);
                    println!(
                        "==> nix build {apply_ref}#homeConfigurations.{host}.activationPackage (bleep {main_rev}; verifying before switch)"
                    );
                    let rc = run_inherit(
                        Command::new("nix")
                            .args(["build", "--no-link"])
                            .arg(format!(
                                "{apply_ref}#homeConfigurations.{host}.activationPackage"
                            ))
                            .args(&extra_opts)
                            .args(&opts),
                    );
                    if rc == 0 {
                        println!(
                            "==> bleep revision {main_rev} (overriding the lock pin {}; canary passed)",
                            pin.rev
                        );
                        extra_opts.extend(opts);
                    } else {
                        eprintln!(
                            "Warning: bleep main {main_rev} failed to build or failed the deployed-hook canary; applying the locked pin {} instead.",
                            pin.rev
                        );
                        eprintln!(
                            "  dotfiles likely needs to follow a breaking bleep change — bump the pin in a PR"
                        );
                        eprintln!(
                            "  together with its call sites (see the bleep input comment in flake.nix)."
                        );
                    }
                }
                None => eprintln!(
                    "==> could not resolve {BLEEP_REF} (offline?); applying the locked bleep pin as-is (bleep {})",
                    pin.rev
                ),
            }
        }
    }

    // ファイル冒頭の `--override-input` の注記を参照: nix 自身の "not writing modified
    // lock file" 出力の前に、説明なしで残さず警告する。
    if lock_warning_expected(&extra_opts) {
        println!(
            "==> note: a nix \"warning: not writing modified lock file\" below is expected — an input is overridden for this switch only; the flake.lock is intentionally left untouched (ADR-0034)"
        );
    }

    // switch の直前: 呼び出し側(`--follow`)が、この適用を進めてよいかを確かめる。
    // 通常の `hms` は常に通す。
    let prepared = Prepared {
        apply_ref: apply_ref.clone(),
        host: host.clone(),
        extra_opts: extra_opts.clone(),
    };
    if let Err(why) = preflight(&prepared) {
        return ApplyOutcome::Stopped(why);
    }

    // 今回の目標の世代を switch の前に求める(switch 後の照合に使う、#1025)。
    let verified = match verify::prepare(&verify::ApplyInput {
        apply_ref: &apply_ref,
        host: &host,
        extra_opts: &extra_opts,
        local_ref: ref_is_local.then_some(r#ref.as_str()),
        meta_json: &ref_meta_json,
        generation_at_start: &req.generation_at_start,
    }) {
        Ok(p) => p,
        Err(rc) => return ApplyOutcome::Done(rc),
    };

    println!(
        "==> home-manager switch --flake {apply_ref}#{host} -b backup {}",
        extra_opts.join(" ")
    );
    let rc = run_inherit(
        Command::new("home-manager")
            .args(["switch", "--flake"])
            .arg(format!("{apply_ref}#{host}"))
            .args(["-b", "backup"])
            .args(&extra_opts),
    );
    if rc != 0 {
        check_generation_consistency(true);
        return ApplyOutcome::Done(rc);
    }

    // switch の成功は「今回の世代が有効」の証拠にならない(別の適用が割り込みうる、#1025)。
    // 目標の世代と profile・current-home を照合し、食い違えば後処理へ進まず非 0 で終える。
    let rc = verify::after_switch(&verified, &req.argv);
    if rc != 0 {
        return ApplyOutcome::Done(rc);
    }

    // この世代をどの経路で作ったかを、後処理の前に記録する。後処理(Tailscale・
    // systemd・fcitx5)が失敗しても、照合を通った時点で世代は適用済み —
    // 記録が無いと、hms が作った世代を「hms 以外で切り替えられた」と誤認する。
    // 照合の後に置くのは、食い違ったとき profile は別の適用の世代を指しており、
    // その前に記録すると他人の世代をこの経路で作ったと書いてしまうため。
    // 記録できなくても適用は失敗にしない。
    if let Err(e) = write_apply_record(apply_mode(ref_given, public_only)) {
        eprintln!(
            "Warning: 適用の記録({})を書けませんでした: {e}",
            state_dir().join("last-apply.json").display()
        );
    }

    // Tailscale prefs の収束(ADR-471、下の check_herdr_staleness と同じ warn-only —
    // Tailscale 未インストール/未認証で switch を失敗させてはならない)。Linux と
    // darwin 共通なので、下の Linux 専用の systemd/fcitx5 後処理より前に走る。
    if command_path("tailscale-prefs").is_some() {
        let rc = run_inherit(Command::new("tailscale-prefs").arg("apply"));
        if rc != 0 {
            return ApplyOutcome::Done(rc);
        }
    }

    // systemd --user と fcitx5 unit は Linux 専用(ADR-0018)。darwin ホスト(altair 等)
    // にはどちらも無いので後処理全体が no-op。
    if std::env::consts::OS != "linux" {
        println!("==> non-Linux host; skipping systemctl daemon-reload and fcitx5 restart.");
        println!("Done.");
        return ApplyOutcome::Done(0);
    }

    println!("==> systemctl --user daemon-reload");
    let rc = run_inherit(Command::new("systemctl").args(["--user", "daemon-reload"]));
    if rc != 0 {
        return ApplyOutcome::Done(rc);
    }

    check_herdr_staleness();

    // fcitx5 unit の後処理 — unit の無いホストではきれいにスキップ。
    if !status_ok(Command::new("systemctl").args(["--user", "cat", FCITX5_UNIT])) {
        println!("==> {FCITX5_UNIT} not present; skipping fcitx5 restart.");
        println!("Done.");
        return ApplyOutcome::Done(0);
    }

    println!("==> systemctl --user restart {FCITX5_UNIT}");
    let rc = run_inherit(Command::new("systemctl").args(["--user", "restart", FCITX5_UNIT]));
    if rc != 0 {
        return ApplyOutcome::Done(rc);
    }

    // 検証: restart は同期的で daemon-reload の後に走るので、MainPID が生きている active な
    // unit は構造上、新 generation の ExecStart で動いている。MainPID が正本 — プロセス名
    // (pgrep)や exe path での照合はここでは不可能: nixpkgs は fcitx5 を wrap する
    // (bin/fcitx5 -> .fcitx5-wrapped -> 本体)ので comm は ".fcitx5-wrapped" になり、
    // /proc/<pid>/exe は unit の ExecStart が指す wrapper の先へ解決される。
    if !status_ok(Command::new("systemctl").args(["--user", "is-active", "--quiet", FCITX5_UNIT])) {
        eprintln!("Error: {FCITX5_UNIT} is not active after the restart.");
        // `systemctl status --no-pager UNIT >&2 || true`
        let _ = Command::new("systemctl")
            .args(["--user", "status", "--no-pager", FCITX5_UNIT])
            .stdout(std::io::stderr())
            .status();
        return ApplyOutcome::Done(1);
    }

    let pid = match capture_strict(Command::new("systemctl").args([
        "--user",
        "show",
        "-p",
        "MainPID",
        "--value",
        FCITX5_UNIT,
    ])) {
        Ok(p) => p,
        Err(rc) => return ApplyOutcome::Done(rc),
    };
    if pid.is_empty() || pid == "0" {
        eprintln!("Error: {FCITX5_UNIT} is active but has no MainPID.");
        return ApplyOutcome::Done(1);
    }

    let running_bin = readlink_f(Path::new(&format!("/proc/{pid}/exe")));
    println!(
        "==> fcitx5 running (pid {pid}) from {}",
        if running_bin.is_empty() {
            "<unknown>"
        } else {
            &running_bin
        }
    );
    println!("Done.");
    ApplyOutcome::Done(0)
}

// ---------------------------------------------------------------------------
// 自動追従(`hms --follow`。hms-follow の timer が呼ぶ)
// ---------------------------------------------------------------------------

/// herdr の通知を出す。表示されたと分かったときだけ true(due-remind と同じ — herdr に
/// クライアントが居ない時刻の通知は捨てられるので、表示を確かめずに「知らせた」と
/// しない)。journal / launchd のログにも同じ内容を残す。
fn notify(notice: &Notice) -> bool {
    eprintln!("hms --follow: {}: {}", notice.title(), notice.body());
    let Some(herdr) = command_path("herdr") else {
        eprintln!("hms --follow: herdr が PATH に無いので、通知は出せません");
        return false;
    };
    // herdr の通知の本文は短く切る(gpg-subkey remind と同じ 230 文字)。
    let body: String = notice.body().chars().take(230).collect();
    let out = Command::new(herdr)
        .args([
            "notification",
            "show",
            notice.title(),
            "--body",
            &body,
            "--sound",
            "request",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) => notification_shown(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => false,
    }
}

fn read_follow_state() -> FollowState {
    read_state_file("follow-state.json")
        .map(|t| parse_follow_state(&t))
        .unwrap_or_default()
}

fn write_follow_state(state: &FollowState) {
    if let Err(e) = write_state_file("follow-state.json", &follow_state_json(state)) {
        eprintln!(
            "Warning: 自動追従の状態({})を書けませんでした: {e}",
            state_dir().join("follow-state.json").display()
        );
    }
}

/// 同じ原因では 1 回だけ知らせる。表示されなかったとき(herdr のクライアントが居ない
/// 等)は覚えず、次の周期でもう一度知らせる。
fn notify_once(state: &mut FollowState, notice: &Notice) {
    let key = notice.key();
    if state.notified.as_deref() == Some(key.as_str()) {
        return;
    }
    if notify(notice) {
        state.notified = Some(key);
        write_follow_state(state);
    }
}

/// 完全なネイティブ権限設定の差を調べる。欠如も自動適用を止める。
/// JSON の一部だけを抽出しない: apps の承認設定も比較対象に含まれる。
pub fn policy_changed(new: Option<&[u8]>, deployed: Option<&[u8]>) -> bool {
    new.is_none() || deployed.is_none() || new != deployed
}

/// switch の直前に Claude と Codex の完全なネイティブ権限設定を比較する。
/// 拡大・縮小を分類せず、差分・欠如・読み取りエラーがあれば本人の確認に戻す。
/// 設定の意味や所有権を hms で再実装しない。
fn policy_preflight(p: &Prepared) -> Result<(), PreflightStop> {
    let target = format!(
        "{}#homeConfigurations.{}.activationPackage",
        p.apply_ref, p.host
    );
    println!(
        "==> nix build {target} (comparing native agent permission settings before the switch)"
    );
    let out = capture_strict(
        Command::new("nix")
            .args(["build", "--no-link", "--print-out-paths"])
            .arg(&target)
            .args(&p.extra_opts),
    )
    .map_err(PreflightStop::BuildFailed)?;
    let Some(package) = out.lines().rev().find(|l| !l.trim().is_empty()) else {
        eprintln!("Error: nix build が store path を返しませんでした");
        return Err(PreflightStop::BuildFailed(1));
    };
    for name in ["claude-permissions.json", "codex-permissions.json"] {
        let candidate = Path::new(package)
            .join("home-files/.config/agent-native")
            .join(name);
        let deployed = config_home().join("agent-native").join(name);
        // Preserve the error distinction in diagnostics rather than equating
        // failed reads to matching absent files.
        let read = |path: &Path| match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!(
                    "Warning: ネイティブ権限設定を読み取れません: {}: {error}",
                    path.display()
                );
                None
            }
        };
        let candidate = read(&candidate);
        let deployed = read(&deployed);
        if policy_changed(candidate.as_deref(), deployed.as_deref()) {
            return Err(PreflightStop::PolicyChanged);
        }
    }
    Ok(())
}

/// `hms --follow`: いまの世代が引数なしの `hms` で作ったもので、main が進んでいれば
/// 追従する。それ以外は適用せず、理由を 1 回だけ知らせる。timer が呼ぶので、
/// 「適用しなかった」「失敗した」は終了コード 0 で終える(失敗した unit が居座らない
/// ように — home/modules/drift.nix の教訓)。非 0 は、ロックを取れないなど環境の異常だけ。
fn follow(wrapper_ref: &str) -> i32 {
    let generation_at_start = verify::current_profile_generation();
    let _lock = match acquire_apply_lock(false) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            println!("==> hms --follow: 別の hms が適用中なので、今回は見送ります");
            return 0;
        }
        Err(e) => {
            eprintln!(
                "Error: 適用ロック({})を取れません: {e}",
                state_dir().join("lock").display()
            );
            return 1;
        }
    };

    let mut state = read_follow_state();
    let Some(main) = refreshed_revision(DEFAULT_REF).map(|(_, rev)| rev) else {
        println!("==> hms --follow: main の先頭を解決できません(オフライン?)。何もしません");
        return 0;
    };
    let current_rev = generation_rev();
    let generation = readlink_f(&profile_link());
    let record = read_state_file("last-apply.json").and_then(|t| parse_apply_record(&t));

    let action = follow_decision(&FollowFacts {
        main_rev: Some(&main),
        current_rev: current_rev.as_deref(),
        current_generation: &generation,
        record: record.as_ref(),
        failed_rev: state.failed_rev.as_deref(),
    });
    match action {
        FollowAction::Idle => {
            if current_rev.as_deref() == Some(main.as_str()) {
                println!("==> hms --follow: 追従済みです(main {})", short_rev(&main));
                if state != FollowState::default() {
                    write_follow_state(&FollowState::default());
                }
            } else {
                println!(
                    "==> hms --follow: main {} は前回の自動追従で失敗したので、再試行しません",
                    short_rev(&main)
                );
            }
            0
        }
        FollowAction::Stop(reason) => {
            let notice = match reason {
                StopReason::NoRecord => Notice::NoRecord { main_rev: main },
                StopReason::ForeignGeneration => Notice::ForeignGeneration {
                    main_rev: main,
                    generation,
                },
                StopReason::ExplicitGeneration => Notice::ExplicitGeneration {
                    main_rev: main,
                    generation,
                },
            };
            notify_once(&mut state, &notice);
            0
        }
        FollowAction::Apply => {
            let policy_notice = Notice::PolicyChanged {
                main_rev: main.clone(),
            };
            // 許可ポリシーの変更で止めた main は、通知済みの間は事前ビルドからやり直さない。
            if state.notified.as_deref() == Some(policy_notice.key().as_str()) {
                println!(
                    "==> hms --follow: main {} は許可ポリシーの変更を含むので、止めたままです",
                    short_rev(&main)
                );
                return 0;
            }
            println!(
                "==> hms --follow: main {} へ追従します(いまの世代 {})",
                short_rev(&main),
                current_rev.as_deref().map(short_rev).unwrap_or("unknown")
            );
            let req = ApplyRequest {
                wrapper_ref: wrapper_ref.to_string(),
                r#ref: wrapper_ref.to_string(),
                public_only: false,
                ref_given: false,
                generation_at_start,
                argv: vec!["--follow".to_string()],
            };
            let failed_rc = match apply(&req, &mut policy_preflight) {
                ApplyOutcome::Done(0) => {
                    write_follow_state(&FollowState::default());
                    return 0;
                }
                ApplyOutcome::Done(rc) | ApplyOutcome::Stopped(PreflightStop::BuildFailed(rc)) => {
                    rc
                }
                ApplyOutcome::Stopped(PreflightStop::PolicyChanged) => {
                    notify_once(&mut state, &policy_notice);
                    return 0;
                }
            };
            state.failed_rev = Some(main.clone());
            write_follow_state(&state);
            notify_once(
                &mut state,
                &Notice::Failed {
                    main_rev: main,
                    rc: failed_rc,
                },
            );
            0
        }
    }
}

/// `x="$(cmd)"` を `set -e` 下で実行する形: 失敗したらその終了コードで中断する。
fn capture_strict(cmd: &mut Command) -> Result<String, i32> {
    use std::os::unix::process::ExitStatusExt;
    match cmd.stdin(Stdio::null()).stderr(Stdio::inherit()).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout)
            .trim_end_matches('\n')
            .to_string()),
        Ok(o) => Err(o
            .status
            .code()
            .unwrap_or_else(|| 128 + o.status.signal().unwrap_or(0))),
        Err(e) => {
            eprintln!("hms: {}: {e}", cmd.get_program().to_string_lossy());
            Err(127)
        }
    }
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let rc = run(&args);
    ExitCode::from(u8::try_from(rc.rem_euclid(256)).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRAPPER: &str = "git+https://example.invalid/wrapper";

    // try_capture(#1036): 起動不能・非ゼロ終了の理由が失われない。

    #[test]
    fn try_capture_reports_why_a_child_could_not_start() {
        let f =
            try_capture(&mut Command::new("/nonexistent/hms-test-child")).expect_err("must fail");
        assert!(f.spawn_failed);
        assert!(
            f.detail.starts_with("/nonexistent/hms-test-child: ")
                && f.detail.contains("No such file or directory"),
            "{}",
            f.detail
        );
    }

    #[test]
    fn try_capture_reports_exit_status_and_stderr_of_a_failed_child() {
        let f = try_capture(Command::new("sh").args(["-c", "echo out; echo oops >&2; exit 3"]))
            .expect_err("must fail");
        assert!(!f.spawn_failed);
        assert_eq!(f.detail, "sh: exit status: 3: oops");
    }

    #[test]
    fn try_capture_returns_stdout_without_trailing_newlines() {
        let out = try_capture(Command::new("sh").args(["-c", "printf 'a\\nb\\n\\n'"]));
        assert_eq!(out.ok().as_deref(), Some("a\nb"));
    }

    // version_line(#807): 古い版を使っていることを、適用の前に気づけるようにする。

    #[test]
    fn version_line_with_and_without_a_generation_rev() {
        assert_eq!(
            version_line(Some("abc1234")),
            "hms 0.1.0 (generation abc1234)"
        );
        assert_eq!(version_line(None), "hms 0.1.0 (generation unknown)");
    }

    // wrapper_locked_dotfiles_rev(selftest 1-3)

    #[test]
    fn rev_1_dotfiles_input_present_is_a_wrapper_flake() {
        let json = r#"{"locks":{"nodes":{
            "root":{"inputs":{"dotfiles":"dotfiles","nixpkgs":"nixpkgs"}},
            "dotfiles":{"locked":{"rev":"abc123def456","type":"github"}}
        }}}"#;
        assert_eq!(wrapper_locked_dotfiles_rev(json), "abc123def456");
    }

    #[test]
    fn rev_2_no_dotfiles_input() {
        let json = r#"{"locks":{"nodes":{"root":{"inputs":{"nixpkgs":"nixpkgs"}}}}}"#;
        assert_eq!(wrapper_locked_dotfiles_rev(json), "");
    }

    #[test]
    fn rev_3_dotfiles_input_is_a_follows_array() {
        let json = r#"{"locks":{"nodes":{"root":{"inputs":{"dotfiles":["nixpkgs","dotfiles"]}}}}}"#;
        assert_eq!(wrapper_locked_dotfiles_rev(json), "");
    }

    #[test]
    fn rev_garbage_json_is_empty() {
        assert_eq!(wrapper_locked_dotfiles_rev("not json"), "");
        assert_eq!(wrapper_locked_dotfiles_rev(""), "");
    }

    // dotfiles_override_opts(selftest 4-5)

    #[test]
    fn opts_4_non_empty_revision_gives_4_args() {
        let o = dotfiles_override_opts("90db04baaa54c598a2b5ba847adbb9451d2bc798");
        assert_eq!(
            o,
            [
                "--override-input",
                "dotfiles",
                "github:tarotene/dotfiles/90db04baaa54c598a2b5ba847adbb9451d2bc798",
                "--no-write-lock-file"
            ]
        );
    }

    #[test]
    fn opts_5_empty_revision_gives_nothing() {
        assert!(dotfiles_override_opts("").is_empty());
    }

    // local_apply_plan(selftest 6-10)

    #[test]
    fn plan_6_wrapper_host_local_public_path_routes() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, true, false, false),
            ApplyPlan::Route
        );
    }

    #[test]
    fn plan_7_no_wrapper_registered_is_asis() {
        assert_eq!(
            local_apply_plan(DEFAULT_REF, DEFAULT_REF, true, false, false),
            ApplyPlan::AsIs
        );
    }

    #[test]
    fn plan_8_public_only_is_asis_even_on_wrapper_host() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, true, false, true),
            ApplyPlan::AsIs
        );
    }

    #[test]
    fn plan_9_ref_is_itself_the_wrapper_checkout_is_asis() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, true, true, false),
            ApplyPlan::AsIs
        );
    }

    #[test]
    fn plan_10_non_local_ref_on_wrapper_host_is_asis() {
        assert_eq!(
            local_apply_plan(WRAPPER, DEFAULT_REF, false, false, false),
            ApplyPlan::AsIs
        );
    }

    // private_hub_downgrade_abort(selftest 11-14)

    #[test]
    fn guard_11_marker_in_prev_gen_but_unregistered_now_aborts() {
        assert!(private_hub_downgrade_abort(
            true,
            DEFAULT_REF,
            DEFAULT_REF,
            false
        ));
    }

    #[test]
    fn guard_12_marker_in_prev_gen_and_still_registered_is_ok() {
        assert!(!private_hub_downgrade_abort(
            true,
            WRAPPER,
            DEFAULT_REF,
            false
        ));
    }

    #[test]
    fn guard_13_no_marker_in_prev_gen_is_ok() {
        assert!(!private_hub_downgrade_abort(
            false,
            DEFAULT_REF,
            DEFAULT_REF,
            false
        ));
    }

    #[test]
    fn guard_14_explicit_public_only_is_ok() {
        assert!(!private_hub_downgrade_abort(
            true,
            DEFAULT_REF,
            DEFAULT_REF,
            true
        ));
    }

    // lock_warning_expected(selftest 15-16)

    #[test]
    fn lock_15_override_input_present() {
        assert!(lock_warning_expected(&[
            "--override-input",
            "dotfiles",
            "path:/x",
            "--no-write-lock-file"
        ]));
    }

    #[test]
    fn lock_16_warn_dirty_only_is_not() {
        assert!(!lock_warning_expected(&["--option", "warn-dirty", "false"]));
        assert!(!lock_warning_expected::<&str>(&[]));
    }

    // bleep_pin / prefetch_revision / bleep_override_opts

    #[test]
    fn bleep_pin_direct_input() {
        let json = r#"{"locks":{"nodes":{
            "root":{"inputs":{"bleep":"bleep"}},
            "bleep":{"locked":{"rev":"abc"}}}}}"#;
        assert_eq!(
            bleep_pin(json),
            Some(BleepPin {
                input: "bleep".into(),
                rev: "abc".into()
            })
        );
    }

    #[test]
    fn bleep_pin_through_wrapper_dotfiles() {
        let json = r#"{"locks":{"nodes":{
            "root":{"inputs":{"dotfiles":"dotfiles"}},
            "dotfiles":{"inputs":{"bleep":"bleep_2"}},
            "bleep_2":{"locked":{"rev":"def"}}}}}"#;
        assert_eq!(
            bleep_pin(json),
            Some(BleepPin {
                input: "dotfiles/bleep".into(),
                rev: "def".into()
            })
        );
    }

    #[test]
    fn bleep_pin_absent_follows_or_garbage_is_none() {
        assert_eq!(
            bleep_pin(r#"{"locks":{"nodes":{"root":{"inputs":{}}}}}"#),
            None
        );
        let follows = r#"{"locks":{"nodes":{
            "root":{"inputs":{"dotfiles":"dotfiles"}},
            "dotfiles":{"inputs":{"bleep":["x","bleep"]}}}}}"#;
        assert_eq!(bleep_pin(follows), None);
        assert_eq!(bleep_pin("not json"), None);
        assert_eq!(bleep_pin(""), None);
    }

    #[test]
    fn prefetch_revision_semantics() {
        assert_eq!(prefetch_revision(r#"{"locked":{"rev":"abc"}}"#), "abc");
        assert_eq!(prefetch_revision(r#"{"locked":{}}"#), "");
        assert_eq!(prefetch_revision("oops"), "");
    }

    #[test]
    fn bleep_override_opts_shape() {
        assert_eq!(
            bleep_override_opts("dotfiles/bleep", "abc"),
            [
                "--override-input",
                "dotfiles/bleep",
                "github:tarotene/bleep/abc",
                "--no-write-lock-file"
            ]
        );
    }

    #[test]
    fn metadata_revision_semantics() {
        assert_eq!(metadata_revision(r#"{"revision":"abc"}"#), "abc");
        assert_eq!(metadata_revision(r#"{"revision":null}"#), "");
        assert_eq!(metadata_revision(r#"{}"#), "");
        assert_eq!(metadata_revision("oops"), "");
    }

    // apply_mode(D2): ref を渡したかどうかで決め、ローカル/リモートでは決めない。

    #[test]
    fn mode_without_a_ref_is_default() {
        assert_eq!(apply_mode(false, false), ApplyMode::Default);
    }

    #[test]
    fn mode_with_any_explicit_ref_is_explicit() {
        // `hms .`、`hms <path>`、`hms github:tarotene/dotfiles/<branch>` はどれも位置引数。
        assert_eq!(apply_mode(true, false), ApplyMode::Explicit);
    }

    #[test]
    fn mode_with_public_only_is_explicit_even_without_a_ref() {
        assert_eq!(apply_mode(false, true), ApplyMode::Explicit);
        assert_eq!(apply_mode(true, true), ApplyMode::Explicit);
    }

    #[test]
    fn apply_record_json_shape() {
        let v: Value = serde_json::from_str(&apply_record_json(
            ApplyMode::Default,
            Some("abc123"),
            "/nix/store/x-home-manager-generation",
            1_790_000_000,
        ))
        .unwrap();
        assert_eq!(v["mode"], "default");
        assert_eq!(v["rev"], "abc123");
        assert_eq!(v["generation"], "/nix/store/x-home-manager-generation");
        assert_eq!(v["applied_at"], 1_790_000_000u64);
    }

    #[test]
    fn apply_record_json_marks_an_unreadable_rev_as_null_and_ends_with_a_newline() {
        let text = apply_record_json(ApplyMode::Explicit, None, "/g", 1);
        assert!(text.ends_with('\n'));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["mode"], "explicit");
        assert!(v["rev"].is_null());
    }

    // parse_apply_record: 書いたものを読み戻せる。読めないものは「記録なし」。

    #[test]
    fn apply_record_round_trips() {
        for (mode, rev) in [
            (ApplyMode::Default, Some("abc123")),
            (ApplyMode::Explicit, None),
        ] {
            let text = apply_record_json(mode, rev, "/nix/store/g", 7);
            assert_eq!(
                parse_apply_record(&text),
                Some(ApplyRecord {
                    mode,
                    rev: rev.map(str::to_string),
                    generation: "/nix/store/g".into(),
                })
            );
        }
    }

    #[test]
    fn unreadable_apply_records_are_no_record() {
        assert_eq!(parse_apply_record("oops"), None);
        assert_eq!(parse_apply_record("{}"), None);
        assert_eq!(
            parse_apply_record(r#"{"mode":"someday","generation":"/g"}"#),
            None
        );
        assert_eq!(parse_apply_record(r#"{"mode":"default"}"#), None);
    }

    // follow_decision: 追従するのは、記録の世代がいまの世代で、かつ default のときだけ。

    const MAIN_REV: &str = "bbbbbbb1111111";
    const OLD_REV: &str = "aaaaaaa0000000";
    const GEN: &str = "/nix/store/gen-now";

    fn record(mode: ApplyMode, generation: &str) -> ApplyRecord {
        ApplyRecord {
            mode,
            rev: Some(OLD_REV.into()),
            generation: generation.into(),
        }
    }

    fn decide(
        main_rev: Option<&str>,
        current_rev: Option<&str>,
        record: Option<&ApplyRecord>,
        failed_rev: Option<&str>,
    ) -> FollowAction {
        follow_decision(&FollowFacts {
            main_rev,
            current_rev,
            current_generation: GEN,
            record,
            failed_rev,
        })
    }

    #[test]
    fn follow_is_idle_when_main_cannot_be_resolved() {
        let r = record(ApplyMode::Default, GEN);
        assert_eq!(
            decide(None, Some(OLD_REV), Some(&r), None),
            FollowAction::Idle
        );
    }

    #[test]
    fn follow_is_idle_when_already_at_main_even_for_a_verification_generation() {
        // 検証用の世代でも、たまたま main の先頭と同じなら、止める理由を知らせない。
        let r = record(ApplyMode::Explicit, GEN);
        assert_eq!(
            decide(Some(MAIN_REV), Some(MAIN_REV), Some(&r), None),
            FollowAction::Idle
        );
        assert_eq!(
            decide(Some(MAIN_REV), Some(MAIN_REV), None, None),
            FollowAction::Idle
        );
    }

    #[test]
    fn follow_stops_without_a_record() {
        assert_eq!(
            decide(Some(MAIN_REV), Some(OLD_REV), None, None),
            FollowAction::Stop(StopReason::NoRecord)
        );
    }

    #[test]
    fn follow_stops_when_the_record_is_for_another_generation() {
        let other = record(ApplyMode::Default, "/nix/store/gen-before");
        assert_eq!(
            decide(Some(MAIN_REV), Some(OLD_REV), Some(&other), None),
            FollowAction::Stop(StopReason::ForeignGeneration)
        );
        // profile を読めない(空)ときに、空の記録と一致したことにしない。
        let empty = record(ApplyMode::Default, "");
        assert_eq!(
            follow_decision(&FollowFacts {
                main_rev: Some(MAIN_REV),
                current_rev: Some(OLD_REV),
                current_generation: "",
                record: Some(&empty),
                failed_rev: None,
            }),
            FollowAction::Stop(StopReason::ForeignGeneration)
        );
    }

    #[test]
    fn follow_stops_for_a_verification_generation() {
        let r = record(ApplyMode::Explicit, GEN);
        assert_eq!(
            decide(Some(MAIN_REV), Some(OLD_REV), Some(&r), None),
            FollowAction::Stop(StopReason::ExplicitGeneration)
        );
    }

    #[test]
    fn follow_applies_for_a_default_generation_when_main_moved() {
        let r = record(ApplyMode::Default, GEN);
        assert_eq!(
            decide(Some(MAIN_REV), Some(OLD_REV), Some(&r), None),
            FollowAction::Apply
        );
        // current_rev が読めなくても、main が進んでいるかは分からないので追従に倒す。
        assert_eq!(
            decide(Some(MAIN_REV), None, Some(&r), None),
            FollowAction::Apply
        );
    }

    #[test]
    fn follow_does_not_retry_a_commit_that_failed_but_does_retry_a_newer_one() {
        let r = record(ApplyMode::Default, GEN);
        assert_eq!(
            decide(Some(MAIN_REV), Some(OLD_REV), Some(&r), Some(MAIN_REV)),
            FollowAction::Idle
        );
        assert_eq!(
            decide(
                Some(MAIN_REV),
                Some(OLD_REV),
                Some(&r),
                Some("older-failure")
            ),
            FollowAction::Apply
        );
    }

    // Notice: 原因が同じなら同じキー(1 回だけ知らせる)、変われば別のキー。

    #[test]
    fn notice_keys_follow_the_cause() {
        let a = Notice::ForeignGeneration {
            main_rev: MAIN_REV.into(),
            generation: "/g1".into(),
        };
        let a_later = Notice::ForeignGeneration {
            main_rev: "ccccccc".into(),
            generation: "/g1".into(),
        };
        let b = Notice::ForeignGeneration {
            main_rev: MAIN_REV.into(),
            generation: "/g2".into(),
        };
        // main が進んでも、同じ世代で止まっている間は知らせ直さない。
        assert_eq!(a.key(), a_later.key());
        assert_ne!(a.key(), b.key());
        // 許可ポリシーと失敗は、main のコミットごと。
        let policy = |r: &str| Notice::PolicyChanged { main_rev: r.into() }.key();
        let failed = |r: &str| {
            Notice::Failed {
                main_rev: r.into(),
                rc: 1,
            }
            .key()
        };
        assert_ne!(policy("aaa"), policy("bbb"));
        assert_ne!(failed("aaa"), failed("bbb"));
        assert_ne!(policy("aaa"), failed("aaa"));
    }

    #[test]
    fn notice_text_names_the_short_rev_and_the_way_out() {
        let n = Notice::PolicyChanged {
            main_rev: MAIN_REV.into(),
        };
        assert!(n.body().contains("bbbbbbb"));
        assert!(!n.body().contains(MAIN_REV));
        assert!(n.body().contains("docs/agents/agent-permissions.md"));
        let f = Notice::Failed {
            main_rev: MAIN_REV.into(),
            rc: 3,
        };
        assert!(f.body().contains("終了コード 3"));
        assert!(f.body().contains("journalctl --user -u hms-follow"));
        // 短い rev でも落ちない。
        assert_eq!(short_rev("abc"), "abc");
    }

    // FollowState / notification_shown / policy_changed

    #[test]
    fn follow_state_round_trips_and_garbage_is_empty() {
        let s = FollowState {
            notified: Some("policy:abc".into()),
            failed_rev: Some("abc".into()),
        };
        assert_eq!(parse_follow_state(&follow_state_json(&s)), s);
        assert_eq!(
            parse_follow_state(&follow_state_json(&FollowState::default())),
            FollowState::default()
        );
        assert_eq!(parse_follow_state("oops"), FollowState::default());
    }

    #[test]
    fn notification_shown_reads_the_nested_flag() {
        assert!(notification_shown(r#"{"id":"x","result":{"shown":true}}"#));
        assert!(!notification_shown(
            r#"{"result":{"shown":false,"reason":"disabled"}}"#
        ));
        // 見つからない・JSON でないときは「表示された」と言い切らない。
        assert!(!notification_shown(r#"{"result":{}}"#));
        assert!(!notification_shown("oops"));
        assert!(!notification_shown(""));
    }

    #[test]
    fn policy_changed_compares_complete_bytes_and_stops_on_any_missing() {
        assert!(!policy_changed(Some(b"{}"), Some(b"{}")));
        assert!(policy_changed(Some(b"{\"a\":1}"), Some(b"{}")));
        assert!(policy_changed(Some(b"{}"), None));
        assert!(policy_changed(None, Some(b"{}")));
        assert!(policy_changed(None, None));
    }
}
// probe 1
// probe 2
// probe 3
