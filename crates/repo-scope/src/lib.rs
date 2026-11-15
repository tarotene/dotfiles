//! repo-scope — 機体の Claude の契約の側(個人 / 会社)と、GitHub の持ち主の側の表から、
//! 読んでよいリポジトリかを決める共通の判定(設計と根拠は docs/agents/repo-scope-guard.md)。
//!
//! 理由は、契約の非対称である。個人の契約は、入力を学習に使わないことを厳密には保証しない。
//! 会社の契約は保証する。そこで、個人の契約の機体(personal)は、**別の側の持ち主・表に無い持ち主の
//! 非公開リポジトリ**を読まない。公開リポジトリは持ち主を問わず読んでよく、会社の契約の機体
//! (company)は、個人側の非公開も読んでよい。
//!
//! | 機体の契約 | 同じ側の持ち主 | 別の側・表に無い持ち主 |
//! |---|---|---|
//! | personal | 公開・非公開とも可 | 公開だけ可(非公開、可視性を確かめられないものは不可) |
//! | company | 可 | 可 |
//!
//! 側は、持ち主の表(`~/.config/dotfiles/github-owners.toml`。公開側が宣言し、値は非公開の設定
//! リポジトリが入れる。ADR-0034)で引く。表のファイルが無い機体は、この規則を配っていないので、
//! 何も制限しない([`load_policy`] が `None`)。
//!
//! 純関数(表の解析・判定・URL の解析)と、環境に触れる部分(表のファイル・可視性の照会と
//! その 1 日の写し・`git remote`)を分けてある。環境に触れる部分は、注入できる形にして試験する。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// GitHub の持ち主の側。閉じた 2 語。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Personal,
    Company,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "personal" => Some(Side::Personal),
            "company" => Some(Side::Company),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Side::Personal => "personal",
            Side::Company => "company",
        }
    }
}

/// リポジトリの公開・非公開。`internal` は非公開に数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    NonPublic,
}

/// `gh api repos/<o>/<r> --jq .visibility` の出力(`public` / `private` / `internal`)を読む。
/// 読めなければ `None`(呼び出し側は、公開と確かめられなかったものとして扱う)。
pub fn parse_visibility(s: &str) -> Option<Visibility> {
    match s.trim() {
        "public" => Some(Visibility::Public),
        "private" | "internal" => Some(Visibility::NonPublic),
        _ => None,
    }
}

/// 持ち主(GitHub のログイン名。大文字小文字を区別しない)から側への表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owners(BTreeMap<String, Side>);

impl Owners {
    /// `[owners]` の表を読む。値は `personal` / `company` だけで、同じ持ち主を(大文字小文字を
    /// 無視して)2 回書くとエラー。`[owners]` が無ければ空の表。
    pub fn parse(text: &str) -> Result<Owners, String> {
        let root: toml::Table = text
            .parse()
            .map_err(|e| format!("TOML として読めない: {e}"))?;
        let mut map = BTreeMap::new();
        let Some(owners) = root.get("owners") else {
            return Ok(Owners(map));
        };
        let owners = owners
            .as_table()
            .ok_or_else(|| "`owners` が表(table)でない".to_string())?;
        for (name, value) in owners {
            let side = value
                .as_str()
                .and_then(Side::parse)
                .ok_or_else(|| format!("`owners.{name}` が personal / company のどちらでもない"))?;
            if map.insert(name.to_ascii_lowercase(), side).is_some() {
                return Err(format!("持ち主 `{name}` が重複している"));
            }
        }
        Ok(Owners(map))
    }

    pub fn side_of(&self, owner: &str) -> Option<Side> {
        self.0.get(&owner.to_ascii_lowercase()).copied()
    }

    /// 側ごとの持ち主(小文字、昇順)。
    pub fn of_side(&self, side: Side) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(_, s)| **s == side)
            .map(|(o, _)| o.as_str())
            .collect()
    }
}

/// 判定が不可になった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    /// 別の側・表に無い持ち主の、非公開リポジトリ(`owner/repo`)。
    NonPublic(String),
    /// 別の側・表に無い持ち主のリポジトリ(`owner/repo`)で、公開か確かめられなかった。
    VisibilityUnknown(String),
    /// 持ち主の単位(`gh repo list <owner>`・組織の API など)で、別の側・表に無い持ち主。
    /// リポジトリを指さないので、公開だけに絞れない。
    OtherSideOwner(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny(DenyReason),
}

