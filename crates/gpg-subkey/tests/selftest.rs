//! `scripts/gpg-subkey --selftest` の全ケース(47 チェック)を再現する統合テスト
//! (#414、docs/rust-migration.md の段 1-2)。
//!
//! 対象は既定で cargo bin(Rust 版)。`GPG_SUBKEY_ORACLE` に bash 版のパスを入れると、
//! CLI 経由のケースを bash 版に向けて走らせる(移植前の緑確認用)。
//!
//! bash selftest と同じ方針で、generate/rotate/revoke/status/remind/export は
//! **実 GnuPG** を、使い捨ての空 passphrase primary 鍵 + 隔離した GNUPGHOME で走らせる
//! (addkey/revkey の command-file の仕組みそのものが検証対象なので、gpg はスタブに
//! しない)。実際の gpg 鍵ストア(~/.gnupg)には決して触れない: GNUPGHOME は一時
//! ディレクトリ、`GPG_SUBKEY_PINENTRY_MODE=loopback` + `GPG_SUBKEY_PASSPHRASE=''` は
//! この fixture 鍵だけを非対話に解錠する。例外は 2 つ:
//! - on-disk/card-backed フィルタの回帰(#252, R1-B-1): 実カードは fixture では偽装
//!   できないので、合成した `--with-colons` 出力をスタブ gpg 経由で通す(sign-prewarm の
//!   テストの card/stub 合成入力と同じ手法)。`colon.rs` の単体テストと本ファイルの
//!   `card_backed_*` が担う。
//! - sync: `gh` と `curl` は PATH 上のスタブ — 実 GitHub アカウントにも実 keyserver にも
//!   決して触れない。
//!
//! 空の passphrase fixture 鍵・パスフレーズは公開リポジトリに置いても無害な使い捨て値
//! (ログにも出さない)。

