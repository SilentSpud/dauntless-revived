//! Credentials out of anything logged (`Redact` in `middleware/BodyLog.ts`): JWTs (some routes carry
//! one in the path) become `<token>`, account keys (`UUK_` and 48 hex characters) and any other run of
//! 64 or more word or `-` characters become `<redacted>`. Linear scans, no backtracking: paths and
//! bodies are untrusted.

fn is_jwt_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// JavaScript's `\w`, plus `-`.
fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

fn redact_jwts(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while from < bytes.len() {
        let Some(offset) = text[from..].find("eyJ") else {
            out.push_str(&text[from..]);
            break;
        };
        let start = from + offset;
        // "eyJ" + header, ".", payload, ".", signature: each part at least one character.
        let skip = |mut at: usize| {
            while at < bytes.len() && is_jwt_char(bytes[at]) {
                at += 1;
            }
            at
        };
        let dot = |at: usize| bytes.get(at) == Some(&b'.');
        let header_end = skip(start + 3);
        let payload_end = skip(header_end + 1);
        let signature_end = skip(payload_end + 1);
        let ok = header_end > start + 3
            && dot(header_end)
            && payload_end > header_end + 1
            && dot(payload_end)
            && signature_end > payload_end + 1;
        let at = signature_end;
        if ok {
            out.push_str(&text[from..start]);
            out.push_str("<token>");
            from = at;
        } else {
            out.push_str(&text[from..start + 3]);
            from = start + 3;
        }
    }
    out
}

fn redact_account_keys(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(offset) = text[from..].find("UUK_") {
        let start = from + offset;
        let hex_end = start
            + 4
            + bytes[start + 4..]
                .iter()
                .take(48)
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
        let followed_by_hex = bytes.get(hex_end).is_some_and(|b| b.is_ascii_hexdigit());
        if hex_end - start == 52 && !followed_by_hex {
            out.push_str(&text[from..start]);
            out.push_str("<redacted>");
            from = hex_end;
        } else {
            out.push_str(&text[from..start + 4]);
            from = start + 4;
        }
    }
    out.push_str(&text[from..]);
    out
}

fn redact_long_runs(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut copied = 0;
    while at < bytes.len() {
        if !is_token_char(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_token_char(bytes[at]) {
            at += 1;
        }
        if at - start >= 64 {
            out.push_str(&text[copied..start]);
            out.push_str("<redacted>");
            copied = at;
        }
    }
    out.push_str(&text[copied..]);
    out
}

pub fn redact(text: &str) -> String {
    redact_long_runs(&redact_account_keys(&redact_jwts(text)))
}

#[cfg(test)]
mod tests {
    use super::redact;

    // Expected values from the Node metagame's Redact.
    #[test]
    fn matches_node() {
        let key = format!("UUK_{}", "a".repeat(48));
        let long_key = format!("UUK_{}", "a".repeat(49));
        let cases = [
            ("eyJ.a.b", "eyJ.a.b"),
            ("eyJa..b", "eyJa..b"),
            ("xeyJa.b.c.d", "x<token>.d"),
            (long_key.as_str(), long_key.as_str()),
        ];
        for (input, expected) in cases {
            assert_eq!(redact(input), expected, "{input}");
        }
        assert_eq!(redact(&format!("eyJa.b.c{}", "z".repeat(70))), "<token>");
        assert_eq!(redact(&key), "<redacted>");
    }

    #[test]
    fn credentials_never_reach_the_log() {
        assert_eq!(
            redact("/account/api/oauth/sessions/kill/eyJhbGc.eyJ1c2Vy.c2ln"),
            "/account/api/oauth/sessions/kill/<token>"
        );
        assert_eq!(redact("eyJ.only"), "eyJ.only");
        assert_eq!(redact("eyJabc.def"), "eyJabc.def", "two parts are not a token");
        let key = format!("UUK_{}", "a".repeat(48));
        assert_eq!(redact(&format!("key={key}&x=1")), "key=<redacted>&x=1");
        assert_eq!(redact(&"x".repeat(64)), "<redacted>");
        assert_eq!(redact(&"x".repeat(63)), "x".repeat(63));
        assert_eq!(redact("/character/UID-1234"), "/character/UID-1234");
    }
}
