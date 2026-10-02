//! IMAP modified UTF-7 (RFC 3501 §5.1.3), the encoding mailbox names use on the wire.

use base64::{Engine, alphabet, engine::GeneralPurpose, engine::general_purpose::NO_PAD};

const IMAP_B64: GeneralPurpose = GeneralPurpose::new(&imap_alphabet(), NO_PAD);

const fn imap_alphabet() -> alphabet::Alphabet {
    match alphabet::Alphabet::new(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,",
    ) {
        Ok(a) => a,
        Err(_) => panic!("valid alphabet"),
    }
}

/// Wire name → display name. Malformed input is returned unchanged rather than failing.
pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            return s.to_owned();
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            let Ok(bytes) = IMAP_B64.decode(chunk) else {
                return s.to_owned();
            };
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes(*c))
                .collect();
            match String::from_utf16(&units) {
                Ok(t) => out.push_str(&t),
                Err(_) => return s.to_owned(),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Display name → wire name.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if !pending.is_empty() {
            let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
            out.push('&');
            out.push_str(&IMAP_B64.encode(bytes));
            out.push('-');
            pending.clear();
        }
    };
    for ch in s.chars() {
        if (' '..='~').contains(&ch) {
            flush(&mut pending, &mut out);
            if ch == '&' {
                out.push_str("&-");
            } else {
                out.push(ch);
            }
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(ch.encode_utf16(&mut buf));
        }
    }
    flush(&mut pending, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        for (display, wire) in [
            ("INBOX", "INBOX"),
            ("Tom & Jerry", "Tom &- Jerry"),
            ("Café", "Caf&AOk-"),
            ("~peter/mail/台北/日本語", "~peter/mail/&U,BTFw-/&ZeVnLIqe-"),
            ("Хувийн", "&BCUEQwQyBDgEOQQ9-"),
        ] {
            assert_eq!(encode(display), wire, "encode {display}");
            assert_eq!(decode(wire), display, "decode {wire}");
        }
    }

    #[test]
    fn malformed_is_left_alone() {
        assert_eq!(decode("broken&AOk"), "broken&AOk");
    }
}
