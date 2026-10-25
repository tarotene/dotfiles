//! dotfiles-identity — この機体の identity と、ログイン中の Claude アカウントの一致を確かめる。
//!
//! 共用の外部アカウント(1 つの GitHub アカウントなど)を、会社と個人のどちらの機体で
//! 操作してよいかを分けるとき、機体の宣言だけに頼ると、宣言と実際のログインが食い違った
//! ときに気づけない。そこで 2 つの独立した事実の一致を要件にする:
//!
//! 1. 機体の宣言: `DOTFILES_IDENTITY`(`personal` / `company`)。
//! 2. ログイン中の Claude アカウントのメール: `~/.claude.json` の
//!    `oauthAccount.emailAddress`。期待する形は `DOTFILES_CLAUDE_ACCOUNT`
//!    (`exact:<メール>` か `domain:<ドメイン>`)で、identity ごとに nix の
//!    `dotfiles.identity` が宣言する(home/modules/identity.nix。正本はそこ 1 か所)。
//!
//! どれかが欠ける・読めない・食い違うときは、identity を返さずエラーにする(閉じる側)。
//! 純関数だけをここに置き、環境変数とファイルの読み出しは main.rs が行う。

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// 認める identity の閉じた集合。
pub const IDENTITIES: [&str; 2] = ["personal", "company"];

/// `DOTFILES_CLAUDE_ACCOUNT` が表す、期待する Claude アカウントの形。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// メールアドレスが完全一致(大文字小文字は区別しない)。
    Exact(String),
    /// メールアドレスのドメイン部が完全一致(サブドメインは含めない)。
    Domain(String),
}

/// 一致を確かめられなかった理由。メールアドレス自体は含めない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// `DOTFILES_IDENTITY` が無い。
    IdentityUnset,
    /// `DOTFILES_IDENTITY` が `personal` / `company` のどちらでもない。
    IdentityUnknown(String),
    /// `DOTFILES_CLAUDE_ACCOUNT` が無い。
    ExpectationUnset,
    /// `DOTFILES_CLAUDE_ACCOUNT` が `exact:<値>` / `domain:<値>` の形でない。
    ExpectationMalformed,
    /// `~/.claude.json` が読めない、または JSON として読めない。
    ClaudeJsonUnreadable,
    /// `oauthAccount.emailAddress` が無い(ログインしていない、または書式が変わった)。
    EmailMissing,
    /// ログイン中のメールが、identity の期待に合わない。
    Mismatch,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityUnset => write!(f, "DOTFILES_IDENTITY is not set"),
            Self::IdentityUnknown(v) => write!(
                f,
                "DOTFILES_IDENTITY={v:?} is not one of {}",
                IDENTITIES.join(" / ")
            ),
            Self::ExpectationUnset => write!(f, "DOTFILES_CLAUDE_ACCOUNT is not set"),
            Self::ExpectationMalformed => write!(
                f,
                "DOTFILES_CLAUDE_ACCOUNT must look like exact:<email> or domain:<domain>"
            ),
            Self::ClaudeJsonUnreadable => write!(f, "the Claude config file could not be read"),
            Self::EmailMissing => write!(
                f,
                "the Claude config file has no oauthAccount.emailAddress (not logged in?)"
            ),
            Self::Mismatch => write!(
                f,
                "the logged-in Claude account does not match this machine's identity"
            ),
        }
    }
}

impl std::error::Error for VerifyError {}

/// `exact:<メール>` / `domain:<ドメイン>` を読む。
pub fn parse_expect(text: &str) -> Result<Expect, VerifyError> {
    let (kind, value) = text
        .split_once(':')
        .ok_or(VerifyError::ExpectationMalformed)?;
    let value = value.trim();
    if value.is_empty() {
        return Err(VerifyError::ExpectationMalformed);
    }
    match kind {
        "exact" if value.contains('@') => Ok(Expect::Exact(value.to_ascii_lowercase())),
        "domain" if !value.contains('@') => Ok(Expect::Domain(value.to_ascii_lowercase())),
        _ => Err(VerifyError::ExpectationMalformed),
    }
}

/// メールアドレスが期待に合うか。ドメインは `@` より後ろ全体の完全一致で、
/// `evil-example.com` や `sub.example.com` は `example.com` に合わない。
pub fn email_matches(expect: &Expect, email: &str) -> bool {
    let email = email.trim().to_ascii_lowercase();
    match expect {
        Expect::Exact(want) => email == *want,
        Expect::Domain(want) => email
            .rsplit_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain == want),
    }
}

/// Claude Code の設定ファイルの場所。`CLAUDE_CONFIG_DIR` があればその直下の `.claude.json`、
/// 無ければ `$HOME/.claude.json`。どちらも決まらなければ `None`。
pub fn claude_json_path(config_dir: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let non_empty = |v: Option<OsString>| v.filter(|v| !v.is_empty());
    match non_empty(config_dir) {
        Some(dir) => Some(PathBuf::from(dir).join(".claude.json")),
        None => non_empty(home).map(|h| PathBuf::from(h).join(".claude.json")),
    }
}

/// `~/.claude.json` の本文から `oauthAccount.emailAddress` を取り出す。
pub fn claude_email(claude_json: &str) -> Result<String, VerifyError> {
    let value: serde_json::Value =
        serde_json::from_str(claude_json).map_err(|_| VerifyError::ClaudeJsonUnreadable)?;
    value
        .pointer("/oauthAccount/emailAddress")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .filter(|e| !e.trim().is_empty())
        .ok_or(VerifyError::EmailMissing)
}

