//! The gateway's request policy and address parsing (MIGRATION_PLAN.md S2, S3, X9): the first thing a public
//! request meets. A method, target and headers from the network must give a decision, never a panic, and a
//! proxied target must be a canonical path the policy allows. The allowlist helper's address check and the
//! metagame's client-address reading take the same untrusted text.
#![no_main]

use arbitrary::Arbitrary;
use http::{HeaderMap, HeaderName, HeaderValue};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Request<'a> {
    method: &'a str,
    target: &'a str,
    headers: Vec<(&'a [u8], &'a [u8])>,
    upgrade: bool,
    allow_private: bool,
}

fuzz_target!(|request: Request<'_>| {
    let mut headers = HeaderMap::new();
    for (name, value) in request.headers.iter().take(32) {
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name), HeaderValue::from_bytes(value)) {
            headers.append(name, value);
        }
    }
    let _ = gateway::policy::classify(request.method, request.target, &headers, request.upgrade);
    let _ = gateway::policy::has_bearer(&headers);
    if let Some((path, _query)) = gateway::policy::split_target(request.target) {
        let _ = gateway::policy::check_path(path);
        if let Some(canonical) = gateway::policy::canonical_path(path) {
            assert!(
                canonical.starts_with('/'),
                "a canonical path is absolute: {canonical:?}"
            );
        }
    }
    let _ = gateway::ip::parse_ip(request.target);
    let _ = gateway::ip::check_allowlist_ip(Some(request.target), request.allow_private);
    let _ = metagame::origin::normalize_text(request.target);
    let _ = metagame::origin::is_proxied(&headers);
});
