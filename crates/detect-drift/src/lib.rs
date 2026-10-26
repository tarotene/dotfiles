//! detect-drift のコアロジック(#4)。
//!
//! home-manager が user 環境の単一正本(ADR-0001)である一方、apt /
//! `cargo install` / `npm -g` / `pipx` を経由した ad-hoc install は宣言と
//! 無関係に蓄積し、気づく仕組みが無かった(Issue #4)。このクレートは
//! **検出専用** — 宣言と実機在庫の diff を報告するだけで、一切変更しない
//! (`crates/git-audit-worktrees` / `crates/dotfiles-doctor` と同じ
//! detector-only の型)。
//!
//! ADR-0029(PATH 優先順位)がカバーする「順序由来の shadow」はここでは
//! 再検出しない — その ADR が Consequences で明示的に「在庫の削除は
//! 別 Issue」と切り出した在庫側を、この crate が引き受ける。
//!
//! パーサ(このファイル)とプロセス起動(main.rs)を分離しているのは、
//! 実コマンドの stdout を文字列として直接テストできるようにするため —
//! `cargo test` は apt/cargo/npm/pipx を一切実行しない。

use std::collections::BTreeSet;

/// レイヤーの報告の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriftKind {
    /// 実機にあるが宣言に無いもの(apt / cargo / npm / pipx)。
    #[default]
    Undeclared,
    /// 更新の遅れ(`apt-stale`、#1053)。`undeclared` の各行は、遅れを説明する 1 行。
    Stale,
}

/// 1 レイヤー(apt/cargo/npm/pipx、apt-stale)の diff 結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerDrift {
    pub layer: &'static str,
    pub kind: DriftKind,
    /// 実機にあるが宣言に無いもの(ソート済み、重複無し)。`kind` が
    /// [`DriftKind::Stale`] のときは、遅れを説明する行。
    pub undeclared: Vec<String>,
}

impl LayerDrift {
    pub fn is_clean(&self) -> bool {
        self.undeclared.is_empty()
    }

    /// 宣言外の在庫の報告(従来の 4 層)。
    pub fn undeclared(layer: &'static str, undeclared: Vec<String>) -> LayerDrift {
        LayerDrift {
            layer,
            kind: DriftKind::Undeclared,
            undeclared,
        }
    }
}

