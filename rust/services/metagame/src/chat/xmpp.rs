//! Small XMPP helpers for the chat listener and its rooms (`realtime/xmpp.ts`), and the frame parser:
//! a port of the parser the Node chat used (ltx 3.1.2, its own small SAX parser and tree builder), so
//! a frame parses here exactly as it did there. It is lenient where ltx is (a mismatched end tag is
//! ignored, text outside the root is dropped, an attribute without a value takes the next one's) and
//! strict where it is (an unknown entity or an illegal character reference fails the whole frame).
//! DTDs, comments, CDATA and processing instructions never reach it (`has_markup_declaration`).
//! Linear in the frame's length, no recursion on the input's depth.

pub const NS_FRAMING: &str = "urn:ietf:params:xml:ns:xmpp-framing";
pub const NS_SASL: &str = "urn:ietf:params:xml:ns:xmpp-sasl";
pub const NS_BIND: &str = "urn:ietf:params:xml:ns:xmpp-bind";
pub const NS_SESSION: &str = "urn:ietf:params:xml:ns:xmpp-session";
pub const NS_STREAMS: &str = "http://etherx.jabber.org/streams";
pub const NS_STREAM_ERRORS: &str = "urn:ietf:params:xml:ns:xmpp-streams";
pub const NS_STANZAS: &str = "urn:ietf:params:xml:ns:xmpp-stanzas";
pub const NS_CLIENT: &str = "jabber:client";
pub const NS_PING: &str = "urn:xmpp:ping";
pub const NS_MUC: &str = "http://jabber.org/protocol/muc";
pub const NS_MUC_USER: &str = "http://jabber.org/protocol/muc#user";

/// The client's cooked `[OnlineSubsystemMcp.XMPP] Domain`, used when its `<open to>` is missing or
/// is not a host name.
pub const DEFAULT_DOMAIN: &str = "prod.ol.epicgames.com";

// ---- The tree ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    pub name: String,
    /// In the order first seen; a repeated name keeps its first place and takes the last value.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// `getText()`: the element's own text children, concatenated.
    pub fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|c| match c {
                Node::Text(t) => Some(t.as_str()),
                Node::Element(_) => None,
            })
            .collect()
    }

    /// `ChildNamed`: the first child element whose local name is `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| local_name(&e.name) == name)
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }
}

/// `TextOf`: an element's text, or "" for none.
pub fn text_of(element: Option<&Element>) -> String {
    element.map(Element::text).unwrap_or_default()
}

// ---- Entities ----

/// JavaScript's `parseInt(text, radix)` for 10 and 16: leading white space, a sign, (for 16) an
/// optional `0x`, then as many digits as there are; `None` for NaN.
fn parse_int(text: &str, radix: u32) -> Option<i64> {
    let text = crate::js::trim(text);
    let (negative, mut rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if radix == 16 && (rest.starts_with("0x") || rest.starts_with("0X")) {
        rest = &rest[2..];
    }
    let digits: String = rest.chars().take_while(|c| c.is_digit(radix)).collect();
    if digits.is_empty() {
        return None;
    }
    // Longer runs are far outside every legal character anyway
    let value = i64::from_str_radix(&digits[..digits.len().min(15)], radix).ok()?;
    Some(if negative { -value } else { value })
}

fn is_xml_char(code: i64) -> bool {
    matches!(code, 0x9 | 0xa | 0xd | 0x20..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff)
}

/// `unescapeXML`: each `&...;` must be one of the five entities or a legal character reference.
fn unescape(text: &str) -> Result<String, ()> {
    let mut out = String::new();
    let mut previous = 0;
    while let Some(start) = text[previous..].find('&').map(|i| i + previous) {
        let Some(end) = text[start + 1..].find(';').map(|i| i + start + 1) else {
            break;
        };
        out.push_str(&text[previous..start]);
        let entity = &text[start..=end];
        let decoded = match entity {
            "&amp;" => '&',
            "&lt;" => '<',
            "&gt;" => '>',
            "&quot;" => '"',
            "&apos;" => '\'',
            _ if entity.as_bytes().get(1) == Some(&b'#') => {
                let body = &entity[2..entity.len() - 1];
                let code = match body.strip_prefix('x') {
                    Some(hex) => parse_int(hex, 16),
                    None => parse_int(body, 10),
                };
                match code.filter(|c| is_xml_char(*c)).and_then(|c| char::from_u32(c as u32)) {
                    Some(c) => c,
                    None => return Err(()),
                }
            }
            _ => return Err(()),
        };
        out.push(decoded);
        previous = end + 1;
    }
    if previous == 0 {
        return Ok(text.to_owned());
    }
    out.push_str(&text[previous..]);
    Ok(out)
}

// ---- The parser ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    TagName,
    Tag,
    AttrName,
    AttrEq,
    AttrQuot,
    AttrValue,
}