/// この機体で、どの持ち主のリポジトリを読んでよいかの方針。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub side: Side,
    pub owners: Owners,
}

impl Policy {
    /// 持ち主の単位の判定。会社の契約の機体は何でも可。個人の契約の機体は、同じ側の持ち主だけ可。
    pub fn check_owner(&self, owner: &str) -> Verdict {
        if self.side == Side::Company || self.owners.side_of(owner) == Some(self.side) {
            Verdict::Allow
        } else {
            Verdict::Deny(DenyReason::OtherSideOwner(owner.to_ascii_lowercase()))
        }
    }

    /// リポジトリ単位の判定。同じ側の持ち主、または会社の契約の機体は、照会せずに可。
    /// そうでなければ `visibility` で公開かを引き、公開だけ可(引けなければ不可)。
    pub fn check_repo(
        &self,
        owner: &str,
        repo: &str,
        visibility: &mut dyn FnMut(&str) -> Option<Visibility>,
    ) -> Verdict {
        if self.check_owner(owner) == Verdict::Allow {
            return Verdict::Allow;
        }
        let nwo = format!(
            "{}/{}",
            owner.to_ascii_lowercase(),
            repo.to_ascii_lowercase()
        );
        match visibility(&nwo) {
            Some(Visibility::Public) => Verdict::Allow,
            Some(Visibility::NonPublic) => Verdict::Deny(DenyReason::NonPublic(nwo)),
            None => Verdict::Deny(DenyReason::VisibilityUnknown(nwo)),
        }
    }

    /// この機体の側と同じ側の持ち主(横断検索の `--owner` に渡す)。
    pub fn same_side_owners(&self) -> Vec<&str> {
        self.owners.of_side(self.side)
    }
}

/// 機体の契約の側。`dotfiles-identity verify` が通って `company` のときだけ会社。通らない
/// (宣言が無い・ログインと食い違う)ときは、個人として扱う(制限する側に倒す)。
pub fn machine_side() -> Side {
    match dotfiles_identity::verify_from_env() {
        Ok("company") => Side::Company,
        _ => Side::Personal,
    }
}

/// 持ち主の表のファイル: `<XDG の設定の基準>/dotfiles/github-owners.toml`。
pub fn owners_path() -> Option<PathBuf> {
    process_env::Xdg::Config
        .resolve()
        .map(|d| d.join("dotfiles").join("github-owners.toml"))
}

/// 表のファイルを読んで、方針を作る。ファイルが無い(この機体にこの規則を配っていない)なら
/// `Ok(None)`。あるのに読めない・解析できないなら `Err`(呼び出し側は閉じる側に倒す)。
pub fn load_policy(side: Side) -> Result<Option<Policy>, String> {
    let Some(path) = owners_path() else {
        return Ok(None);
    };
    load_policy_from(&path, side)
}

/// [`load_policy`] の、ファイルの場所を注入できる版。
pub fn load_policy_from(path: &Path, side: Side) -> Result<Option<Policy>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{} が読めない: {e}", path.display())),
    };
    let owners = Owners::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(Policy { side, owners }))
}

// ---- `owner/repo` と URL の解析 ----

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// `owner/repo`、または `github.com/owner/repo` を (owner, repo) に。ほかの形は `None`。
/// 末尾の `.git` は落とす。
pub fn split_nwo(s: &str) -> Option<(String, String)> {
    let s = s.strip_prefix("github.com/").unwrap_or(s);
    let (owner, repo) = s.split_once('/')?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let valid = |p: &str| !p.is_empty() && p.chars().all(is_name_char) && !p.starts_with('.');
    (valid(owner) && valid(repo)).then(|| (owner.to_string(), repo.to_string()))
}

/// `https://github.com/<o>/<r>[/…]` を (o, r) に。
pub fn repo_from_url(u: &str) -> Option<(String, String)> {
    let rest = u
        .strip_prefix("https://github.com/")
        .or_else(|| u.strip_prefix("http://github.com/"))?;
    let rest = rest.split(['#', '?']).next()?;
    let mut parts = rest.split('/');
    let (o, r) = (parts.next()?, parts.next()?);
    split_nwo(&format!("{o}/{r}"))
}

