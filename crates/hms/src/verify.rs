//! 適用の前後で世代を照合する(#1025)。
//!
//! home-manager の switch は、build した世代を `nix-env --profile … --set` で
//! profile に載せてから activation script を走らせるだけで、比較して置き換える
//! (CAS)ことも、他の適用を排他することも無い(上流 `home-manager` の `doSwitch`:
//! `nix build --out-link` → `nix-env --set` → `activate`)。別の worktree や
//! セッションの適用(ロックを持たない古い `hms`、手で実行した `home-manager
//! switch` を含む)がその間に割り込むと、`switch` が成功を返したあとでも profile と
//! current-home は別の世代を指す。switch の終了コードは「今回の世代が有効か」の
//! 証拠にならないので、hms は次の 2 点を自分で確かめる。
//!
//! 1. switch の前に、今回の目標の世代(`activationPackage` の store path)を
//!    `nix eval --raw` で求める。home-manager が build する derivation と同じ
//!    attribute なので、入力が同じなら同じ store path になる。
//! 2. switch が成功した直後に、profile と current-home の両方が目標を指すか照合する。
//!    指さなければ非 0 で終え、適用元・実際の世代・次の操作を示す。
//!
//! 目標の世代の取り方として `home-manager build`(`./result` を作る)や、
//! `nix build --print-out-paths` で先に build して `activate` を直接呼ぶ案も比べた。
//! 後者は switch の実体(backup 拡張子の環境変数、`nix-env --set`、gen-version の
//! 判定)を hms が再実装することになり、適用契約そのものを変える。`nix eval` は
//! switch をそのまま使い、読むだけで済む。

use super::{home, readlink_f};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn profile_link() -> PathBuf {
    home().join(".local/state/nix/profiles/home-manager")
}

fn current_home_link() -> PathBuf {
    home().join(".local/state/home-manager/gcroots/current-home")
}

/// 今の profile が指す世代の store path(無ければ空)。
pub fn current_profile_generation() -> String {
    readlink_f(&profile_link())
}

/// 世代の dotfiles rev(`home-files/.local/share/dotfiles/rev` の 1 行目)。
/// path input の適用は "unknown …" と書かれる(flake.nix)。読めなければ None。
fn generation_rev_of(generation: &str) -> Option<String> {
    if generation.is_empty() {
        return None;
    }
    let text =
        std::fs::read_to_string(Path::new(generation).join("home-files/.local/share/dotfiles/rev"))
            .ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// `nix flake metadata --json` の、適用元の rev。clean なら `revision`、
/// dirty なら `dirtyRevision`(`hms .` は大抵こちら)。
pub fn source_revision(meta_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(meta_json).ok()?;
    ["revision", "dirtyRevision"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|r| r.as_str()))
        .filter(|r| !r.is_empty())
        .map(str::to_string)
}

/// `nix eval --raw` の出力から store path を取り出す。絶対パスでなければ None。
fn parse_target(stdout: &str) -> Option<String> {
    let t = stdout.trim();
    t.starts_with('/').then(|| t.to_string())
}

/// 照合の結果。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Verdict {
    /// profile も current-home も目標を指す。
    Active,
    /// profile が目標を指さない(別の適用が世代を進めた・戻した)。
    ProfileElsewhere,
    /// profile は目標を指すが、current-home が目標を指さない(別の適用の
    /// activation が途中、または今回の activation が反映されていない)。
    HomeNotCurrent,
}

pub fn judge(target: &str, profile: &str, current_home: &str) -> Verdict {
    if profile != target {
        Verdict::ProfileElsewhere
    } else if current_home != target {
        Verdict::HomeNotCurrent
    } else {
        Verdict::Active
    }
}

/// 今回の適用が何から作られたか(競合の報告に出す)。
#[derive(Debug, Clone)]
pub struct Source {
    pub apply_ref: String,
    /// ローカル checkout を適用したときの、その絶対パス(worktree)。
    pub worktree: Option<String>,
    pub rev: Option<String>,
}

impl Source {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(w) = &self.worktree {
            parts.push(format!("worktree {w}"));
        }
        parts.push(format!("rev {}", self.rev.as_deref().unwrap_or("不明")));
        format!("{}({})", self.apply_ref, parts.join("、"))
    }
}

fn describe_generation(generation: &str) -> String {
    if generation.is_empty() {
        return "<無し>".to_string();
    }
    match generation_rev_of(generation) {
        Some(rev) => format!("{generation}(rev {rev})"),
        None => generation.to_string(),
    }
}

