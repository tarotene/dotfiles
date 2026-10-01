//! `base64 -d` 相当の最小デコーダ。`gh api …/contents/…` の `.content` は
//! 60 桁ごとに `\n` が入った標準 base64 で、GNU `base64 -d` は改行を読み飛ばす。
//! 不正な文字に当たったら、そこまでにデコードできた分を返す(GNU base64 も
//! エラーの前までは出力する)。新しい依存を足すほどの量ではないので自前。

/// 戻り値: (デコード結果, 全体が正しい base64 だったか)
pub fn decode_lenient(input: &str) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    let mut padding = 0;
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b'\n' | b'\r' => continue,
            _ => return (out, false),
        };
        if padding > 0 {
            // 末尾の `=` の後に値が来るのは不正
            return (out, false);
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::decode_lenient;

    #[test]
    fn decodes_with_padding_and_newlines() {
        assert_eq!(decode_lenient("aGVsbG8=").0, b"hello");
        assert_eq!(decode_lenient("aGVs\nbG8=\n"), (b"hello".to_vec(), true));
        assert_eq!(decode_lenient("aGk=").0, b"hi");
        assert_eq!(decode_lenient("").0, b"");
    }

    #[test]
    fn invalid_character_keeps_the_prefix_and_reports_failure() {
        assert_eq!(decode_lenient("aGVs!bG8="), (b"hel".to_vec(), false));
    }
}
