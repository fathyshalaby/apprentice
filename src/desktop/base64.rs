//! Standard base64 (RFC 4648, with padding), for the few places Hex sends
//! bytes as text: OpenCode's Basic sign-in and images sent to a local model.

/// `bytes` in standard base64 with `=` padding.
pub(crate) fn encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from_be_bytes([
            0,
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ]);
        let ch = |shift: u32| char::from(TABLE[((n >> shift) & 63) as usize]);
        out.extend([
            ch(18),
            ch(12),
            if chunk.len() > 1 { ch(6) } else { '=' },
            if chunk.len() > 2 { ch(0) } else { '=' },
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encodes_the_rfc_4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(input.as_bytes()), expected, "{input:?}");
        }
    }

    #[test]
    fn encodes_every_byte_value() {
        assert_eq!(encode(&[0xff, 0xfe, 0x00]), "//4A");
        assert_eq!(encode(b"opencode:secret"), "b3BlbmNvZGU6c2VjcmV0");
    }
}