/// `packages/declarative/apt-packages.txt` を解析する。`#` 始まりはコメント、
/// 空行は無視。前後空白は trim する。
pub fn parse_apt_declared(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// `apt-mark showmanual` の出力(1行1パッケージ名)を解析する。
pub fn parse_apt_installed(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// apt レイヤーの diff: 手動インストール済みだが宣言にも baseline にも無いもの。
///
/// `baseline`(#445)は Pop!_OS の distinst post-install が入れる初期 seed
/// パッケージ(`apt-mark showmanual` からは「手動インストール」にしか見えない)
/// を除外するためのホスト別集合。`apt_baseline_path()`(main.rs)で解決した
/// ファイルが無ければ空集合を渡す — その場合は従来どおり `showmanual −
/// declared` の判定になり、seed パッケージ全件が drift として出る
/// (main.rs 側が WARN 1 行で baseline 作成を促す)。
pub fn diff_apt(
    declared: &BTreeSet<String>,
    installed: &BTreeSet<String>,
    baseline: &BTreeSet<String>,
) -> LayerDrift {
    let undeclared = installed
        .difference(declared)
        .filter(|name| !baseline.contains(*name))
        .cloned()
        .collect();
    LayerDrift::undeclared("apt", undeclared)
}

/// baseline ファイルの解析。宣言ファイル(`packages/declarative/
/// apt-packages.txt`)と同じ書式 — `#` コメント行・空行を許す(#3)。
/// これにより、Playwright `install-deps` のような一括インストールが
/// 入れた lib 群を「なぜ baseline に入っているか」の理由付きブロックと
/// して残せる。`parse_apt_declared` をそのまま使う(パーサは1つ、
/// duplicate しない)。
pub fn parse_apt_baseline(text: &str) -> BTreeSet<String> {
    parse_apt_declared(text)
}

/// `Commandline:` 1行から `apt`/`apt-get install <pkg...>`(`sudo` 経由も
/// 可)のインストール対象パッケージ名を抽出する。install 以外の
/// サブコマンド(upgrade/remove/autoremove 等)は空集合を返す。
/// `pkg=version` 形式のバージョン指定は名前部分だけを残す。フラグ
/// (`-y` 等)は無視する。
fn parse_install_commandline(cmd: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.first() == Some(&"sudo") {
        tokens.remove(0);
    }
    let Some(bin) = tokens.first() else {
        return names;
    };
    if !(bin.ends_with("apt") || bin.ends_with("apt-get")) {
        return names;
    }
    if tokens.get(1) != Some(&"install") {
        return names;
    }
    for tok in &tokens[2..] {
        if tok.starts_with('-') {
            continue;
        }
        let name = tok.split('=').next().unwrap_or(tok);
        if !name.is_empty() {
            names.insert(name.to_string());
        }
    }
    names
}

/// `/var/log/apt/history.log*`(複数世代、展開済みテキストとして連結した
/// もの)から、**人間が対話的に打った** `apt`/`apt-get install <pkg...>`
/// で明示的に入れたパッケージ名を復元する(#445)。
///
/// ブロック(`Start-Date:` 〜 `End-Date:`)ごとに `Requested-By:` 行の
/// 有無を見る。apt 自身が `Requested-By:` を書くのは `SUDO_UID` /
/// `PKEXEC_UID` / `PACKAGEKIT_CALLER_UID` のいずれかが設定されている
/// ときだけ(apt upstream `apt-pkg/deb/dpkgpm.cc` の
/// `AptHistoryRequestingUser()`)。Pop!_OS の `distinst` post-install や
/// `pop-upgrade` は root の非対話プロセスとして直接 `apt-get` を呼ぶため
/// これらの環境変数を持たず、`Requested-By:` が書かれない — 結果として
/// この関数はそれらの seed install を「人間の ad-hoc」から除外できる
/// (対話シェルからの `sudo apt install` は `SUDO_UID` が立つため区別
/// できる)。`Requested-By:` を持たないブロックは無視する。
pub fn parse_history_log_installs(text: &str) -> BTreeSet<String> {
    fn flush(cmd: Option<&str>, requested_by: bool, out: &mut BTreeSet<String>) {
        if requested_by {
            if let Some(cmd) = cmd {
                out.extend(parse_install_commandline(cmd));
            }
        }
    }

    let mut names = BTreeSet::new();
    let mut block_cmd: Option<&str> = None;
    let mut block_requested_by = false;

    for line in text.lines() {
        if let Some(cmd) = line.strip_prefix("Commandline: ") {
            block_cmd = Some(cmd);
        } else if line.starts_with("Requested-By:") {
            block_requested_by = true;
        } else if line.starts_with("End-Date:") {
            flush(block_cmd, block_requested_by, &mut names);
            block_cmd = None;
            block_requested_by = false;
        }
    }
    // 末尾のブロックが `End-Date:` を欠いたまま切れている場合(logrotate
    // の境界等)も拾う。
    flush(block_cmd, block_requested_by, &mut names);

    names
}

/// `--from-history`(#445): 既存ホストの baseline を、現在の
/// `apt-mark showmanual` から history.log 由来の ad-hoc install 集合を
/// 除いたものとして復元する。
pub fn compute_baseline_from_history(
    installed: &BTreeSet<String>,
    ad_hoc: &BTreeSet<String>,
) -> BTreeSet<String> {
    installed.difference(ad_hoc).cloned().collect()
}

/// apt の更新遅れのしきい値(日)の既定(#1053)。
///
/// 根拠(運用判断で、一次資料ではない): #1053 の実測で、vega は最後の `apt full-upgrade`
/// (2025-11-27)から 335 日で 1141 件が更新待ちだった(1 日あたり約 3.4 件)。90 日(四半期)
/// なら約 300 件で、`docs/operations.md` の手順(前後の確認・再起動)でまとめて上げられる量に
/// 収まる。さらに短くすると、更新を急ぐ必要のない週にも毎週起票が積まれ、長くすると
/// COSMIC(1.0 前のベータ)・kernel・Mesa の修正が数か月届かない。`DETECT_DRIFT_APT_STALE_DAYS`
/// で上書きできる。
pub const APT_STALE_DAYS_DEFAULT: i64 = 90;

/// 1970-01-01 からの日数(Howard Hinnant の days_from_civil)。UTC の暦日。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// [`days_from_civil`] の逆(`YYYY-MM-DD`)。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn format_ymd(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `Start-Date: 2025-11-27  10:00:00` の日付部分(UTC でなくホストの現地日付。日単位の
/// しきい値には影響しない)。
fn parse_start_date(rest: &str) -> Option<i64> {
    let date = rest.split_whitespace().next()?;
    let mut it = date.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

/// `Commandline:` 1 行が、システム全体の更新(`apt`/`apt-get`/`aptitude` の `upgrade`・
/// `full-upgrade`・`dist-upgrade`・`safe-upgrade`、PackageKit の `update-packages`)か。
/// `apt install pop-upgrade` のように、パッケージ名に `upgrade` を含むだけのものは含まない
/// (サブコマンドだけを見る)。`sudo` 経由・パス付きの実行ファイルも同じに見る。
pub fn is_upgrade_commandline(cmd: &str) -> bool {
    let mut tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.first() == Some(&"sudo") {
        tokens.remove(0);
    }
    let Some(bin) = tokens.first() else {
        return false;
    };
    // PackageKit(Pop!_Shop 等の GUI の更新): `packagekit role='update-packages'`。
    // 実機の history.log には install-packages の実例だけがあり、update-packages は
    // PackageKit の role 名(`PK_ROLE_ENUM_UPDATE_PACKAGES`)からの推定(未確認)。
    if *bin == "packagekit" {
        return tokens.iter().any(|t| t.contains("role='update-packages'"));
    }
    let name = bin.rsplit('/').next().unwrap_or(bin);
    if !matches!(name, "apt" | "apt-get" | "aptitude") {
        return false;
    }
    // サブコマンド = 最初の非オプション。`-o`・`-c`・`-t` は次の語を引数に取る。
    let mut it = tokens[1..].iter();
    while let Some(t) = it.next() {
        if matches!(*t, "-o" | "-c" | "-t") {
            it.next();
            continue;
        }
        if t.starts_with('-') {
            continue;
        }
        return matches!(
            *t,
            "upgrade" | "full-upgrade" | "dist-upgrade" | "safe-upgrade"
        );
    }
    false
}

/// `/var/log/apt/history.log*`(展開済みを連結したもの)から読んだ、更新の履歴。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AptUpdateHistory {
    /// 最後の upgrade 系の実行の日(1970-01-01 からの日数)。
    pub last_upgrade: Option<i64>,
    /// 履歴に残っている最古のブロックの日。`None` は履歴が空(読めない)。
    pub oldest: Option<i64>,
}

pub fn parse_history_log_updates(text: &str) -> AptUpdateHistory {
    let mut h = AptUpdateHistory::default();
    let mut block_date: Option<i64> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Start-Date:") {
            block_date = parse_start_date(rest);
            if let Some(d) = block_date {
                h.oldest = Some(h.oldest.map_or(d, |o| o.min(d)));
            }
        } else if let Some(cmd) = line.strip_prefix("Commandline: ") {
            if let (Some(d), true) = (block_date, is_upgrade_commandline(cmd)) {
                h.last_upgrade = Some(h.last_upgrade.map_or(d, |l| l.max(d)));
            }
        }
    }
    h
}

