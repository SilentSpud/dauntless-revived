//! What `/key link` accepts (`link-input.mjs`): the launcher's account-key alphabet, also inside a
//! code block, quotes, or the `Key:` line of the launcher's backup file. A Join invite is refused
//! with its own message: it cannot identify an existing account.

use service_kit::js::{is_space, trim};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkInput {
    Key(String),
    /// `invalid_key_format` or `invite_not_key`
    Refused(&'static str),
}

fn is_key(text: &str) -> bool {
    (8..=128).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn link_input(input: &str) -> LinkInput {
    if input.encode_utf16().count() > 8192 {
        return LinkInput::Refused("invalid_key_format");
    }
    let mut text = trim(input).to_owned();
    if text.to_ascii_lowercase().contains("dauntless-revived://join") {
        return LinkInput::Refused("invite_not_key");
    }
    if text.starts_with("```") && text.ends_with("```") && text.len() >= 6 {
        text = trim(&text[3..text.len() - 3]).to_owned();
    } else if text.len() >= 2
        && ((text.starts_with('`') && text.ends_with('`')) || (text.starts_with('"') && text.ends_with('"')))
    {
        text = trim(&text[1..text.len() - 1]).to_owned();
    }
    let labelled: Vec<String> = text
        .split('\n')
        .filter_map(|line| {
            let line = line.trim_matches(is_space);
            let value = trim(line.strip_prefix("Key:")?);
            is_key(value).then(|| value.to_owned())
        })
        .collect();
    if labelled.len() > 1 {
        return LinkInput::Refused("invalid_key_format");
    }
    if let Some(key) = labelled.into_iter().next() {
        text = key;
    }
    if !is_key(&text) {
        return LinkInput::Refused("invalid_key_format");
    }
    LinkInput::Key(text)
}

pub const KEY_INSTRUCTIONS: &str = "In the launcher, open **Settings → Save a backup of your key…**. Open the saved text file and copy only the value after **Key:** into `/key link key:`. This uses your existing account; your key and progress stay unchanged. Do not post the key in chat.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_in_every_private_format() {
        let key = "oldkey12";
        for input in [
            key.to_owned(),
            format!(" `{key}` "),
            format!("```\n{key}\n```"),
            format!("Dauntless Revived account key\nUsername: Slayer\nKey: {key}\n\nKeep this file private."),
            format!("\"{key}\""),
        ] {
            assert_eq!(link_input(&input), LinkInput::Key(key.into()), "{input:?}");
        }
    }

    #[test]
    fn invites_ambiguous_backups_and_malformed_input() {
        assert_eq!(
            link_input("dauntless-revived://join?v=2&code=OLD-CODE"),
            LinkInput::Refused("invite_not_key")
        );
        for input in [
            "Key: firstkey\nKey: secondkey".to_owned(),
            "bad key value".into(),
            "x".repeat(8193),
            "short".into(),
        ] {
            assert_eq!(
                link_input(&input),
                LinkInput::Refused("invalid_key_format"),
                "{input:?}"
            );
        }
    }
}
