//! `gpg --with-colons` の出力を読む(旧 bash 版の awk 部分)。
//!
//! 欄番号は GnuPG の doc/DETAILS に従う(1 始まり)。awk の `$n` と同じ数え方に
//! 揃えてあり、欠けた欄は空文字として扱う(awk と同じ)。

/// 1 始まりの欄番号で引く(awk の `$n`)。無ければ空。
fn f<'a>(fields: &[&'a str], n: usize) -> &'a str {
    fields.get(n - 1).copied().unwrap_or("")
}

/// usage_subkeys の 1 行(旧 bash 版では TSV だったもの)。
///
/// bash 版は TSV を `IFS=$'\t' read` で読んでいたため、空欄(失効しない subkey の
/// expires など)が連続タブとして潰れて後続の欄がずれる不具合があった。ここでは構造体
/// で持つのでずれない(意図的な修正、#414 の報告参照)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sub {
    pub primary_fpr: String,
    pub primary_uid: String,
    pub keyid: String,
    pub subkey_fpr: String,
    pub created: String,
    /// 空なら失効しない
    pub expires: String,
    pub revoked: bool,
    /// gpg の生の capability 欄("s" / "e" 等)
    pub cap: String,
}

impl Sub {
    pub fn created_epoch(&self) -> i64 {
        self.created.parse().unwrap_or(0)
    }
}

/// `usage_subkeys <cap-char> [selector] [on-disk]` の awk と同じ走査。
///
/// `ondisk_only` が真なら on-disk の秘密素材(`--with-colons` 欄 15 == "+"、GnuPG
/// DETAILS。`crates/sign-prewarm` の `key_is_on_disk` と同じ判定)に限る — card-backed
/// (token S/N)と stub(#)は除外する。
pub fn usage_subkeys(listing: &str, cap: char, ondisk_only: bool) -> Vec<Sub> {
    let mut out = Vec::new();
    let (mut pfpr, mut puid) = (String::new(), String::new());
    let mut insec = false;
    let mut pending = false;
    // ssb 行で控える欄
    let (mut revoked, mut cap_field, mut created, mut expires, mut keyid) = (
        false,
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    );
    for line in listing.lines() {
        let fl: Vec<&str> = line.split(':').collect();
        let kind = f(&fl, 1);
        if kind == "sec" {
            pfpr.clear();
            puid.clear();
            insec = true;
            continue;
        }
        if kind == "fpr" && insec {
            pfpr = f(&fl, 10).to_string();
            insec = false;
            continue;
        }
        if kind == "uid" && puid.is_empty() {
            puid = f(&fl, 10).to_string();
            continue;
        }
        if kind == "ssb" {
            revoked = f(&fl, 2) == "r";
            cap_field = f(&fl, 12).to_string();
            created = f(&fl, 6).to_string();
            expires = f(&fl, 7).to_string();
            keyid = f(&fl, 5).to_string();
            let loc = f(&fl, 15);
            let cap_match = cap_field.contains(cap);
            let loc_match = !ondisk_only || loc == "+";
            pending = cap_match && loc_match;
            continue;
        }
        if kind == "fpr" && pending {
            out.push(Sub {
                primary_fpr: pfpr.clone(),
                primary_uid: puid.clone(),
                keyid: keyid.clone(),
                subkey_fpr: f(&fl, 10).to_string(),
                created: created.clone(),
                expires: expires.clone(),
                revoked,
                cap: cap_field.clone(),
            });
            pending = false;
        }
    }
    out
}

/// `resolve_primary_key`: `sec` の直後の `fpr` 行の欄 10 を行ごとに返す。
pub fn primary_fprs(listing: &str) -> Vec<String> {
    let mut pending = false;
    let mut out = Vec::new();
    for line in listing.lines() {
        let fl: Vec<&str> = line.split(':').collect();
        match f(&fl, 1) {
            "sec" => pending = true,
            "fpr" if pending => {
                out.push(f(&fl, 10).to_string());
                pending = false;
            }
            _ => {}
        }
    }
    out
}