/// apt-stale レイヤー: 最後の upgrade 系の実行が `threshold_days` より古ければ 1 行の
/// 報告を返す。履歴に upgrade の記録が無いときは、履歴の最古が `threshold_days` より古い
/// とき(それだけの期間、記録が無い)だけ報告する — 入れたばかりのホストは報告しない。
/// 履歴が空(`oldest` が `None`)なら、判定の材料が無いので clean(呼び出し側がレイヤーごと
/// skip する)。`today` は 1970-01-01 からの日数。
pub fn diff_apt_stale(h: &AptUpdateHistory, today: i64, threshold_days: i64) -> LayerDrift {
    let mut lines = Vec::new();
    match (h.last_upgrade, h.oldest) {
        (Some(last), _) if today - last > threshold_days => lines.push(format!(
            "最後の apt upgrade / full-upgrade は {}({} 日前、しきい値 {threshold_days} 日)",
            format_ymd(last),
            today - last
        )),
        (None, Some(oldest)) if today - oldest > threshold_days => lines.push(format!(
            "history.log の範囲({} 以降)に apt upgrade / full-upgrade の記録が無い(しきい値 {threshold_days} 日)",
            format_ymd(oldest)
        )),
        _ => {}
    }
    LayerDrift {
        layer: "apt-stale",
        kind: DriftKind::Stale,
        undeclared: lines,
    }
}

/// `cargo install --list` の出力を解析し、パッケージ名の集合を返す。
/// 形式は `<pkgname> v<version>:` の見出し行に続けて、インデントされた
/// バイナリ名の行が並ぶ(`<pkgname> v<version> (<path>):` の派生形もある
/// — git/path 由来のインストールで括弧内にソース情報が付く)。バイナリ名
/// 自体ではなくパッケージ名だけを拾う: 見出し行は行頭が空白でなく `:` で
/// 終わる行。
pub fn parse_cargo_installed(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| !l.starts_with(char::is_whitespace))
        .filter_map(|l| {
            let l = l.trim_end();
            let l = l.strip_suffix(':')?;
            // "<name> v<ver>" または "<name> v<ver> (<path>)" の先頭トークン。
            let name = l.split_whitespace().next()?;
            Some(name.to_string())
        })
        .collect()
}

/// cargo レイヤーの diff: この repo は cargo install に対する宣言ファイルを
/// 持たない(Issue #4 の設計どおり)ので、インストール済み全件が候補。
pub fn diff_cargo(installed: &BTreeSet<String>) -> LayerDrift {
    LayerDrift::undeclared("cargo", installed.iter().cloned().collect())
}

/// `npm ls -g --depth=0 --json` の出力から `.dependencies` のキー(パッケージ
/// 名)を抽出する。`npm` 自身は常に存在し ad-hoc install の対象ではないため
/// 除外する。`corepack` も同様に除外する(#3) — Node 配布物(mise の node
/// tarball 等、ADR-0002)に同梱される同梱物であり、`npm -g` の一覧に載る
/// ものの人間が ad-hoc install したものではない。壊れた JSON は空集合を
/// 返す(呼び出し側が WARN として扱う)。
pub fn parse_npm_global(json_text: &str) -> BTreeSet<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return BTreeSet::new();
    };
    let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) else {
        return BTreeSet::new();
    };
    deps.keys()
        .filter(|k| !matches!(k.as_str(), "npm" | "corepack"))
        .cloned()
        .collect()
}

pub fn diff_npm(installed: &BTreeSet<String>) -> LayerDrift {
    LayerDrift::undeclared("npm", installed.iter().cloned().collect())
}

/// `pipx list --json` の出力から `.venvs` のキー(パッケージ名)を抽出する。
pub fn parse_pipx_venvs(json_text: &str) -> BTreeSet<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return BTreeSet::new();
    };
    let Some(venvs) = v.get("venvs").and_then(|d| d.as_object()) else {
        return BTreeSet::new();
    };
    venvs.keys().cloned().collect()
}