/// The tree being built: open elements as a stack, so a closed child is appended to its parent.
#[derive(Default)]
struct Builder {
    open: Vec<Element>,
    tree: Option<Element>,
}

impl Builder {
    fn start(&mut self, name: String, attrs: Vec<(String, String)>) {
        self.open.push(Element {
            name,
            attrs,
            children: Vec::new(),
        });
    }

    fn end(&mut self, name: &str) {
        if self.open.last().is_none_or(|e| e.name != name) {
            return;
        }
        let Some(done) = self.open.pop() else {
            return;
        };
        match self.open.last_mut() {
            Some(parent) => parent.children.push(Node::Element(done)),
            None => {
                if self.tree.is_none() {
                    self.tree = Some(done);
                }
            }
        }
    }

    fn text(&mut self, text: String) {
        if let Some(current) = self.open.last_mut() {
            current.children.push(Node::Text(text));
        }
    }
}

/// `ParseFrame` (ltx's `parse`): the first complete root element, or `None` for anything that does
/// not parse. Callers refuse markup declarations first.
pub fn parse_frame(raw: &str) -> Option<Element> {
    let data = raw.as_bytes();
    let mut builder = Builder::default();
    let mut state = State::Text;
    let mut record: Option<usize> = Some(0);
    let mut tag_name = String::new();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut end_tag = false;
    let mut self_closing = false;
    let mut quote = b'"';
    let mut attr_name = String::new();
    let mut pos = 0;
    let recorded = |record: &mut Option<usize>, pos: usize| -> String {
        // Every boundary is an ASCII byte, so these slices are on character boundaries
        let text = record
            .map(|start| raw.get(start..pos).unwrap_or("").to_owned())
            .unwrap_or_default();
        *record = None;
        text
    };
    while pos < data.len() {
        match state {
            State::Text => match raw.get(pos..).and_then(|rest| rest.find('<')) {
                Some(lt) => pos += lt,
                // Text after the last tag is never emitted
                None => break,
            },
            State::AttrValue => {
                if let Some(at) = data[pos..].iter().position(|b| *b == quote) {
                    pos += at;
                }
            }
            _ => {}
        }
        let Some(&c) = data.get(pos) else {
            break;
        };
        match state {
            State::Text => {
                if c == b'<' {
                    let text = recorded(&mut record, pos);
                    if !text.is_empty() {
                        builder.text(unescape(&text).ok()?);
                    }
                    state = State::TagName;
                    record = Some(pos + 1);
                    attrs = Vec::new();
                }
            }
            State::TagName => {
                if c == b'/' && record == Some(pos) {
                    record = Some(pos + 1);
                    end_tag = true;
                } else if c == b'!' || c == b'?' {
                    // Refused before parsing (has_markup_declaration); never a stanza
                    return None;
                } else if c <= 32 || c == b'/' || c == b'>' {
                    tag_name = recorded(&mut record, pos);
                    state = State::Tag;
                    continue;
                }
            }
            State::Tag => {
                if c == b'>' {
                    let name = std::mem::take(&mut tag_name);
                    if end_tag {
                        builder.end(&name);
                    } else {
                        builder.start(name.clone(), std::mem::take(&mut attrs));
                        if self_closing {
                            builder.end(&name);
                        }
                    }
                    end_tag = false;
                    self_closing = false;
                    state = State::Text;
                    record = Some(pos + 1);
                } else if c == b'/' {
                    self_closing = true;
                } else if c > 32 {
                    record = Some(pos);
                    state = State::AttrName;
                }
            }
            State::AttrName => {
                if c <= 32 || c == b'=' {
                    attr_name = recorded(&mut record, pos);
                    state = State::AttrEq;
                    continue;
                }
            }
            State::AttrEq => {
                if c == b'=' {
                    state = State::AttrQuot;
                }
            }
            State::AttrQuot => {
                if c == b'"' || c == b'\'' {
                    quote = c;
                    state = State::AttrValue;
                    record = Some(pos + 1);
                }
            }
            State::AttrValue => {
                if c == quote {
                    let value = unescape(&recorded(&mut record, pos)).ok()?;
                    match attrs.iter_mut().find(|(n, _)| *n == attr_name) {
                        Some(slot) => slot.1 = value,
                        None => attrs.push((std::mem::take(&mut attr_name), value)),
                    }
                    state = State::Tag;
                }
            }
        }
        pos += 1;
    }
    builder.tree
}