/// 競合の報告(stderr に出す本文)。
pub fn conflict_report(
    verdict: Verdict,
    source: &Source,
    argv: &[String],
    target: &str,
    profile: &str,
    current_home: &str,
) -> String {
    let cmd = if argv.is_empty() {
        "hms".to_string()
    } else {
        format!("hms {}", argv.join(" "))
    };
    let cause = match verdict {
        Verdict::ProfileElsewhere => {
            "今回の switch の後に、別の適用(別の worktree・セッションの hms、手で実行した\n  \
             home-manager switch、ロックを持たない古い hms)が profile を別の世代へ切り替えました。"
        }
        Verdict::HomeNotCurrent => {
            "profile は今回の世代ですが、current-home が追いついていません。別の適用の\n  \
             activation が進行中か、今回の activation が途中で止まった可能性があります。"
        }
        Verdict::Active => "",
    };
    format!(
        "Error: switch は成功しましたが、今回の世代が有効になっていません(#1025)。\n\
         \x20 適用元: {}\n\
         \x20 今回の目標の世代: {target}\n\
         \x20 実際の世代 profile: {}\n\
         \x20 実際の世代 current-home: {}\n\
         \x20 {cause}\n\
         \x20 いまの環境(Hook の登録を含む)は「実際の世代」のままで、今回の変更は有効ではありません。\n\
         \x20 次の操作:\n\
         \x20   1. 別の適用の変更も残したいときは、その変更と今回の変更を 1 つの checkout に統合する。\n\
         \x20   2. `{cmd}` を、ほかの適用が終わっていることを確かめてから再実行する。\n\
         \x20      再実行は「実際の世代」を今回の世代で置き換える。\n\
         \x20 (この照合は適用直後の 1 回の読み取りです。ここで通っても、その後に別の適用が重なれば\n\
         \x20  また入れ替わります。)\n",
        source.describe(),
        describe_generation(profile),
        describe_generation(current_home),
    )
}

/// switch の前に済ませたこと。
pub struct Prepared {
    pub(crate) target: String,
    /// 待っている間に別の適用が世代を切り替え、今回の switch がそれを置き換えるとき、
    /// 置き換える世代。
    replaces: Option<String>,
    source: Source,
}

/// `prepare` に渡す、今回の適用の入力。
pub struct ApplyInput<'a> {
    pub apply_ref: &'a str,
    pub host: &'a str,
    pub extra_opts: &'a [String],
    /// 適用するローカル checkout の path(リモート ref なら None)。
    pub local_ref: Option<&'a str>,
    /// 適用元の `nix flake metadata --json`(無ければ空)。
    pub meta_json: &'a str,
    /// `run` の開始時点(ロックを待つ前)の profile が指す世代。
    pub generation_at_start: &'a str,
}

/// switch の前: 目標の世代を求め、待っている間に世代が切り替わっていれば知らせる。
/// 目標を求められないとき(適用元が評価できない)は、何も変えないうちにエラーで止める
/// — switch も同じ評価をするので、続けても成功しない。
pub fn prepare(input: &ApplyInput<'_>) -> Result<Prepared, i32> {
    let attr = format!(
        "{}#homeConfigurations.{}.activationPackage.outPath",
        input.apply_ref, input.host
    );
    println!("==> nix eval --raw {attr}");
    let out = Command::new("nix")
        .args(["eval", "--raw"])
        .arg(&attr)
        .args(input.extra_opts)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    let target = match out {
        Ok(o) if o.status.success() => parse_target(&String::from_utf8_lossy(&o.stdout)),
        Ok(o) => {
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            None
        }
        Err(e) => {
            eprintln!("hms: nix: {e}");
            None
        }
    };
    let Some(target) = target else {
        eprintln!("Error: 今回の適用の目標の世代を求められませんでした({attr})。");
        eprintln!("  switch も同じ評価をするので、世代は変えずに止めます。");
        return Err(1);
    };

    let now = current_profile_generation();
    let replaces = (!input.generation_at_start.is_empty()
        && now != input.generation_at_start
        && now != target)
        .then_some(now);
    if let Some(prev) = &replaces {
        eprintln!(
            "==> 注意: 待っている間に別の適用が世代を切り替えました。今回の switch はそれを置き換えます。"
        );
        eprintln!(
            "    開始時の世代: {}",
            describe_generation(input.generation_at_start)
        );
        eprintln!(
            "    いまの世代(置き換えられる): {}",
            describe_generation(prev)
        );
        eprintln!("    今回の目標の世代: {}", describe_generation(&target));
        eprintln!(
            "    置き換えたくなければ Ctrl-C で止めてください(switch はまだ始まっていません)。"
        );
    }

    Ok(Prepared {
        target,
        replaces,
        source: Source {
            apply_ref: input.apply_ref.to_string(),
            worktree: input.local_ref.map(|r| {
                std::fs::canonicalize(r)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| r.to_string())
            }),
            rev: source_revision(input.meta_json),
        },
    })
}