pub fn diff_pipx(installed: &BTreeSet<String>) -> LayerDrift {
    LayerDrift::undeclared("pipx", installed.iter().cloned().collect())
}

/// ADR-0025 対象(update-own-tools のホストローカルレジストリに載る
/// pre-release CLI 名)を drift レイヤーから除外する。cargo/npm/pipx
/// レイヤーはインストール済み全件を候補にするため、そこに ADR-0025 対象が
/// 混ざりうる — この名前を GitHub Issue へ書くことは ADR-0025 が無条件で
/// 禁じている(「存在すら書かない」)。除外は `--file-issue`(main.rs)経由の
/// 起票本文の組み立てでのみ使う。通常の(GitHub に出ない)ターミナル出力は
/// この関数を通さない — ホストローカルな自分の端末で自分のレジストリの
/// 中身を見ることは ADR-0025 の対象外。
pub fn filter_registry_excluded(
    drifts: &[LayerDrift],
    registry: &update_own_tools::Registry,
) -> (Vec<LayerDrift>, usize) {
    let registered: BTreeSet<&str> = registry.tools.iter().map(|t| t.name.as_str()).collect();
    let mut excluded_count = 0usize;
    let filtered = drifts
        .iter()
        .map(|d| {
            let kept: Vec<String> = d
                .undeclared
                .iter()
                .filter(|name| {
                    let is_registered = registered.contains(name.as_str());
                    if is_registered {
                        excluded_count += 1;
                    }
                    !is_registered
                })
                .cloned()
                .collect();
            LayerDrift {
                layer: d.layer,
                kind: d.kind,
                undeclared: kept,
            }
        })
        .collect();
    (filtered, excluded_count)
}

/// `--file-issue` の結果。起票そのものを要求しなかった(CLI 単独実行)か、
/// 配信(起票・コメント追記・報告対象ゼロによる無配信)に成功したか、
/// 配信に失敗したか — 3 択の閉じた語彙にすることで、`exit_code` が
/// 未定義の組み合わせを扱わずに済む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIssueOutcome {
    NotRequested,
    Delivered,
    Failed,
}

/// systemd/launchd の oneshot unit が「成功」と「要修復」のどちらに終わる
/// べきかを決める純粋関数。cargo/npm/pipx レイヤーは宣言ファイルを持たない
/// ため在庫全件が drift 扱いになり(#4 の設計)、`--file-issue` 運用では
/// drift の有無そのものを exit code の失敗条件にすると恒久的に failed に
/// なる(2026-09-24 の実機観測、hms が degraded と報告した原因)。
/// `--file-issue` を要求していない CLI 単独実行では、従来どおり
/// drift の有無を報告する(`detect-drift`/`detect-drift --porcelain` の
/// 既存契約を維持)。
///
/// 戻り値: 0 = clean、1 = drift(CLI 契約) / 0 = 配信済み、3 = 起票失敗
/// (`--file-issue` 契約。2 は usage error で既に main.rs が使用中)。
pub fn exit_code(any_drift: bool, outcome: FileIssueOutcome) -> u8 {
    match outcome {
        FileIssueOutcome::NotRequested => u8::from(any_drift),
        FileIssueOutcome::Delivered => 0,
        FileIssueOutcome::Failed => 3,
    }
}

/// `gh repo view <repo> --json visibility --jq .visibility` の出力が、
/// ad-hoc install の在庫(machine-state の実値)を書き込んでよい宛先かを
/// 判定する。PRIVATE のみ許可する fail-closed な純関数 —
/// ADR-0034「規則は public・実値は private」により、`--file-issue` の
/// 宛先は private リポジトリに限る。判定不能な入力(取得失敗・
/// INTERNAL・空文字列)はすべて拒否側に倒す。
pub fn visibility_permits_filing(visibility: &str) -> bool {
    visibility.trim() == "PRIVATE"
}