/// `git remote get-url` の出力から (持ち主, リポジトリ) を取る(`https://github.com/o/r(.git)`、
/// `git@github.com:o/r.git`、`ssh://git@github.com/o/r.git`)。GitHub 以外は `None`。
pub fn repo_of_remote_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let path = if let Some(p) = url.strip_prefix("git@github.com:") {
        p
    } else if let Some(p) = url.strip_prefix("ssh://git@github.com/") {
        p
    } else {
        return repo_from_url(url);
    };
    split_nwo(path)
}

/// `dir` の `origin` の (持ち主, リポジトリ)。git の外・origin が無い・GitHub でないときは `None`。
pub fn origin_repo(dir: &Path) -> Option<(String, String)> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["remote", "get-url", "origin"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| repo_of_remote_url(&String::from_utf8_lossy(&out.stdout)))
        .flatten()
}

// ---- 可視性の照会と、1 日の写し ----

/// 可視性の写しの寿命(秒)。公開 → 非公開の変更は稀で、1 日の遅れを許す。
pub const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// 写しのファイル: `<XDG の状態の基準>/claude/repo-scope/visibility.json`。
pub fn default_cache_path() -> Option<PathBuf> {
    process_env::Xdg::State.resolve().map(|base| {
        base.join("claude")
            .join("repo-scope")
            .join("visibility.json")
    })
}

