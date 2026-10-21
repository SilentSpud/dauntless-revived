//! The room nickname the 1.4.4 client joins with, and the rules a join must pass
//! (`realtime/chatnick.ts`; docs/findings/chat.md, "Names").
//!
//! The client joins every room as `UrlEncode(DisplayName):AccountId:OwnResource`. Its UrlEncode keeps
//! A-Z a-z 0-9 - _ . ~ and writes every other UTF-8 byte as %XX, so the name part never holds a ":".
//! Other clients take a nickname holding their own account id for their own presence, so the server
//! never rewrites a nickname: it admits one only when it is exactly `<name>:<own id>:<own resource>`,
//! carries no other account id, and `<name>` decodes to the account's username (now, or at chat
//! login) or to the client's fallback for a failed account read.

pub const FALLBACK_NAME: &str = "InvalidMCPUser";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NickReason {
    Account,
    Resource,
    Format,
    Name,
}

impl NickReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Account => "nick-account",
            Self::Resource => "nick-resource",
            Self::Format => "nick-format",
            Self::Name => "nick-name",
        }
    }
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~')
}

/// The client's UrlEncode (tests and docs): UTF-8, unreserved bytes kept, %XX upper case, NUL dropped.
pub fn url_encode_like_client(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b == 0 {
            continue;
        }
        if is_unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The name part decoded, or `None` when it is outside the encoder's alphabet or not valid UTF-8.
pub fn decode_name_part(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = part
                .get(i + 1..i + 3)
                .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else if is_unreserved(bytes[i]) {
            out.push(bytes[i]);
            i += 1;
        } else {
            return None;
        }
    }
    String::from_utf8(out).ok()
}

/// The name part of any nickname, decoded when possible (for logs).
pub fn name_part_of(nick: &str) -> Option<String> {
    decode_name_part(nick.split(':').next().unwrap_or(""))
}

/// `CheckNickname`: `Ok(name)` or the rule it breaks (with the decoded name for a wrong one).
pub fn check_nickname(nick: &str, uid: &str, resource: &str, names: &[Option<&str>]) -> Result<String, NickReason> {
    let Some((part1, rest)) = nick.split_once(':') else {
        return Err(NickReason::Account);
    };
    if rest != format!("{uid}:{resource}") {
        return Err(if rest.split(':').next() != Some(uid) {
            NickReason::Account
        } else {
            NickReason::Resource
        });
    }
    // No other account id anywhere (the resource is the client's own free text)
    if nick.split(uid).collect::<String>().contains("UID-") {
        return Err(NickReason::Account);
    }
    let name = decode_name_part(part1).ok_or(NickReason::Format)?;
    if name != FALLBACK_NAME && !names.iter().flatten().any(|known| *known == name) {
        return Err(NickReason::Name);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nicknames_are_checked_like_node() {
        let resource = "V2:app:WIN::0123";
        let nick = |name: &str| format!("{}:UID-a:{resource}", url_encode_like_client(name));
        assert_eq!(
            check_nickname(&nick("Ada Lovelace"), "UID-a", resource, &[Some("Ada Lovelace")]),
            Ok("Ada Lovelace".into())
        );
        assert_eq!(
            check_nickname(&nick("InvalidMCPUser"), "UID-a", resource, &[None]),
            Ok(FALLBACK_NAME.into())
        );
        assert_eq!(
            check_nickname(&nick("Bo"), "UID-a", resource, &[Some("Ada")]),
            Err(NickReason::Name)
        );
        assert_eq!(
            check_nickname("Ada", "UID-a", resource, &[Some("Ada")]),
            Err(NickReason::Account)
        );
        assert_eq!(
            check_nickname("Ada:UID-b:x", "UID-a", resource, &[Some("Ada")]),
            Err(NickReason::Account)
        );
        assert_eq!(
            check_nickname("Ada:UID-a:other", "UID-a", resource, &[Some("Ada")]),
            Err(NickReason::Resource)
        );
        assert_eq!(
            check_nickname("A%2:UID-a:V2:app:WIN::0123", "UID-a", resource, &[Some("A")]),
            Err(NickReason::Format)
        );
        assert_eq!(
            check_nickname("%FF:UID-a:V2:app:WIN::0123", "UID-a", resource, &[Some("A")]),
            Err(NickReason::Format)
        );
        assert_eq!(
            check_nickname("UID-b:UID-a:V2:app:WIN::0123", "UID-a", resource, &[Some("UID-b")]),
            Err(NickReason::Account)
        );
        assert_eq!(decode_name_part("%c3%A9"), Some("\u{e9}".into()));
        assert_eq!(url_encode_like_client("\u{e9} a"), "%C3%A9%20a");
    }
}