/// 自動起票する GitHub Issue の title/body を組み立てる(純粋関数、
/// 副作用なし)。`drifts` は `filter_registry_excluded` を通した後のもの
/// でなければならない — この関数自身は ADR-0025 のフィルタを行わない。
pub fn compose_issue_report(
    hostname: &str,
    drifts: &[LayerDrift],
    excluded_count: usize,
) -> String {
    let has_undeclared = drifts
        .iter()
        .any(|d| d.kind == DriftKind::Undeclared && !d.is_clean());
    let mut body = String::new();
    if has_undeclared {
        body.push_str(&format!(
            "`detect-drift` が {hostname} で宣言外の ad-hoc install を検出しました。\n\n"
        ));
    } else {
        body.push_str(&format!(
            "`detect-drift` が {hostname} で次を検出しました。\n\n"
        ));
    }
    for d in drifts {
        if d.undeclared.is_empty() {
            continue;
        }
        body.push_str(&format!("## {}\n\n", d.layer));
        for name in &d.undeclared {
            match d.kind {
                DriftKind::Undeclared => body.push_str(&format!("- `{name}`\n")),
                DriftKind::Stale => body.push_str(&format!("- {name}\n")),
            }
        }
        if d.kind == DriftKind::Stale {
            body.push_str(
                "\n手順(`sudo apt full-upgrade` の前後に読むもの、再起動、held パッケージ)は \
                 `docs/operations.md` の「システム層(apt)の定期更新」を参照してください。\n",
            );
        }
        body.push('\n');
    }
    if excluded_count > 0 {
        body.push_str(&format!(
            "({excluded_count} 件は update-own-tools のホストローカルレジストリに登録済みのため、ADR-0025 によりここには列挙していません。)\n\n"
        ));
    }
    if has_undeclared {
        body.push_str(
            "宣言(home-manager / packages/declarative/apt-packages.txt)へ取り込むか、\
             escape hatch として残すかは人間の判断です(Issue #4 の分類ルール)。\n",
        );
    }
    body
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use update_own_tools::parse_registry;

    fn drift(layer: &'static str, names: &[&str]) -> LayerDrift {
        LayerDrift::undeclared(layer, names.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn filter_registry_excluded_removes_registered_tools() {
        let registry = parse_registry(
            r#"
            [[tool]]
            name = "telepath"
            repo = "/home/x/.ghr/example/telepath"
            "#,
        )
        .unwrap();
        let drifts = vec![drift("cargo", &["telepath", "bat", "starship"])];
        let (filtered, excluded) = filter_registry_excluded(&drifts, &registry);
        assert_eq!(excluded, 1);
        assert_eq!(
            filtered[0].undeclared,
            vec!["bat".to_string(), "starship".to_string()]
        );
    }

    #[test]
    fn filter_registry_excluded_noop_on_empty_registry() {
        let registry = parse_registry("").unwrap();
        let drifts = vec![drift("cargo", &["bat"])];
        let (filtered, excluded) = filter_registry_excluded(&drifts, &registry);
        assert_eq!(excluded, 0);
        assert_eq!(filtered[0].undeclared, vec!["bat".to_string()]);
    }

    #[test]
    fn compose_issue_report_lists_layers_and_notes_exclusions() {
        let drifts = vec![
            drift("apt", &["pandoc"]),
            drift("cargo", &[]),
            drift("npm", &["typescript"]),
        ];
        let body = compose_issue_report("vega", &drifts, 2);
        assert!(body.contains("vega"));
        assert!(body.contains("## apt"));
        assert!(body.contains("`pandoc`"));
        assert!(!body.contains("## cargo")); // empty layer omitted
        assert!(body.contains("## npm"));
        assert!(body.contains("`typescript`"));
        assert!(body.contains("2 件は"));
    }

    #[test]
    fn compose_issue_report_omits_exclusion_note_when_zero() {
        let drifts = vec![drift("apt", &["pandoc"])];
        let body = compose_issue_report("vega", &drifts, 0);
        assert!(!body.contains("ADR-0025"));
    }

    #[test]
    fn exit_code_not_requested_reports_drift_presence() {
        assert_eq!(exit_code(false, FileIssueOutcome::NotRequested), 0);
        assert_eq!(exit_code(true, FileIssueOutcome::NotRequested), 1);
    }

    #[test]
    fn exit_code_delivered_is_always_success_regardless_of_drift() {
        assert_eq!(exit_code(false, FileIssueOutcome::Delivered), 0);
        assert_eq!(exit_code(true, FileIssueOutcome::Delivered), 0);
    }

    #[test]
    fn exit_code_failed_is_always_nonzero_regardless_of_drift() {
        assert_eq!(exit_code(false, FileIssueOutcome::Failed), 3);
        assert_eq!(exit_code(true, FileIssueOutcome::Failed), 3);
    }

    #[test]
    fn visibility_permits_filing_only_for_private() {
        assert!(visibility_permits_filing("PRIVATE"));
    }

    #[test]
    fn visibility_permits_filing_rejects_public_internal_and_empty() {
        assert!(!visibility_permits_filing("PUBLIC"));
        assert!(!visibility_permits_filing("INTERNAL"));
        assert!(!visibility_permits_filing(""));
    }

    #[test]
    fn visibility_permits_filing_trims_surrounding_whitespace() {
        assert!(visibility_permits_filing("PRIVATE\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apt_declared_skips_comments_and_blanks() {
        let text = "# header\nzsh\n\nbuild-essential\n# --- section ---\npkg-config\n";
        let got = parse_apt_declared(text);
        assert_eq!(
            got,
            BTreeSet::from([
                "zsh".to_string(),
                "build-essential".to_string(),
                "pkg-config".to_string(),
            ])
        );
    }

    #[test]
    fn diff_apt_reports_only_undeclared() {
        let declared = BTreeSet::from(["zsh".to_string(), "jq".to_string()]);
        let installed = BTreeSet::from(["zsh".to_string(), "jq".to_string(), "pandoc".to_string()]);
        let drift = diff_apt(&declared, &installed, &BTreeSet::new());
        assert_eq!(drift.undeclared, vec!["pandoc".to_string()]);
        assert!(!drift.is_clean());
    }

    #[test]
    fn diff_apt_clean_when_no_extra_installed() {
        let declared = BTreeSet::from(["zsh".to_string()]);
        let installed = BTreeSet::from(["zsh".to_string()]);
        let drift = diff_apt(&declared, &installed, &BTreeSet::new());
        assert!(drift.is_clean());
    }

    #[test]
    fn diff_apt_excludes_baseline_seed_packages() {
        // #445: Pop!_OS の初期 seed(baseline)は showmanual に出るが drift
        // ではない。宣言にも baseline にも無いものだけが drift。
        let declared = BTreeSet::from(["zsh".to_string()]);
        let installed = BTreeSet::from([
            "zsh".to_string(),
            "pop-desktop".to_string(), // baseline 由来(seed)
            "pandoc".to_string(),      // ad-hoc(真の drift)
        ]);
        let baseline = BTreeSet::from(["pop-desktop".to_string()]);
        let drift = diff_apt(&declared, &installed, &baseline);
        assert_eq!(drift.undeclared, vec!["pandoc".to_string()]);
    }

    #[test]
    fn diff_apt_baseline_takes_priority_over_declaration_gap() {
        // baseline に載っているだけで宣言が無くても drift にしない
        // (baseline 自体が「宣言していない既知の seed」を表現するため)。
        let declared = BTreeSet::new();
        let installed = BTreeSet::from(["pop-desktop".to_string()]);
        let baseline = BTreeSet::from(["pop-desktop".to_string()]);
        let drift = diff_apt(&declared, &installed, &baseline);
        assert!(drift.is_clean());
    }

    #[test]
    fn parse_apt_baseline_same_format_as_installed() {
        let text = "pop-desktop\n\ngnome-terminal\n";
        let got = parse_apt_baseline(text);
        assert_eq!(
            got,
            BTreeSet::from(["pop-desktop".to_string(), "gnome-terminal".to_string()])
        );
    }

    #[test]
    fn parse_apt_baseline_skips_comment_blocks() {
        // #3: 一括インストールスクリプト由来の lib 群を理由コメント付き
        // ブロックとして baseline に残せることの確認。
        let text = "\
# 2026-01-14 playwright install-deps
libasound2t64
libatk1.0-0t64

# 2025-12-23 一括(cabextract/samba/winbind/spacenavd 等)
cabextract
samba
";
        let got = parse_apt_baseline(text);
        assert_eq!(
            got,
            BTreeSet::from([
                "libasound2t64".to_string(),
                "libatk1.0-0t64".to_string(),
                "cabextract".to_string(),
                "samba".to_string(),
            ])
        );
    }

    #[test]
    fn parse_history_log_installs_extracts_install_targets_when_requested_by_present() {
        // apt は SUDO_UID/PKEXEC_UID/PACKAGEKIT_CALLER_UID のいずれかが
        // 立っているときだけ Requested-By: を書く(対話的な人間の実行)。
        let text = "\
Start-Date: 2026-08-01  10:00:00
Commandline: apt install gh pandoc
Requested-By: tarotene (1000)
Install: gh:amd64 (2.0.0), pandoc:amd64 (3.0.0)
End-Date: 2026-08-01  10:00:05

Start-Date: 2026-08-02  09:00:00
Commandline: sudo apt-get install -y solaar=1.1.10-1
Requested-By: tarotene (1000)
Install: solaar:amd64 (1.1.10-1)
End-Date: 2026-08-02  09:00:02

Start-Date: 2026-08-03  08:00:00
Commandline: apt-get autoremove
Requested-By: tarotene (1000)
End-Date: 2026-08-03  08:00:01

Start-Date: 2026-08-04  07:00:00
Commandline: apt upgrade
Requested-By: tarotene (1000)
End-Date: 2026-08-04  07:01:00
";
        let got = parse_history_log_installs(text);
        assert_eq!(
            got,
            BTreeSet::from(["gh".to_string(), "pandoc".to_string(), "solaar".to_string(),])
        );
    }

    #[test]
    fn parse_history_log_installs_empty_on_no_install_commands() {
        let text = "Start-Date: 2026-08-01\nCommandline: apt update\nRequested-By: tarotene (1000)\nEnd-Date: 2026-08-01\n";
        assert_eq!(parse_history_log_installs(text), BTreeSet::new());
    }

    #[test]
    fn parse_history_log_installs_excludes_blocks_without_requested_by() {
        // #445/#3 の追加要件: distinst の post-install や pop-upgrade は
        // root の非対話プロセスとして apt-get を直接呼ぶため
        // Requested-By: を持たない — これは seed install であり drift
        // 候補にしない(見た目は同じ `apt-get install` でも人間が打った
        // ものではないため)。
        let text = "\
Start-Date: 2026-08-01  10:00:00
Commandline: apt-get install -y pop-desktop cosmic-term
Install: pop-desktop:amd64 (1.0), cosmic-term:amd64 (1.0)
End-Date: 2026-08-01  10:00:05

Start-Date: 2026-08-02  09:00:00
Commandline: apt install gh
Requested-By: tarotene (1000)
Install: gh:amd64 (2.0.0)
End-Date: 2026-08-02  09:00:02
";
        let got = parse_history_log_installs(text);
        assert_eq!(got, BTreeSet::from(["gh".to_string()]));
    }

    #[test]
    fn parse_history_log_installs_flushes_trailing_block_without_end_date() {
        // logrotate の境界などで末尾ブロックが End-Date: を欠くケース。
        let text = "\
Start-Date: 2026-08-01  10:00:00
Commandline: apt install gh
Requested-By: tarotene (1000)
Install: gh:amd64 (2.0.0)
";
        let got = parse_history_log_installs(text);
        assert_eq!(got, BTreeSet::from(["gh".to_string()]));
    }

    #[test]
    fn compute_baseline_from_history_subtracts_ad_hoc_installs() {
        let installed = BTreeSet::from([
            "zsh".to_string(),
            "pop-desktop".to_string(),
            "gh".to_string(),
        ]);
        let ad_hoc = BTreeSet::from(["gh".to_string()]);
        let baseline = compute_baseline_from_history(&installed, &ad_hoc);
        assert_eq!(
            baseline,
            BTreeSet::from(["zsh".to_string(), "pop-desktop".to_string()])
        );
    }

    #[test]
    fn parse_cargo_installed_extracts_package_names() {
        let text = "\
alacritty v0.15.1:
    alacritty
cargo-flash v0.24.0:
    cargo-flash
    cargo-embed
ghr v1.2.3 (/home/x/.ghr/some/path):
    ghr
";
        let got = parse_cargo_installed(text);
        assert_eq!(
            got,
            BTreeSet::from([
                "alacritty".to_string(),
                "cargo-flash".to_string(),
                "ghr".to_string(),
            ])
        );
    }

    #[test]
    fn parse_cargo_installed_empty_on_no_packages() {
        assert_eq!(parse_cargo_installed(""), BTreeSet::new());
    }

    #[test]
    fn diff_cargo_reports_all_installed() {
        let installed = BTreeSet::from(["bat".to_string(), "starship".to_string()]);
        let drift = diff_cargo(&installed);
        assert_eq!(
            drift.undeclared,
            vec!["bat".to_string(), "starship".to_string()]
        );
    }

    #[test]
    fn parse_npm_global_extracts_deps_excluding_npm_itself() {
        let json = r#"{
          "dependencies": {
            "npm": {"version": "10.0.0"},
            "typescript": {"version": "5.0.0"},
            "eslint": {"version": "8.0.0"}
          }
        }"#;
        let got = parse_npm_global(json);
        assert_eq!(
            got,
            BTreeSet::from(["typescript".to_string(), "eslint".to_string()])
        );
    }

    #[test]
    fn parse_npm_global_excludes_corepack_as_node_bundled() {
        // #3: corepack は mise 配布の node tarball に同梱される(ADR-0002)
        // ため、npm -g の一覧には載るが ad-hoc install ではない。
        let json = r#"{
          "dependencies": {
            "npm": {"version": "10.0.0"},
            "corepack": {"version": "0.34.1"},
            "backlog.md": {"version": "1.50.1"}
          }
        }"#;
        let got = parse_npm_global(json);
        assert_eq!(got, BTreeSet::from(["backlog.md".to_string()]));
    }

    #[test]
    fn parse_npm_global_empty_dependencies() {
        let json = r#"{"dependencies": {}}"#;
        assert_eq!(parse_npm_global(json), BTreeSet::new());
    }

    #[test]
    fn parse_npm_global_malformed_json_returns_empty() {
        assert_eq!(parse_npm_global("not json"), BTreeSet::new());
    }

    #[test]
    fn parse_pipx_venvs_extracts_keys() {
        let json = r#"{"venvs": {"black": {}, "ruff": {}}}"#;
        let got = parse_pipx_venvs(json);
        assert_eq!(
            got,
            BTreeSet::from(["black".to_string(), "ruff".to_string()])
        );
    }

    #[test]
    fn parse_pipx_venvs_malformed_json_returns_empty() {
        assert_eq!(parse_pipx_venvs("{{{"), BTreeSet::new());
    }

    #[test]
    fn diff_npm_and_pipx_report_all_installed() {
        let npm_installed = BTreeSet::from(["typescript".to_string()]);
        assert_eq!(
            diff_npm(&npm_installed).undeclared,
            vec!["typescript".to_string()]
        );
        let pipx_installed = BTreeSet::from(["black".to_string()]);
        assert_eq!(
            diff_pipx(&pipx_installed).undeclared,
            vec!["black".to_string()]
        );
    }

    // ---- apt-stale(#1053) ----

    const HISTORY: &str = "\
Start-Date: 2025-10-18  10:00:00
Commandline: apt upgrade
Requested-By: tarotene (1000)
Upgrade: foo:amd64 (1, 2)
End-Date: 2025-10-18  10:05:00

Start-Date: 2025-11-21  23:21:28
Commandline: apt install ros-jazzy-desktop
Requested-By: tarotene (1000)
Install: bar:amd64 (1, automatic)
End-Date: 2025-11-21  23:25:05

Start-Date: 2025-11-27  08:00:00
Commandline: apt full-upgrade
Requested-By: tarotene (1000)
Upgrade: foo:amd64 (2, 3)
End-Date: 2025-11-27  08:30:00

Start-Date: 2026-01-13  09:00:00
Commandline: apt-get -y -o Dpkg::Options::=--force-confdef install pop-upgrade libpop-upgrade-gtk
Install: pop-upgrade:amd64 (1)
End-Date: 2026-01-13  09:00:09
";

    fn day(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d)
    }

    #[test]
    fn civil_days_roundtrip_and_known_values() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for (y, m, d) in [
            (2024, 2, 29),
            (2025, 11, 27),
            (2026, 10, 27),
            (1999, 12, 31),
        ] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        assert_eq!(format_ymd(day(2025, 11, 27)), "2025-11-27");
        assert_eq!(day(2026, 10, 26) - day(2025, 11, 27), 333);
    }

    #[test]
    fn upgrade_commandlines_are_recognised_by_subcommand_only() {
        for cmd in [
            "apt upgrade",
            "apt full-upgrade",
            "apt-get upgrade -y --allow-downgrades",
            "sudo apt-get -y dist-upgrade",
            "/usr/bin/apt full-upgrade",
            "aptitude safe-upgrade",
            "apt -o Dpkg::Options::=--force-confold full-upgrade",
            "packagekit role='update-packages'",
        ] {
            assert!(is_upgrade_commandline(cmd), "{cmd}");
        }
        for cmd in [
            "apt install gh",
            "apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold --allow-downgrades install pop-upgrade libpop-upgrade-gtk",
            "apt-get purge -y ubuntu-minimal unattended-upgrades yaru-theme-gnome-shell",
            "apt list --upgradable",
            "packagekit role='install-packages'",
            "dpkg -i foo.deb",
            "",
        ] {
            assert!(!is_upgrade_commandline(cmd), "{cmd}");
        }
    }

    #[test]
    fn history_gives_last_upgrade_and_oldest_block() {
        let h = parse_history_log_updates(HISTORY);
        assert_eq!(h.last_upgrade, Some(day(2025, 11, 27)));
        assert_eq!(h.oldest, Some(day(2025, 10, 18)));
        assert_eq!(parse_history_log_updates(""), AptUpdateHistory::default());
    }

    #[test]
    fn old_upgrade_is_stale_with_a_one_line_report() {
        let h = parse_history_log_updates(HISTORY);
        let d = diff_apt_stale(&h, day(2026, 10, 26), 90);
        assert_eq!(d.layer, "apt-stale");
        assert_eq!(d.kind, DriftKind::Stale);
        assert_eq!(d.undeclared.len(), 1);
        assert!(d.undeclared[0].contains("2025-11-27"), "{:?}", d.undeclared);
        assert!(d.undeclared[0].contains("333 日前"), "{:?}", d.undeclared);
        assert!(
            d.undeclared[0].contains("しきい値 90 日"),
            "{:?}",
            d.undeclared
        );
    }

    #[test]
    fn upgrade_within_the_threshold_is_clean() {
        let h = parse_history_log_updates(HISTORY);
        assert!(diff_apt_stale(&h, day(2025, 11, 27) + 90, 90).is_clean());
        assert!(!diff_apt_stale(&h, day(2025, 11, 27) + 91, 90).is_clean());
    }

    #[test]
    fn no_upgrade_record_is_stale_only_when_the_log_is_old_enough_to_say_so() {
        let installs_only = "Start-Date: 2026-09-06  09:56:54\nCommandline: apt install gh\nEnd-Date: 2026-09-06  09:57:00\n";
        let h = parse_history_log_updates(installs_only);
        assert_eq!(h.last_upgrade, None);
        // 入れて 50 日: まだ判断できない。
        assert!(diff_apt_stale(&h, day(2026, 10, 26), 90).is_clean());
        // 100 日: それだけの期間、記録が無い。
        let d = diff_apt_stale(&h, day(2026, 12, 15), 90);
        assert_eq!(d.undeclared.len(), 1);
        assert!(d.undeclared[0].contains("記録が無い"), "{:?}", d.undeclared);
        // 履歴が空なら、判定の材料が無いので clean。
        assert!(diff_apt_stale(&AptUpdateHistory::default(), day(2026, 10, 26), 90).is_clean());
    }

    #[test]
    fn stale_report_is_a_plain_line_and_points_at_the_runbook() {
        let h = parse_history_log_updates(HISTORY);
        let stale = diff_apt_stale(&h, day(2026, 10, 26), 90);
        let body = compose_issue_report("vega", std::slice::from_ref(&stale), 0);
        assert!(body.contains("## apt-stale"));
        assert!(body.contains("- 最後の apt upgrade / full-upgrade は 2025-11-27"));
        assert!(
            !body.contains("`最後の"),
            "名前ではないので code span にしない"
        );
        assert!(body.contains("docs/operations.md"));
        assert!(!body.contains("ad-hoc install を検出"), "{body}");
        // 宣言外の在庫と並ぶときは、従来の見出し・結びが残る。
        let body = compose_issue_report(
            "vega",
            &[
                LayerDrift::undeclared("apt", vec!["pandoc".to_string()]),
                stale,
            ],
            0,
        );
        assert!(body.contains("ad-hoc install を検出"));
        assert!(body.contains("`pandoc`") && body.contains("## apt-stale"));
    }

    #[test]
    fn filter_registry_excluded_keeps_the_kind() {
        let registry = update_own_tools::parse_registry("").unwrap();
        let h = parse_history_log_updates(HISTORY);
        let stale = diff_apt_stale(&h, day(2026, 10, 26), 90);
        let (filtered, excluded) = filter_registry_excluded(&[stale], &registry);
        assert_eq!(excluded, 0);
        assert_eq!(filtered[0].kind, DriftKind::Stale);
        assert_eq!(filtered[0].undeclared.len(), 1);
    }
}