/// `primary_uid`: 最初の `sec` 以降で最初の `uid` 行の欄 10。
pub fn primary_uid(listing: &str) -> String {
    let mut insec = false;
    for line in listing.lines() {
        let fl: Vec<&str> = line.split(':').collect();
        match f(&fl, 1) {
            "sec" => insec = true,
            "uid" if insec => return f(&fl, 10).to_string(),
            "pub" => insec = false,
            _ => {}
        }
    }
    String::new()
}

/// `find_subkey_row`: keyid が一致する最初の `ssb` の (revoked, capability, location)。
pub fn find_subkey_row(listing: &str, keyid: &str) -> Option<(bool, String, String)> {
    listing.lines().find_map(|line| {
        let fl: Vec<&str> = line.split(':').collect();
        (f(&fl, 1) == "ssb" && f(&fl, 5) == keyid).then(|| {
            (
                f(&fl, 2) == "r",
                f(&fl, 12).to_string(),
                f(&fl, 15).to_string(),
            )
        })
    })
}

/// `gpg --with-colons --show-keys file` から、最初の `pub` の次の `fpr`(欄 10)。
pub fn pubkey_fpr(listing: &str) -> Option<String> {
    let mut want = false;
    for line in listing.lines() {
        let fl: Vec<&str> = line.split(':').collect();
        match f(&fl, 1) {
            "pub" => want = true,
            "fpr" if want => return Some(f(&fl, 10).to_string()),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // 実機の --with-colons --with-fingerprint を雛形にした合成入力。
    // field 15 は on-disk "+" / card = token serial / stub "#"。
    const MIXED_E: &str = "\
sec:u:255:22:AAAAAAAAAAAAAAAA:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
uid:u::::1::HASH::Selftest <s@example.invalid>::::::::::0:
ssb:u:255:18:CARDEEEEEEEEEEEE:1753115911:1784651911:::::e:::D2760001240100000006246379980000::cv25519::
fpr:::::::::CARDCARDCARDCARDCARDCARDCARDCARDCARDEEEE:
ssb:u:255:18:DISKEEEEEEEEEEEE:1783930808:1815466808:::::e:::+::cv25519::
fpr:::::::::57B25182FB450B06570860488608A3F925E329CC:
";

    #[test]
    fn ondisk_filter_excludes_card_backed() {
        // on-disk フィルタ(#252, R1-B-1)の回帰: card-backed な subkey は「現行」判定にも
        // revoke 候補にも入らない。
        assert_eq!(usage_subkeys(MIXED_E, 'e', false).len(), 2);
        let ondisk = usage_subkeys(MIXED_E, 'e', true);
        assert_eq!(ondisk.len(), 1);
        assert_eq!(ondisk[0].keyid, "DISKEEEEEEEEEEEE");
        assert_eq!(ondisk[0].primary_uid, "Selftest <s@example.invalid>");
        assert_eq!(
            ondisk[0].primary_fpr,
            "92E7B05978F0FE4E5500F6F76CFC837175BE257E"
        );
        assert!(usage_subkeys(MIXED_E, 's', false).is_empty());
    }

    #[test]
    fn empty_expiry_keeps_columns_aligned() {
        let l = "\
sec:u:255:22:AA:1::::u:::scESCA:::+::ed25519:::0:
fpr:::::::::PFPR:
uid:u::::1::H::Name::::::::::0:
ssb:r:255:22:KEY1:100::::::s:::+::ed25519::
fpr:::::::::SFPR:
";
        let rows = usage_subkeys(l, 's', false);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].expires.is_empty());
        assert!(rows[0].revoked);
        assert_eq!(rows[0].cap, "s");
    }

    #[test]
    fn helpers() {
        assert_eq!(
            primary_fprs(MIXED_E),
            vec!["92E7B05978F0FE4E5500F6F76CFC837175BE257E"]
        );
        assert_eq!(primary_uid(MIXED_E), "Selftest <s@example.invalid>");
        let (rev, cap, loc) = find_subkey_row(MIXED_E, "CARDEEEEEEEEEEEE").unwrap();
        assert!(!rev);
        assert_eq!(cap, "e");
        assert_eq!(loc, "D2760001240100000006246379980000");
        assert!(find_subkey_row(MIXED_E, "NOPE").is_none());
        assert_eq!(
            pubkey_fpr("pub:u:::AA\nfpr:::::::::XYZ:\n").as_deref(),
            Some("XYZ")
        );
    }
}