fn read_cache(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| match v {
            serde_json::Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// `nwo`(小文字の `owner/repo`)の可視性。`now`(epoch 秒)から [`CACHE_TTL_SECS`] 以内の写しが
/// あればそれを返し、無ければ `fetch` で引いて、引けたものだけを写しに書く(書けなくても無視)。
pub fn cached_visibility(
    cache: &Path,
    nwo: &str,
    now: u64,
    fetch: impl FnOnce(&str) -> Option<Visibility>,
) -> Option<Visibility> {
    let mut map = read_cache(cache);
    if let Some(hit) = map.get(nwo) {
        let at = hit.get("at").and_then(serde_json::Value::as_u64);
        let vis = match hit.get("v").and_then(serde_json::Value::as_str) {
            Some("public") => Some(Visibility::Public),
            Some("nonpublic") => Some(Visibility::NonPublic),
            _ => None,
        };
        if let (Some(at), Some(vis)) = (at, vis) {
            if now.saturating_sub(at) < CACHE_TTL_SECS {
                return Some(vis);
            }
        }
    }
    let vis = fetch(nwo)?;
    let v = match vis {
        Visibility::Public => "public",
        Visibility::NonPublic => "nonpublic",
    };
    // 期限切れの行は、書き直すついでに落とす。
    map.retain(|_, e| {
        e.get("at")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|at| now.saturating_sub(at) < CACHE_TTL_SECS)
    });
    map.insert(nwo.to_string(), serde_json::json!({ "v": v, "at": now }));
    write_cache(cache, &map);
    Some(vis)
}

fn write_cache(path: &Path, map: &serde_json::Map<String, serde_json::Value>) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // 同時に走る hook が互いの書きかけを読まないよう、別名に書いてから置き換える。
    let tmp = dir.join(format!("visibility.json.{}.tmp", std::process::id()));
    let body = serde_json::Value::Object(map.clone()).to_string();
    if std::fs::write(&tmp, body).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// `gh api repos/<nwo> --jq .visibility` で引く。引けなければ `None`(404・権限・ネットワークを区別しない)。
pub fn gh_visibility(nwo: &str) -> Option<Visibility> {
    let out =
        github_command::exec::gh_ok(&["api", &format!("repos/{nwo}"), "--jq", ".visibility"])?;
    parse_visibility(&out)
}

/// 写しつきで、実際に `gh` で引く。
pub fn visibility_of(nwo: &str) -> Option<Visibility> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match default_cache_path() {
        Some(cache) => cached_visibility(&cache, nwo, now, gh_visibility),
        None => gh_visibility(nwo),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // 試験に使う名前は、すべて架空(実在の持ち主の名前をここに書かない。ADR-0034)。
    const TABLE: &str = r#"
[owners]
me = "personal"
Friends-Org = "personal"
acme-corp = "company"
"#;

    fn policy(side: Side) -> Policy {
        Policy {
            side,
            owners: Owners::parse(TABLE).unwrap(),
        }
    }

    #[test]
    fn owners_table_is_closed_case_insensitive_and_rejects_duplicates() {
        let o = Owners::parse(TABLE).unwrap();
        assert_eq!(o.side_of("ME"), Some(Side::Personal));
        assert_eq!(o.side_of("friends-org"), Some(Side::Personal));
        assert_eq!(o.side_of("ACME-CORP"), Some(Side::Company));
        assert_eq!(o.side_of("stranger"), None);
        assert_eq!(o.of_side(Side::Personal), vec!["friends-org", "me"]);
        assert_eq!(o.of_side(Side::Company), vec!["acme-corp"]);

        assert!(Owners::parse("[owners]\nx = \"other\"\n").is_err());
        assert!(Owners::parse("[owners]\nx = 1\n").is_err());
        assert!(Owners::parse("[owners]\nAbc = \"personal\"\nabc = \"company\"\n").is_err());
        assert!(Owners::parse("owners = \"x\"\n").is_err());
        assert!(Owners::parse("[owners\n").is_err());
        assert_eq!(Owners::parse("").unwrap(), Owners::default());
    }

    #[test]
    fn personal_machine_reads_its_own_side_without_a_lookup() {
        let p = policy(Side::Personal);
        let mut never = |_: &str| -> Option<Visibility> { panic!("同じ側は照会しない") };
        assert_eq!(p.check_repo("me", "dotfiles", &mut never), Verdict::Allow);
        assert_eq!(p.check_repo("ME", "x", &mut never), Verdict::Allow);
        assert_eq!(p.check_owner("friends-org"), Verdict::Allow);
    }

    #[test]
    fn personal_machine_reads_only_public_repos_of_the_other_side_and_unknown_owners() {
        let p = policy(Side::Personal);
        for owner in ["acme-corp", "Acme-Corp", "stranger"] {
            let mut public = |_: &str| Some(Visibility::Public);
            assert_eq!(
                p.check_repo(owner, "r", &mut public),
                Verdict::Allow,
                "{owner}"
            );
            let mut private = |_: &str| Some(Visibility::NonPublic);
            assert_eq!(
                p.check_repo(owner, "R", &mut private),
                Verdict::Deny(DenyReason::NonPublic(format!(
                    "{}/r",
                    owner.to_ascii_lowercase()
                ))),
                "{owner}"
            );
            // 引けない(404・ネットワーク)ときは、公開と確かめられないので閉じる
            let mut unknown = |_: &str| None;
            assert!(
                matches!(
                    p.check_repo(owner, "r", &mut unknown),
                    Verdict::Deny(DenyReason::VisibilityUnknown(_))
                ),
                "{owner}"
            );
        }
    }

    #[test]
    fn owner_level_queries_have_no_visibility_to_narrow_with() {
        let p = policy(Side::Personal);
        assert_eq!(p.check_owner("me"), Verdict::Allow);
        assert_eq!(
            p.check_owner("acme-corp"),
            Verdict::Deny(DenyReason::OtherSideOwner("acme-corp".into()))
        );
        assert!(matches!(p.check_owner("stranger"), Verdict::Deny(_)));
    }

    #[test]
    fn company_machine_reads_everything_without_a_lookup() {
        let p = policy(Side::Company);
        let mut never =
            |_: &str| -> Option<Visibility> { panic!("会社の契約は照会しない") };
        for owner in ["acme-corp", "me", "stranger"] {
            assert_eq!(p.check_repo(owner, "r", &mut never), Verdict::Allow);
            assert_eq!(p.check_owner(owner), Verdict::Allow);
        }
    }

    #[test]
    fn same_side_owners_follow_the_machine_side() {
        assert_eq!(
            policy(Side::Personal).same_side_owners(),
            vec!["friends-org", "me"]
        );
        assert_eq!(policy(Side::Company).same_side_owners(), vec!["acme-corp"]);
    }

    #[test]
    fn visibility_output_is_parsed_closed() {
        assert_eq!(parse_visibility("public\n"), Some(Visibility::Public));
        assert_eq!(parse_visibility("private"), Some(Visibility::NonPublic));
        assert_eq!(parse_visibility("internal"), Some(Visibility::NonPublic));
        assert_eq!(parse_visibility(""), None);
        assert_eq!(parse_visibility("PUBLIC"), None);
        assert_eq!(parse_visibility("null"), None);
    }

    #[test]
    fn nwo_and_url_forms() {
        assert_eq!(split_nwo("o/r"), Some(("o".into(), "r".into())));
        assert_eq!(split_nwo("github.com/o/r"), Some(("o".into(), "r".into())));
        assert_eq!(split_nwo("o/r.git"), Some(("o".into(), "r".into())));
        for bad in ["", "o", "o/", "/r", "o/r/x", "-R", "o/ r", "../x", "a b/c"] {
            assert_eq!(split_nwo(bad), None, "{bad}");
        }
        assert_eq!(
            repo_from_url("https://github.com/o/r/pull/12#x"),
            Some(("o".into(), "r".into()))
        );
        assert_eq!(repo_from_url("https://example.com/o/r"), None);
        assert_eq!(repo_from_url("https://github.com/o"), None);

        for url in [
            "https://github.com/o/r.git\n",
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r",
            "https://github.com/o/r",
        ] {
            assert_eq!(
                repo_of_remote_url(url),
                Some(("o".into(), "r".into())),
                "{url}"
            );
        }
        assert_eq!(repo_of_remote_url("git@gitlab.com:o/r.git"), None);
        assert_eq!(repo_of_remote_url("/srv/git/r.git"), None);
    }

    #[test]
    fn a_missing_table_file_means_the_rule_is_not_deployed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("github-owners.toml");
        assert_eq!(load_policy_from(&path, Side::Personal), Ok(None));

        std::fs::write(&path, TABLE).unwrap();
        let p = load_policy_from(&path, Side::Personal).unwrap().unwrap();
        assert_eq!(p.side, Side::Personal);
        assert_eq!(p.owners.side_of("me"), Some(Side::Personal));

        // あるのに解析できないものは、配っていないことにせず、エラーにする
        std::fs::write(&path, "[owners\n").unwrap();
        assert!(load_policy_from(&path, Side::Personal).is_err());
    }

    #[test]
    fn visibility_cache_serves_within_the_ttl_and_refetches_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sub").join("visibility.json");
        let calls = Cell::new(0);
        let fetch = |v: Option<Visibility>| {
            let calls = &calls;
            move |_: &str| {
                calls.set(calls.get() + 1);
                v
            }
        };

        let t0 = 1_000_000;
        assert_eq!(
            cached_visibility(&cache, "o/r", t0, fetch(Some(Visibility::NonPublic))),
            Some(Visibility::NonPublic)
        );
        assert_eq!(calls.get(), 1);
        // 寿命の内は、引き直さない(別の値を返す fetch を渡しても、写しが返る)
        assert_eq!(
            cached_visibility(
                &cache,
                "o/r",
                t0 + CACHE_TTL_SECS - 1,
                fetch(Some(Visibility::Public))
            ),
            Some(Visibility::NonPublic)
        );
        assert_eq!(calls.get(), 1);
        // 寿命を過ぎたら引き直す
        assert_eq!(
            cached_visibility(
                &cache,
                "o/r",
                t0 + CACHE_TTL_SECS,
                fetch(Some(Visibility::Public))
            ),
            Some(Visibility::Public)
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn failed_lookups_are_not_cached_and_a_corrupt_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("visibility.json");
        assert_eq!(cached_visibility(&cache, "o/r", 5, |_| None), None);
        assert!(!cache.exists(), "引けなかったものは書かない");

        std::fs::write(&cache, "not json").unwrap();
        assert_eq!(
            cached_visibility(&cache, "o/r", 5, |_| Some(Visibility::Public)),
            Some(Visibility::Public)
        );
        let again = cached_visibility(&cache, "o/r", 6, |_| panic!("写しがあるはず"));
        assert_eq!(again, Some(Visibility::Public));
    }
}