/// `HasMarkupDeclaration`: a DTD, comment, CDATA section or processing instruction.
pub fn has_markup_declaration(raw: &str) -> bool {
    raw.contains("<!") || raw.contains("<?")
}

// ---- Text ----

/// `EscapeXml`.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// `LocalName`: "stream:features" -> "features", lower case.
pub fn local_name(name: &str) -> String {
    name.rsplit(':').next().unwrap_or("").to_lowercase()
}

/// `LogText`: client text for a log line, at most `max` characters, anything but printable ASCII as `?`.
pub fn log_text(text: &str, max: usize) -> String {
    let count = text.chars().count();
    let shown: String = text
        .chars()
        .take(max)
        .map(|c| if ('\x21'..='\x7e').contains(&c) { c } else { '?' })
        .collect();
    if count > max { format!("{shown}...") } else { shown }
}

/// `IsHostName`: 1 to 253 characters of A-Z a-z 0-9 . -
pub fn is_host_name(value: &str) -> bool {
    (1..=253).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// `IsXmppDomain`: a host name with an optional port (the launcher's local MCP endpoint keeps it).
pub fn is_xmpp_domain(value: &str) -> bool {
    if value.is_empty() || value.len() > 259 {
        return false;
    }
    let (host, port) = match value.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (value, None),
    };
    if !is_host_name(host) {
        return false;
    }
    port.is_none_or(|p| {
        (1..=5).contains(&p.len())
            && p.bytes().all(|b| b.is_ascii_digit())
            && p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jid {
    pub local: String,
    pub domain: String,
    pub resource: Option<String>,
}

/// `ParseJid`: local@domain/resource; the resource is everything after the first "/".
pub fn parse_jid(text: &str) -> Option<Jid> {
    let (bare, resource) = match text.split_once('/') {
        Some((bare, resource)) => (bare, Some(resource.to_owned())),
        None => (text, None),
    };
    let at = bare.find('@')?;
    if at == 0 || at == bare.len() - 1 {
        return None;
    }
    Some(Jid {
        local: bare[..at].to_owned(),
        domain: bare[at + 1..].to_owned(),
        resource,
    })
}

/// `CONTROL_CHARACTER`: `\p{Cc}`.
pub fn has_control_character(text: &str) -> bool {
    text.chars().any(char::is_control)
}

// ---- Rate limits ----

/// A token bucket: `burst` tokens, one more every `refill_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    tokens: f64,
    at: i64,
}

impl Bucket {
    pub fn new(burst: u32, now: i64) -> Self {
        Self {
            tokens: f64::from(burst),
            at: now,
        }
    }

    /// `TakeToken`.
    pub fn take(&mut self, burst: u32, refill_ms: f64, now: i64) -> bool {
        let elapsed = (now - self.at).max(0) as f64;
        self.tokens = f64::from(burst).min(self.tokens + elapsed / refill_ms);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ---- The trace ----

/// Replaces the content between `<open...>` and `</close>` (case-insensitive tag names) with `with`.
fn redact_between(text: &str, tag: &str, with: impl Fn(&str) -> String) -> String {
    let lower = text.to_ascii_lowercase();
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut out = String::new();
    let mut from = 0;
    while let Some(start) = lower[from..].find(&open).map(|i| i + from) {
        // A word boundary after the name, as `\b` asks
        let after = start + open.len();
        if lower
            .as_bytes()
            .get(after)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            out.push_str(&text[from..after]);
            from = after;
            continue;
        }
        let Some(head_end) = lower[after..].find('>').map(|i| i + after + 1) else {
            break;
        };
        let Some(close_at) = lower[head_end..].find(&close).map(|i| i + head_end) else {
            break;
        };
        out.push_str(&text[from..head_end]);
        out.push_str(&with(&text[head_end..close_at]));
        out.push_str(&text[close_at..close_at + close.len()]);
        from = close_at + close.len();
    }
    out.push_str(&text[from..]);
    out
}

/// `RedactFrame` (`CHAT_TRACE=1`): no SASL payload, legacy password, message text or token reaches
/// the log. Capped at 2 KB.
pub fn redact_frame(raw: &str) -> String {
    let text = redact_between(raw, "auth", |_| "[redacted]".into());
    let text = redact_between(&text, "password", |_| "[redacted]".into());
    let text = redact_between(&text, "body", |inner| format!("[{} chars]", inner.chars().count()));
    let text = crate::redact::redact(&text);
    if text.encode_utf16().count() > 2048 {
        let cut: String = text
            .chars()
            .scan(0, |n, c| {
                *n += c.len_utf16();
                (*n <= 2048).then_some(c)
            })
            .collect();
        format!("{cut}...")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_parse_as_ltx_parses_them() {
        let open =
            parse_frame(r#"<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" to="127.0.0.1:61000" version="1.0"/>"#)
                .unwrap();
        assert_eq!(open.name, "open");
        assert_eq!(open.attr("to"), Some("127.0.0.1:61000"));

        let message =
            parse_frame("<message to='a@b' type=\"chat\"><body>hi &amp; &#65;&#x42; there</body>tail</message>")
                .unwrap();
        assert_eq!(message.child("body").unwrap().text(), "hi & AB there");
        assert_eq!(message.text(), "tail");

        // Mismatched end tags are ignored; the root closes at its own name
        let loose = parse_frame("<a><b></c></b></a>").unwrap();
        assert_eq!(loose.child("b").unwrap().name, "b");

        // An attribute without a value takes the next one's
        let odd = parse_frame(r#"<a b c="1"/>"#).unwrap();
        assert_eq!(odd.attrs, vec![("b".into(), "1".into())]);

        // An unknown entity or an illegal character fails the frame; no root fails it too
        assert_eq!(parse_frame("<a>&nbsp;</a>"), None);
        assert_eq!(parse_frame("<a>&#0;</a>"), None);
        assert_eq!(parse_frame("<a b='&foo;'/>"), None);
        assert_eq!(parse_frame("<a><b/>"), None);
        assert_eq!(parse_frame("text only"), None);
        // An & without a ; stays
        assert_eq!(parse_frame("<a>x & y</a>").unwrap().text(), "x & y");
        // Stream prefixes stay in the name
        assert_eq!(local_name(&parse_frame("<stream:features/>").unwrap().name), "features");
        // The first root wins
        assert_eq!(parse_frame("<a/><b/>").unwrap().name, "a");
    }

    #[test]
    fn helpers_match_node() {
        assert!(is_xmpp_domain("127.0.0.1:61000"));
        assert!(is_xmpp_domain("prod.ol.epicgames.com"));
        assert!(!is_xmpp_domain("host:0"));
        assert!(!is_xmpp_domain("host:123456"));
        assert!(!is_xmpp_domain("bad host"));
        assert_eq!(
            parse_jid("City-1@muc.dom/Name:UID-a:V2:x/y"),
            Some(Jid {
                local: "City-1".into(),
                domain: "muc.dom".into(),
                resource: Some("Name:UID-a:V2:x/y".into())
            })
        );
        assert_eq!(parse_jid("@dom"), None);
        assert_eq!(parse_jid("a@"), None);
        assert_eq!(log_text("ab c\u{e9}", 3), "ab?...");
        assert_eq!(
            escape("<a href='x'>&\"</a>"),
            "&lt;a href=&apos;x&apos;&gt;&amp;&quot;&lt;/a&gt;"
        );
        let mut bucket = Bucket::new(2, 0);
        assert!(bucket.take(2, 1000.0, 0));
        assert!(bucket.take(2, 1000.0, 0));
        assert!(!bucket.take(2, 1000.0, 0));
        assert!(bucket.take(2, 1000.0, 1000));
    }

    #[test]
    fn the_trace_is_redacted() {
        assert_eq!(
            redact_frame("<auth mechanism='PLAIN'>c2VjcmV0</auth>"),
            "<auth mechanism='PLAIN'>[redacted]</auth>"
        );
        assert_eq!(
            redact_frame("<message><body>h\u{e9}llo</body></message>"),
            "<message><body>[5 chars]</body></message>"
        );
        assert_eq!(redact_frame("<authx>keep</authx>"), "<authx>keep</authx>");
    }
}
