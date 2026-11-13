//! 個人と会社の対称性の不変条件: 側(identity)ごとの違いが入る場所を、identity のファイル・host の
//! ファイル・private の module の並びだけに限る(`docs/adr/` の「個人と会社は対称な対」)。
//!
//! 公開の module(`home/modules/**`)は、機体が個人か会社かで分岐しない。module は、側ごとに違う値を
//! typed な option(例: `dotfiles.tailscale.*`・`dotfiles.claudePermissions.*`)で受け取り、identity の
//! ファイル(`home/identities/**`)がその option に値を与える。公開の module が `dotfiles.identity` を
//! 直接読むと、側ごとの振る舞いの差が、option の宣言(読み手に見える差)の外に入り、「違いは Slack の有無と
//! GitHub アカウントの共用だけ」という対称な対の前提が、コードの中で静かに崩れる。
//!
//! 検査は Nix の字句解析(`host_names::lex`)を通して行う。コメントは読まない(説明のコメントに書くのは
//! 構わない)。例外は、`dotfiles.identity` を宣言し、環境変数へ変換する唯一の module
//! `home/modules/identity.nix`。ホスト名の不変条件は `host_names` が持つ。

use crate::host_names::{find_word, lex, NixFile};
use crate::manifest::Violation;

/// 側(identity)を表す option。
pub const IDENTITY_OPTION: &str = "dotfiles.identity";

/// `dotfiles.identity` を宣言し、環境変数へ変換する唯一の公開 module。
pub const IDENTITY_MODULE: &str = "home/modules/identity.nix";

/// 公開の module の置き場。
const PUBLIC_MODULES: &str = "home/modules/";

fn violation(subject: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: subject.to_string(),
        detail: detail.into(),
    }
}

/// 公開の module が、側(identity)で分岐していないかを検査する。
pub fn check_identity_branches(files: &[NixFile]) -> Vec<Violation> {
    let mut out = Vec::new();

    for f in files {
        if !f.path.starts_with(PUBLIC_MODULES) || f.path == IDENTITY_MODULE {
            continue;
        }
        if find_word(&lex(&f.text).code, IDENTITY_OPTION).is_some() {
            out.push(violation(
                &f.path,
                format!(
                    "`{IDENTITY_OPTION}` を読んでいる。公開の module は側(個人・会社)で分岐しない。側ごとに違う値は typed な option にして、`home/identities/` のファイルが値を与える(説明は `<identity>` のように書く)"
                ),
            ));
        }
    }

    // 番兵: 例外の module が無い、または宣言を持たないと、検査が空振りしても通ってしまう。
    match files.iter().find(|f| f.path == IDENTITY_MODULE) {
        Some(f) if find_word(&lex(&f.text).code, IDENTITY_OPTION).is_some() => {}
        Some(f) => out.push(violation(
            &f.path,
            format!("`{IDENTITY_OPTION}` を宣言していない。側を表す option を宣言するのは、この module の仕事"),
        )),
        None => out.push(violation(
            IDENTITY_MODULE,
            "ファイルが無い。側を表す option を宣言する module が要る",
        )),
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nix(path: &str, text: &str) -> NixFile {
        NixFile {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    fn ok_tree() -> Vec<NixFile> {
        vec![
            nix(
                IDENTITY_MODULE,
                "{ config, ... }: { options.dotfiles.identity.name = 1; config.x = config.dotfiles.identity.name; }\n",
            ),
            nix("home/modules/a.nix", "{ x = \"hello\"; }\n"),
            nix(
                "home/identities/company.nix",
                "{ dotfiles.identity.name = \"company\"; }\n",
            ),
        ]
    }

    fn details(v: &[Violation]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_clean_tree_has_no_violations() {
        assert_eq!(
            details(&check_identity_branches(&ok_tree())),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_public_module_that_reads_the_identity_fails() {
        let mut files = ok_tree();
        files.push(nix(
            "home/modules/b.nix",
            "{ config, lib, ... }: { x = lib.mkIf (config.dotfiles.identity.name == \"company\") 1; }\n",
        ));
        let v = details(&check_identity_branches(&files));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].contains("home/modules/b.nix"), "{v:?}");
        assert!(v[0].contains("dotfiles.identity"), "{v:?}");
    }

    #[test]
    fn a_mention_in_a_comment_is_not_a_read() {
        let mut files = ok_tree();
        files.push(nix(
            "home/modules/b.nix",
            "# dotfiles.identity で分岐しない\n{ x = 1; } /* dotfiles.identity */\n",
        ));
        assert_eq!(
            details(&check_identity_branches(&files)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_identity_layer_may_set_the_option() {
        // `home/identities/**` と host のファイルは、側ごとの値を与える場所。
        let mut files = ok_tree();
        files.push(nix(
            "home/hosts/vega.nix",
            "{ dotfiles.identity.name = \"personal\"; }\n",
        ));
        assert_eq!(
            details(&check_identity_branches(&files)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_missing_or_empty_identity_module_fails() {
        let no_module = vec![nix("home/modules/a.nix", "{ x = 1; }\n")];
        let v = details(&check_identity_branches(&no_module));
        assert!(v.iter().any(|m| m.contains("ファイルが無い")), "{v:?}");

        let no_declaration = vec![nix(IDENTITY_MODULE, "{ x = 1; }\n")];
        let v = details(&check_identity_branches(&no_declaration));
        assert!(v.iter().any(|m| m.contains("宣言していない")), "{v:?}");
    }
}
