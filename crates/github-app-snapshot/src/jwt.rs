//! GitHub App の JWT(GitHub Docs, "Generating a JSON Web Token (JWT) for a
//! GitHub App"、取得 2026-09-29): RS256、iat は 60 秒前、exp は 10 分以内、
//! iss = App ID。署名だけ `openssl dgst -sha256 -sign` に任せる(bash 版と同じ。
//! PEM は呼び出し元が 0600 の一時ファイルに置いて渡す)。

use crate::config::Config;
use hook_io::jqfmt::J;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// パディング無しの base64url(bash 版の `openssl base64 -A | tr '+/' '-_' | tr -d '='`)。
pub fn base64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = match *chunk {
            [a] => (a as u32) << 16,
            [a, b] => (a as u32) << 16 | (b as u32) << 8,
            [a, b, c] => (a as u32) << 16 | (b as u32) << 8 | c as u32,
            _ => unreachable!(),
        };
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(B64URL[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// `build_jwt <app_id> <pem_file>`。署名に失敗したら Err(bash 版は壊れた JWT で
/// GitHub に 401 を返させていた — 意図的に変えた箇所)。
pub fn build_jwt(cfg: &Config, id: &str, pem_file: &std::path::Path) -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = J::obj(vec![
        ("iat", J::Num((now.saturating_sub(60)).to_string())),
        ("exp", J::Num((now + 540).to_string())),
        ("iss", J::str(id)),
    ])
    .compact();
    let signing_input = format!(
        "{}.{}",
        base64url_encode(br#"{"alg":"RS256","typ":"JWT"}"#),
        base64url_encode(payload.as_bytes())
    );
    let mut child = Command::new(&cfg.openssl_bin)
        .args(["dgst", "-sha256", "-sign"])
        .arg(pem_file)
        .arg("-binary")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", cfg.openssl_bin))?;
    child
        .stdin
        .take()
        .ok_or("openssl: no stdin")?
        .write_all(signing_input.as_bytes())
        .map_err(|e| format!("openssl: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("openssl: {e}"))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err("openssl failed to sign the JWT (is the PEM valid?)".to_string());
    }
    Ok(format!("{signing_input}.{}", base64url_encode(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use github_audit::jq::{j_get, parse_ordered};

    fn b64url_decode(s: &str) -> Vec<u8> {
        let mut acc = 0u32;
        let mut bits = 0;
        let mut out = Vec::new();
        for c in s.bytes() {
            let v = B64URL.iter().position(|&x| x == c).unwrap() as u32;
            acc = acc << 6 | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        out
    }

    fn openssl(args: &[&str]) {
        let s = Command::new("openssl")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(s.success(), "openssl {args:?}");
    }

    #[test]
    fn base64url_matches_rfc4648_vectors() {
        assert_eq!(base64url_encode(b""), "");
        assert_eq!(base64url_encode(b"f"), "Zg");
        assert_eq!(base64url_encode(b"fo"), "Zm8");
        assert_eq!(base64url_encode(b"foo"), "Zm9v");
        assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
    }

    /// bash#1〜#5(selftest の build_jwt): 実 openssl で JWT を作り、header・iss・
    /// exp-iat の窓・iat が過去・RS256 署名が対の公開鍵で検証できることを見る。
    #[test]
    fn jwt_claims_and_signature_verify() {
        let tmp = tempfile::tempdir().unwrap();
        let priv_pem = tmp.path().join("test.pem");
        let pub_pem = tmp.path().join("test.pub");
        openssl(&["genrsa", "-out", priv_pem.to_str().unwrap(), "2048"]);
        openssl(&[
            "rsa",
            "-in",
            priv_pem.to_str().unwrap(),
            "-pubout",
            "-out",
            pub_pem.to_str().unwrap(),
        ]);
        let cfg = Config::from_env();
        let jwt = build_jwt(&cfg, "123456", &priv_pem).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header = parse_ordered(&String::from_utf8(b64url_decode(parts[0])).unwrap()).unwrap();
        let payload = parse_ordered(&String::from_utf8(b64url_decode(parts[1])).unwrap()).unwrap();
        // bash#1: header
        assert_eq!(j_get(&header, "alg"), Some(&J::str("RS256")));
        assert_eq!(j_get(&header, "typ"), Some(&J::str("JWT")));
        // bash#2: iss
        assert_eq!(j_get(&payload, "iss"), Some(&J::str("123456")));
        let num = |k: &str| match j_get(&payload, k) {
            Some(J::Num(n)) => n.parse::<u64>().unwrap(),
            other => panic!("{k}: {other:?}"),
        };
        // bash#3: 0 < exp - iat <= 600
        let (iat, exp) = (num("iat"), num("exp"));
        assert!(exp > iat && exp - iat <= 600);
        // bash#4: iat は過去(時計ずれ対策)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(iat < now);
        // bash#5: RS256 署名が対の公開鍵で検証できる
        let sig = tmp.path().join("sig.bin");
        let input = tmp.path().join("input.bin");
        std::fs::write(&sig, b64url_decode(parts[2])).unwrap();
        std::fs::write(&input, format!("{}.{}", parts[0], parts[1])).unwrap();
        openssl(&[
            "dgst",
            "-sha256",
            "-verify",
            pub_pem.to_str().unwrap(),
            "-signature",
            sig.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
    }

    #[test]
    fn signing_failure_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("bad.pem");
        std::fs::write(&bad, "not a pem").unwrap();
        assert!(build_jwt(&Config::from_env(), "1", &bad).is_err());
    }
}