/// 3 つの入力が一致したときだけ identity を返す。入力は欠けていてよく(`None`)、
/// そのときは理由つきで失敗する。
pub fn verify(
    identity: Option<&str>,
    expectation: Option<&str>,
    claude_json: Option<&str>,
) -> Result<&'static str, VerifyError> {
    let identity = identity.ok_or(VerifyError::IdentityUnset)?;
    let known = IDENTITIES
        .iter()
        .copied()
        .find(|k| *k == identity)
        .ok_or_else(|| VerifyError::IdentityUnknown(identity.to_owned()))?;
    let expect = parse_expect(expectation.ok_or(VerifyError::ExpectationUnset)?)?;
    let email = claude_email(claude_json.ok_or(VerifyError::ClaudeJsonUnreadable)?)?;
    if email_matches(&expect, &email) {
        Ok(known)
    } else {
        Err(VerifyError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERSONAL: &str = "exact:me@example.org";
    const COMPANY: &str = "domain:corp.example";

    fn json(email: &str) -> String {
        format!(r#"{{"oauthAccount":{{"emailAddress":"{email}"}}}}"#)
    }

    #[test]
    fn personal_matches_the_exact_address_only() {
        let e = parse_expect(PERSONAL).unwrap();
        assert!(email_matches(&e, "me@example.org"));
        assert!(email_matches(&e, " ME@Example.org "), "case and padding");
        assert!(!email_matches(&e, "other@example.org"));
        assert!(!email_matches(&e, "me@example.org.evil.test"));
    }

    #[test]
    fn company_matches_the_whole_domain_not_a_suffix_or_a_subdomain() {
        let e = parse_expect(COMPANY).unwrap();
        assert!(email_matches(&e, "anyone@corp.example"));
        assert!(!email_matches(&e, "anyone@evil-corp.example"));
        assert!(!email_matches(&e, "anyone@sub.corp.example"));
        assert!(!email_matches(&e, "@corp.example"), "empty local part");
        assert!(!email_matches(&e, "corp.example"), "no @ at all");
    }

    #[test]
    fn expectation_must_be_well_formed() {
        for bad in [
            "",
            "exact",
            "exact:",
            "exact:not-an-email",
            "domain:me@example.org",
            "domain:",
            "prefix:corp.example",
            "corp.example",
        ] {
            assert_eq!(
                parse_expect(bad),
                Err(VerifyError::ExpectationMalformed),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn verify_returns_the_identity_when_declaration_and_login_agree() {
        assert_eq!(
            verify(
                Some("personal"),
                Some(PERSONAL),
                Some(&json("me@example.org"))
            ),
            Ok("personal")
        );
        assert_eq!(
            verify(
                Some("company"),
                Some(COMPANY),
                Some(&json("a@corp.example"))
            ),
            Ok("company")
        );
    }

    #[test]
    fn verify_refuses_when_the_login_belongs_to_the_other_side() {
        // 個人の機体の宣言なのに会社のアカウントでログインしている、およびその逆。
        assert_eq!(
            verify(
                Some("personal"),
                Some(PERSONAL),
                Some(&json("a@corp.example"))
            ),
            Err(VerifyError::Mismatch)
        );
        assert_eq!(
            verify(
                Some("company"),
                Some(COMPANY),
                Some(&json("me@example.org"))
            ),
            Err(VerifyError::Mismatch)
        );
    }

    #[test]
    fn verify_refuses_every_missing_or_unreadable_input() {
        let ok = json("me@example.org");
        assert_eq!(
            verify(None, Some(PERSONAL), Some(&ok)),
            Err(VerifyError::IdentityUnset)
        );
        assert_eq!(
            verify(Some("work"), Some(PERSONAL), Some(&ok)),
            Err(VerifyError::IdentityUnknown("work".into()))
        );
        assert_eq!(
            verify(Some("personal"), None, Some(&ok)),
            Err(VerifyError::ExpectationUnset)
        );
        assert_eq!(
            verify(Some("personal"), Some(PERSONAL), None),
            Err(VerifyError::ClaudeJsonUnreadable)
        );
        assert_eq!(
            verify(Some("personal"), Some(PERSONAL), Some("not json")),
            Err(VerifyError::ClaudeJsonUnreadable)
        );
        assert_eq!(
            verify(Some("personal"), Some(PERSONAL), Some("{}")),
            Err(VerifyError::EmailMissing)
        );
        assert_eq!(
            verify(
                Some("personal"),
                Some(PERSONAL),
                Some(r#"{"oauthAccount":{"emailAddress":"  "}}"#)
            ),
            Err(VerifyError::EmailMissing)
        );
    }

    #[test]
    fn claude_json_path_prefers_the_config_dir_over_home() {
        let os = |s: &str| Some(OsString::from(s));
        assert_eq!(
            claude_json_path(os("/cfg"), os("/home/x")),
            Some(PathBuf::from("/cfg/.claude.json"))
        );
        assert_eq!(
            claude_json_path(None, os("/home/x")),
            Some(PathBuf::from("/home/x/.claude.json"))
        );
        assert_eq!(
            claude_json_path(os(""), os("/home/x")),
            Some(PathBuf::from("/home/x/.claude.json")),
            "an empty CLAUDE_CONFIG_DIR is unset"
        );
        assert_eq!(claude_json_path(None, None), None);
    }

    #[test]
    fn error_text_never_contains_the_email() {
        let err = verify(
            Some("personal"),
            Some(PERSONAL),
            Some(&json("secret@corp.example")),
        )
        .unwrap_err();
        assert!(!err.to_string().contains("secret"));
    }
}