use gpg_subkey::Cfg;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Out {
    rc: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn both(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

fn target() -> Command {
    match std::env::var_os("GPG_SUBKEY_ORACLE") {
        Some(script) => {
            let mut c = Command::new("bash");
            c.arg(script);
            c
        }
        None => Command::new(env!("CARGO_BIN_EXE_gpg-subkey")),
    }
}

fn write_exec(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fx {
    dir: tempfile::TempDir,
    gnupghome: PathBuf,
    fpr: String,
}

impl Fx {
    fn new() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let gnupghome = dir.path().join("gnupg");
        std::fs::create_dir(&gnupghome).unwrap();
        std::fs::set_permissions(&gnupghome, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            gnupghome.join("gpg-agent.conf"),
            "allow-loopback-pinentry\n",
        )
        .unwrap();
        let st = Command::new("gpg")
            .env("GNUPGHOME", &gnupghome)
            .args([
                "--batch",
                "--passphrase",
                "",
                "--pinentry-mode",
                "loopback",
                "--quick-generate-key",
                "Selftest <selftest@example.invalid>",
                "ed25519",
                "cert",
                "never",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("gpg が PATH に必要(nix develop の devShell が提供する)");
        assert!(st.success(), "fixture 鍵の生成に失敗");
        let list = Command::new("gpg")
            .env("GNUPGHOME", &gnupghome)
            .args(["--list-secret-keys", "--with-colons", "--with-fingerprint"])
            .output()
            .unwrap();
        let fpr = String::from_utf8_lossy(&list.stdout)
            .lines()
            .skip_while(|l| !l.starts_with("sec:"))
            .find(|l| l.starts_with("fpr:"))
            .and_then(|l| l.split(':').nth(9))
            .unwrap()
            .to_string();
        Fx {
            dir,
            gnupghome,
            fpr,
        }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn cmd(&self) -> Command {
        let mut c = target();
        c.env("GNUPGHOME", &self.gnupghome)
            .env("GPG_SUBKEY_PINENTRY_MODE", "loopback")
            .env("GPG_SUBKEY_PASSPHRASE", "")
            // 実環境の dotfiles/host マーカーを読まない(hermetic)
            .env("XDG_CONFIG_HOME", self.p("xdg-default"))
            .stdin(Stdio::null());
        c
    }

    fn gs(&self, args: &[&str]) -> Out {
        Self::collect(self.cmd().args(args).output().unwrap())
    }

    fn collect(o: std::process::Output) -> Out {
        Out {
            rc: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    fn gpg(&self, args: &[&str]) -> Vec<u8> {
        Command::new("gpg")
            .env("GNUPGHOME", &self.gnupghome)
            .args(args)
            .output()
            .unwrap()
            .stdout
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        // 後始末: fixture の gpg-agent を止める(bash 版は放置していた)。
        let _ = Command::new("gpgconf")
            .env("GNUPGHOME", &self.gnupghome)
            .args(["--kill", "gpg-agent"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// usage_subkeys 相当(in-process)。GNUPGHOME は呼び出し側が env に入れている。
fn rows(cap: char, fpr: &str) -> Vec<gpg_subkey::colon::Sub> {
    Cfg::from_env().usage_subkeys(cap, fpr, false)
}

#[test]
fn real_gnupg_flow() {
    let fx = Fx::new();
    // in-process の usage_subkeys / extract_latest_usage_keyid が同じ鍵束を見る。
    std::env::set_var("GNUPGHOME", &fx.gnupghome);
    let fpr = fx.fpr.clone();
    let cfg = Cfg::from_env();

    // 1. edit_key_extra_args は本番(env 未設定)で空 — lib の単体テスト
    //    `production_extra_args_are_empty` が担う(ここの Cfg::from_env は GNUPGHOME しか
    //    触らないので、passphrase 系が未設定のとき空であることだけを確認する)。
    let prod = Cfg {
        pinentry_mode: None,
        passphrase: None,
        ..cfg.clone()
    };
    assert_eq!(
        prod.edit_key_extra_args().len(),
        0,
        "1 edit_key_extra_args: 本番(env未設定)では空配列"
    );

    // 2-3. generate
    let g = fx.gs(&["generate", "--key", &fpr, "--validity", "1d"]);
    assert_eq!(g.rc, 0, "generate は成功する: {}", g.stderr);
    let first_keyid = first_line(&g.stdout).to_string();
    assert!(
        !first_keyid.is_empty(),
        "2 generate: 新しい keyid を出力する"
    );
    assert_eq!(
        rows('s', &fpr).len(),
        1,
        "3 generate 後: [S] subkey が 1 本"
    );

    // 4-5. status
    let st = fx.gs(&["status"]).stdout;
    assert!(
        st.contains(&first_keyid),
        "4 status: 生成した subkey を含む"
    );
    assert!(
        st.contains(&format!("sub={first_keyid} [S]:")),
        "5 status: [S] ラベルを付ける: {st}"
    );

    // 6-7. remind
    assert_eq!(
        fx.gs(&["remind", "--threshold", "2"]).rc,
        1,
        "6 remind: 閾値内なら exit 1"
    );
    assert_eq!(
        fx.gs(&["remind", "--threshold", "0"]).rc,
        0,
        "7 remind: 閾値外(0日)なら exit 0"
    );

    // export: keys/selftest.pub を rotate 前の状態(コミット済みの dotfiles checkout が
    // まだ持っているもの)で seed し、下の export が rotate 後の鍵束に対して本物の差分を
    // 報告できるようにする。
    let repo = fx.p("repo");
    std::fs::create_dir_all(repo.join("keys")).unwrap();
    std::fs::write(
        repo.join("keys/selftest.pub"),
        fx.gpg(&["--armor", "--export", &fpr]),
    )
    .unwrap();
    let repo_s = repo.to_str().unwrap();

    // 8-9. rotate --revoke-old
    let r = fx.gs(&["rotate", "--key", &fpr, "--validity", "1d", "--revoke-old"]);
    assert_eq!(r.rc, 0, "rotate は成功する: {}", r.stderr);
    let second_keyid = first_line(&r.stdout).to_string();
    assert_ne!(
        second_keyid, first_keyid,
        "8 rotate: 新しい keyid は generate と異なる"
    );
    let old = rows('s', &fpr)
        .into_iter()
        .find(|s| s.keyid == first_keyid)
        .unwrap();
    assert!(
        old.revoked,
        "9 rotate --revoke-old: 旧 [S] subkey が revoked=1"
    );

    // 10-12. export
    let e = fx
        .gs(&["export", "--repo", repo_s, "--identity", "selftest"])
        .stdout;
    assert!(e.contains("更新しました"), "10 export: 更新を報告する");
    assert!(
        e.contains(&second_keyid),
        "11 export: 新 subkey の次手順を印刷する"
    );
    let e = fx
        .gs(&["export", "--repo", repo_s, "--identity", "selftest"])
        .stdout;
    assert!(
        e.contains("変更はありません"),
        "12 export: 変更なしなら再実行で no-op"
    );

    // --- [E] カバレッジ(#252) ---
    let g = fx.gs(&[
        "generate",
        "--key",
        &fpr,
        "--usage",
        "encrypt",
        "--validity",
        "1d",
    ]);
    assert_eq!(g.rc, 0, "generate --usage encrypt は成功する: {}", g.stderr);
    let e_first = first_line(&g.stdout).to_string();
    assert!(
        !e_first.is_empty(),
        "13 generate --usage encrypt: 新しい keyid を出力する"
    );
    assert!(
        g.stderr.contains(&e_first),
        "14 generate --usage encrypt: 次手順(再暗号化)を案内する"
    );
    assert_eq!(
        rows('e', &fpr).len(),
        1,
        "15 generate --usage encrypt 後: [E] subkey が 1 本"
    );
    let st = fx.gs(&["status"]).stdout;
    assert!(
        st.contains(&format!("sub={e_first} [E]:")),
        "16 status: [E] subkey も報告する: {st}"
    );

    let r = fx.gs(&[
        "rotate",
        "--key",
        &fpr,
        "--usage",
        "encrypt",
        "--validity",
        "1d",
        "--revoke-old",
    ]);
    assert_eq!(r.rc, 0, "rotate --usage encrypt は成功する: {}", r.stderr);
    let e_second = first_line(&r.stdout).to_string();
    assert_ne!(
        e_second, e_first,
        "17 rotate --usage encrypt: 新しい keyid は generate と異なる"
    );
    let old = rows('e', &fpr)
        .into_iter()
        .find(|s| s.keyid == e_first)
        .unwrap();
    assert!(
        old.revoked,
        "18 rotate --usage encrypt --revoke-old: 旧 on-disk [E] subkey が revoked=1"
    );
    assert_eq!(
        cfg.extract_latest_usage_keyid('s', &fpr),
        second_keyid,
        "19 encrypt の rotate は [S] の現行 subkey に影響しない"
    );

    // --- revoke (standalone) ---
    let third_keyid = first_line(
        &fx.gs(&["generate", "--key", &fpr, "--validity", "1d"])
            .stdout,
    )
    .to_string();
    assert!(!third_keyid.is_empty());
    let rv = fx.gs(&["revoke", "--key", &fpr, "--subkey", &third_keyid]);
    assert_eq!(rv.rc, 0, "25 revoke: 単独実行は exit 0: {}", rv.both());
    assert!(
        rv.both().contains(&third_keyid),
        "26 revoke: revoke した keyid を報告する"
    );
    let rs = rows('s', &fpr);
    assert!(
        rs.iter().find(|s| s.keyid == third_keyid).unwrap().revoked,
        "27 revoke: 対象 subkey が revoked=1 になる"
    );
    assert!(
        !rs.iter().find(|s| s.keyid == second_keyid).unwrap().revoked,
        "28 revoke: 現行 [S] subkey には影響しない"
    );
    let again = fx.gs(&["revoke", "--key", &fpr, "--subkey", &third_keyid]);
    assert_eq!(again.rc, 1, "29 revoke: 既に revoked な subkey は失敗する");
    assert!(
        again.stderr.contains("既に revoked"),
        "30 revoke: 既に revoked のエラーメッセージ"
    );
    let missing = fx.gs(&["revoke", "--key", &fpr, "--subkey", "NOSUCHKEYID0000"]);
    assert_eq!(missing.rc, 1, "31 revoke: 存在しない keyid は失敗する");

    // --- sync(gh / curl はスタブ) ---
    // keys/selftest.pub はこの時点で 2 通りに古い: (a) 最後の export の後に [S] が rotate
    // された(github/keyserver 関連)、(b) 最後の export 時点では [E] subkey がまだ無かった
    // (export/keyserver 関連)。下の GitHub 登録は、[E] 対応が入る前と同じく github drift
    // 検出がカバーされ続けるよう、意図的に旧 [S] 鍵($first_keyid、既に revoked)を指す。
    let bin = fx.p("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let primary_short = &fpr[fpr.len() - 16..];
    std::fs::write(
        fx.p("gh-list.json"),
        serde_json::json!([{"id": 999, "key_id": primary_short,
            "subkeys": [{"key_id": first_keyid, "can_sign": true, "revoked": false}]}])
        .to_string(),
    )
    .unwrap();
    write_exec(
        &bin.join("gh"),
        &format!(
            "#!/usr/bin/env bash\nall=\"$*\"\ncase \"$all\" in\n  *\"-X DELETE\"*) printf 'delete\\n' >>\"{d}/gh-calls.txt\"; exit 0 ;;\n  *\"-X POST\"*) printf 'post\\n' >>\"{d}/gh-calls.txt\"; exit 0 ;;\n  *\"user/gpg_keys\"*) cat \"{d}/gh-list.json\"; exit 0 ;;\nesac\nexit 1\n",
            d = fx.dir.path().display()
        ),
    );
    write_exec(
        &bin.join("curl"),
        "#!/usr/bin/env bash\nout=\"\"\nprev=\"\"\nfor a in \"$@\"; do\n  [[ \"$prev\" == \"-o\" ]] && out=\"$a\"\n  prev=\"$a\"\ndone\nif [[ -n \"${GPG_SUBKEY_TEST_CURL_RESPONSE:-}\" && -f \"${GPG_SUBKEY_TEST_CURL_RESPONSE:-}\" ]]; then\n  cp \"$GPG_SUBKEY_TEST_CURL_RESPONSE\" \"$out\"\n  exit 0\nfi\nexit 22\n",
    );
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let sync = |args: &[&str], xdg: &str, curl_resp: Option<&Path>| -> Out {
        let mut c = fx.cmd();
        c.env("PATH", &path).env("XDG_CONFIG_HOME", fx.p(xdg));
        if let Some(r) = curl_resp {
            c.env("GPG_SUBKEY_TEST_CURL_RESPONSE", r);
        }
        Fx::collect(
            c.args(["sync", "--repo", repo_s, "--identity", "selftest"])
                .args(args)
                .output()
                .unwrap(),
        )
    };
    std::fs::create_dir_all(fx.p("xdg-default")).unwrap();

    let s = sync(&[], "xdg-default", None);
    let so = s.both();
    assert_eq!(s.rc, 1, "34 sync: drift があれば exit 1: {so}");
    assert!(
        so.contains("GitHub"),
        "35 sync: github drift を報告する(旧 [S] key を stub 登録している)"
    );
    assert!(
        so.contains(&format!("現行 [S] subkey {second_keyid}")),
        "36 sync: keyserver drift で [S] を報告する: {so}"
    );
    assert!(
        so.contains(&format!("現行 [E] subkey {e_second}")),
        "37 sync: keyserver drift で [E] も報告する(on-disk [E] が存在するため): {so}"
    );
    assert_eq!(
        so.lines()
            .filter(|l| l.contains("keys/selftest.pub"))
            .count(),
        1,
        "38 sync: export drift も報告する([E] 生成後の再 export が未反映)"
    );

    // --fix: curl スタブで keyserver drift を無効化し(現行 export を keyserver の
    // 「公開済み」応答として返す — 主鍵の export は全 subkey を含むので [S]/[E] を一度に
    // 覆う)、スタブの github + export 経路だけを通す — この実行は実ネットワークへ
    // `gpg --send-keys` を決して送らない。
    let ks_current = fx.p("keyserver-current.bin");
    std::fs::write(&ks_current, fx.gpg(&["--export", &fpr])).unwrap();
    let _ = std::fs::remove_file(fx.p("gh-calls.txt"));
    let f = sync(&["--fix", "--yes"], "xdg-default", Some(&ks_current));
    assert_eq!(f.rc, 0, "39 sync --fix --yes: exit 0: {}", f.both());
    assert!(
        f.both().contains("keys/selftest.pub を更新しました"),
        "40 sync --fix: export を更新する"
    );
    let calls = std::fs::read_to_string(fx.p("gh-calls.txt")).unwrap_or_default();
    assert!(
        calls.contains("delete"),
        "41 sync --fix: GitHub の delete を呼ぶ"
    );
    assert!(
        calls.contains("post"),
        "42 sync --fix: GitHub の post を呼ぶ"
    );

    // #423: host-file 不在のサイレントスキップ潰し(正・負両経路)。実環境の
    // ${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/host を読まないよう、どの呼び出しも
    // XDG_CONFIG_HOME を専用の一時ディレクトリへ差し替える(hermetic)。
    let active_sign_keyid = cfg.extract_latest_usage_keyid('s', &fpr);
    let active_sign_fpr = cfg
        .usage_subkeys('s', &fpr, false)
        .into_iter()
        .find(|s| s.keyid == active_sign_keyid)
        .unwrap()
        .subkey_fpr;

    // 負経路 1: マーカー未設置 → hostname フォールバックで解決したホスト名に対応する
    // home/hosts/<host>.nix が無い。
    std::fs::create_dir_all(fx.p("xdg-nomarker")).unwrap();
    let h = sync(&[], "xdg-nomarker", None).both();
    assert!(
        h.contains("hostname フォールバック"),
        "43 sync: マーカー未設置ホストで hostfile drift を報告する(hostname フォールバック由来)"
    );
    assert!(
        h.contains("検査できません"),
        "44 sync: hostfile drift のメッセージが出る"
    );

    // 負経路 2: マーカーは設置されているが、対応する home/hosts/<host>.nix が無い。
    std::fs::create_dir_all(fx.p("xdg-marker-nohost/dotfiles")).unwrap();
    std::fs::write(fx.p("xdg-marker-nohost/dotfiles/host"), "ghost-host\n").unwrap();
    let h = sync(&[], "xdg-marker-nohost", None).both();
    assert!(
        h.contains("dotfiles/host マーカー"),
        "45 sync: マーカーはあるが home/hosts/<host>.nix が無ければ hostfile drift(マーカー由来)"
    );
    assert!(
        !h.contains("hostname フォールバック"),
        "46 sync: マーカー由来では hostname フォールバックの文言は出ない"
    );

    // 正経路: マーカーが指すホストの home/hosts/<host>.nix が存在し、identity と現行 [S]
    // subkey フィンガープリントの両方が一致する → hostfile drift は出ない(github/keyserver
    // は stub が状態を永続化しないため引き続き drift として出うるが、ここでは hostfile 固有の
    // 文言だけを見る)。
    std::fs::create_dir_all(fx.p("xdg-marker-ok/dotfiles")).unwrap();
    std::fs::create_dir_all(repo.join("home/hosts")).unwrap();
    std::fs::write(fx.p("xdg-marker-ok/dotfiles/host"), "selftest-host\n").unwrap();
    std::fs::write(
        repo.join("home/hosts/selftest-host.nix"),
        format!(
            "{{\n  imports = [ ../identities/selftest.nix ];\n  programs.git.signing.key = \"{active_sign_fpr}\";\n}}\n"
        ),
    )
    .unwrap();
    let h = sync(&[], "xdg-marker-ok", None).both();
    assert!(
        !h.contains("検査できません"),
        "47 sync: host-file が存在し identity・signing key とも一致すれば hostfile drift は出ない: {h}"
    );
}

// --- on-disk フィルタの回帰(#252, R1-B-1) -----------------------------------
// 実カードは fixture 内で偽装できないため、合成した --with-colons 出力をスタブ gpg 経由で
// 通し、card-backed な subkey が「現行」判定にも revoke 候補にも入らないことを検証する。

const LIST_MIXED_E: &str = "\
sec:u:255:22:AAAAAAAAAAAAAAAA:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:u:255:18:CARDEEEEEEEEEEEE:1753115911:1784651911:::::e:::D2760001240100000006246379980000::cv25519::
fpr:::::::::CARDCARDCARDCARDCARDCARDCARDCARDCARDEEEE:
ssb:u:255:18:DISKEEEEEEEEEEEE:1783930808:1815466808:::::e:::+::cv25519::
fpr:::::::::57B25182FB450B06570860488608A3F925E329CC:
";

fn list_stub(dir: &Path) -> PathBuf {
    std::fs::write(dir.join("list-mixed-e.txt"), LIST_MIXED_E).unwrap();
    let stub = dir.join("gpg-list-stub");
    write_exec(
        &stub,
        "#!/usr/bin/env bash\ncase \"$*\" in\n  *--list-secret-keys*) cat \"$GPG_SUBKEY_TEST_LIST_FILE\" ;;\n  *) exit 1 ;;\nesac\n",
    );
    stub
}

#[test]
fn card_backed_filter_via_stub_gpg() {
    let dir = tempfile::tempdir().unwrap();
    let stub = list_stub(dir.path());
    std::env::set_var(
        "GPG_SUBKEY_TEST_LIST_FILE",
        dir.path().join("list-mixed-e.txt"),
    );
    let cfg = Cfg {
        gpg_bin: stub.to_string_lossy().into_owned(),
        herdr_bin: "herdr".into(),
        pinentry_mode: None,
        passphrase: None,
    };
    let all = cfg.usage_subkeys('e', "", false);
    assert_eq!(
        all.len(),
        2,
        "20 on-disk フィルタなし: card/disk 両方の [E] を含む(2行)"
    );
    let ondisk = cfg.usage_subkeys('e', "", true);
    assert_eq!(
        ondisk.len(),
        1,
        "21 on-disk フィルタあり: card-backed [E] を除外する(1行)"
    );
    assert_eq!(
        ondisk[0].keyid, "DISKEEEEEEEEEEEE",
        "22 on-disk フィルタあり: 残る行は on-disk keyid"
    );
    assert!(
        !ondisk.iter().any(|s| s.keyid == "CARDEEEEEEEEEEEE"),
        "23 on-disk フィルタあり: card keyid は含まれない"
    );
    assert_eq!(
        cfg.extract_latest_usage_keyid('e', ""),
        "DISKEEEEEEEEEEEE",
        "24 extract_latest_usage_keyid: card-backed を「現行」として解決しない"
    );
}

#[test]
fn card_backed_subkey_is_refused_by_revoke() {
    let dir = tempfile::tempdir().unwrap();
    let stub = list_stub(dir.path());
    let o = target()
        .args([
            "revoke",
            "--key",
            "92E7B05978F0FE4E5500F6F76CFC837175BE257E",
            "--subkey",
            "CARDEEEEEEEEEEEE",
        ])
        .env("GPG_SUBKEY_GPG_BIN", &stub)
        .env(
            "GPG_SUBKEY_TEST_LIST_FILE",
            dir.path().join("list-mixed-e.txt"),
        )
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let o = Fx::collect(o);
    assert_eq!(o.rc, 1, "32 revoke: card-backed subkey は拒否する");
    assert!(
        o.stderr.contains("card-backed"),
        "33 revoke: card-backed 拒否のメッセージ"
    );
}

// --- 追加: bash 版の selftest が踏んでいなかった経路 --------------------------

#[test]
fn argument_and_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Fx::collect(
            target()
                .args(args)
                .env("GPG_SUBKEY_GPG_BIN", "/nonexistent/gpg")
                .stdin(Stdio::null())
                .output()
                .unwrap(),
        )
    };
    let _ = &dir;
    assert_eq!(run(&["generate"]).rc, 2);
    assert!(run(&["generate"]).stderr.contains("--key が必要です"));
    assert_eq!(run(&["generate", "--key", "x", "--algorithm", "rsa"]).rc, 2);
    assert_eq!(run(&["rotate", "--key", "x", "--usage", "auth"]).rc, 2);
    assert_eq!(run(&["revoke", "--key", "x"]).rc, 2);
    assert_eq!(run(&["export", "--bogus"]).rc, 2);
    assert_eq!(run(&["bogus"]).rc, 2);
    assert_eq!(run(&["--help"]).rc, 0);
    assert!(run(&["--help"])
        .stdout
        .starts_with("usage:\n  gpg-subkey generate"));
    assert_eq!(run(&[]).rc, 1, "引数なしは usage を stdout に出して 1");
}

#[test]
fn unknown_key_is_reported_not_silent() {
    // bash 版は set -e のせいでこのメッセージが出ず gpg の終了コード 2 で無言で落ちて
    // いた(意図的な修正、lib.rs 冒頭参照)。
    if std::env::var_os("GPG_SUBKEY_ORACLE").is_some() {
        return;
    }
    let fx = Fx::new();
    let g = fx.gs(&["generate", "--key", "NOSUCHKEY"]);
    assert_eq!(g.rc, 1);
    assert!(g.stderr.contains("に一致する秘密鍵がありません"));
    let r = fx.gs(&["rotate", "--key", "NOSUCHKEY"]);
    assert_eq!(r.rc, 1);
    assert!(r.stderr.contains("一意に特定できません"));
}

#[test]
fn secrets_never_reach_output() {
    // loopback の passphrase は fixture では空文字なので、非空値で漏れを検査する: 偽の
    // gpg が argv を記録し、gpg-subkey の stdout/stderr に値が現れないことを確かめる。
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("gpg");
    write_exec(
        &stub,
        "#!/usr/bin/env bash\ncase \"$*\" in\n  *--list-secret-keys*) cat \"$GPG_SUBKEY_TEST_LIST_FILE\" ;;\n  *--edit-key*) echo \"boom\" >&2; exit 2 ;;\nesac\n",
    );
    std::fs::write(
        dir.path().join("list.txt"),
        "sec:u:255:22:AAAAAAAAAAAAAAAA:1::::u:::scESCA:::+::ed25519:::0:\nfpr:::::::::PRIMARYFPR:\nuid:u::::1::H::Name::::::::::0:\n",
    )
    .unwrap();
    let o = Fx::collect(
        target()
            .args(["generate", "--key", "PRIMARYFPR"])
            .env("GPG_SUBKEY_GPG_BIN", &stub)
            .env("GPG_SUBKEY_TEST_LIST_FILE", dir.path().join("list.txt"))
            .env("GPG_SUBKEY_PINENTRY_MODE", "loopback")
            .env("GPG_SUBKEY_PASSPHRASE", "hunter2-secret")
            .stdin(Stdio::null())
            .output()
            .unwrap(),
    );
    if std::env::var_os("GPG_SUBKEY_ORACLE").is_none() {
        assert_eq!(o.rc, 1);
        assert!(o.stderr.contains("subkey 生成に失敗しました"));
    }
    assert!(!o.both().contains("hunter2-secret"));
}