/// switch の成功直後: profile と current-home が目標を指すか照合する。
/// 指していれば 0、指していなければ報告を stderr に出して 1。
pub fn after_switch(prepared: &Prepared, argv: &[String]) -> i32 {
    let profile = current_profile_generation();
    let current_home = readlink_f(&current_home_link());
    match judge(&prepared.target, &profile, &current_home) {
        Verdict::Active => {
            println!(
                "==> 世代を照合しました: profile と current-home が今回の世代を指しています ({})",
                prepared.target
            );
            if let Some(prev) = &prepared.replaces {
                println!(
                    "==> 注意: 待っている間に別の適用が切り替えた世代 {} を、今回の世代で置き換えました。",
                    describe_generation(prev)
                );
            }
            0
        }
        verdict => {
            eprint!(
                "{}",
                conflict_report(
                    verdict,
                    &prepared.source,
                    argv,
                    &prepared.target,
                    &profile,
                    &current_home
                )
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_requires_both_links_to_point_at_the_target() {
        assert_eq!(judge("/g/a", "/g/a", "/g/a"), Verdict::Active);
        assert_eq!(judge("/g/a", "/g/b", "/g/b"), Verdict::ProfileElsewhere);
        assert_eq!(judge("/g/a", "/g/b", "/g/a"), Verdict::ProfileElsewhere);
        assert_eq!(judge("/g/a", "/g/a", "/g/b"), Verdict::HomeNotCurrent);
        // 読めない(空)も一致とは見なさない。
        assert_eq!(judge("/g/a", "", ""), Verdict::ProfileElsewhere);
        assert_eq!(judge("/g/a", "/g/a", ""), Verdict::HomeNotCurrent);
    }

    #[test]
    fn parse_target_accepts_only_an_absolute_path() {
        assert_eq!(
            parse_target("/nix/store/x-gen"),
            Some("/nix/store/x-gen".into())
        );
        assert_eq!(
            parse_target("/nix/store/x-gen\n"),
            Some("/nix/store/x-gen".into())
        );
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("error: attribute missing"), None);
    }

    #[test]
    fn source_revision_prefers_the_clean_rev_then_the_dirty_one() {
        assert_eq!(
            source_revision(r#"{"revision":"abc","dirtyRevision":"abc-dirty"}"#),
            Some("abc".into())
        );
        assert_eq!(
            source_revision(r#"{"dirtyRevision":"abc-dirty"}"#),
            Some("abc-dirty".into())
        );
        assert_eq!(source_revision(r#"{}"#), None);
        assert_eq!(source_revision(""), None);
    }

    #[test]
    fn the_report_names_the_source_the_actual_generation_and_the_next_step() {
        let source = Source {
            apply_ref: "github:example/dotfiles".into(),
            worktree: Some("/w/feature".into()),
            rev: Some("abc123".into()),
        };
        let argv = vec![".".to_string(), "--public-only".to_string()];
        let r = conflict_report(
            Verdict::ProfileElsewhere,
            &source,
            &argv,
            "/nix/store/target",
            "/nix/store/other",
            "/nix/store/other",
        );
        assert!(r.contains("worktree /w/feature"));
        assert!(r.contains("rev abc123"));
        assert!(r.contains("今回の目標の世代: /nix/store/target"));
        assert!(r.contains("実際の世代 profile: /nix/store/other"));
        assert!(r.contains("`hms . --public-only` を"));
    }

    #[test]
    fn the_report_explains_the_two_verdicts_differently() {
        let source = Source {
            apply_ref: "r".into(),
            worktree: None,
            rev: None,
        };
        let moved = conflict_report(Verdict::ProfileElsewhere, &source, &[], "/t", "/o", "/o");
        let home = conflict_report(Verdict::HomeNotCurrent, &source, &[], "/t", "/t", "/o");
        assert!(moved.contains("profile を別の世代へ切り替えました"));
        assert!(home.contains("current-home が追いついていません"));
        assert!(moved.contains("`hms` を"));
    }
}
