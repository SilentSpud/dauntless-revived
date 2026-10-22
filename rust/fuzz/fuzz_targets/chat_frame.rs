//! The chat frame parser and the helpers that read client text (MIGRATION_PLAN.md M33): whatever a
//! client sends must neither panic nor take time beyond linear, and a frame that parses must give
//! back only what was in it.
#![no_main]

use libfuzzer_sys::fuzz_target;
use metagame::chat::nick::{check_nickname, decode_name_part, name_part_of};
use metagame::chat::xmpp::{
    Element, Node, escape, has_markup_declaration, log_text, parse_frame, parse_jid, redact_frame,
};

/// Every name, attribute and text in the tree came from the input: none is longer than it (decoding
/// an entity only shortens), and the tree is no deeper than the input is long. Walked without
/// recursion, as deep as the input nests.
fn check(root: &Element, input_len: usize) {
    let mut pending = vec![(root, 0usize)];
    while let Some((element, depth)) = pending.pop() {
        assert!(element.name.len() <= input_len);
        assert!(depth <= input_len);
        for (name, value) in &element.attrs {
            assert!(name.len() <= input_len && value.len() <= input_len);
        }
        for child in &element.children {
            match child {
                Node::Element(inner) => pending.push((inner, depth + 1)),
                Node::Text(text) => assert!(text.len() <= input_len),
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // The chat reads every frame as UTF-8, lossily (text and binary frames alike)
    let raw = String::from_utf8_lossy(data);
    if !has_markup_declaration(&raw)
        && let Some(element) = parse_frame(&raw)
    {
        check(&element, raw.len());
        let _ = element.text();
        let _ = element.child("body");
    }
    let _ = redact_frame(&raw);
    let _ = log_text(&raw, 80);
    let _ = escape(&raw);
    if let Some(jid) = parse_jid(&raw) {
        let nick = jid.resource.unwrap_or_default();
        let _ = check_nickname(&nick, &jid.local, &jid.domain, &[Some("Ada")]);
        let _ = name_part_of(&nick);
    }
    let _ = decode_name_part(&raw);
});
