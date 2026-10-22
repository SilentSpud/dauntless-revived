//! The content server's range handling and manifest checks (MIGRATION_PLAN.md S1, X9): a `Range` header
//! from the network must give a range inside the file or a refusal, and no manifest text makes the server
//! accept a path that leaves the game folder.
#![no_main]

use arbitrary::Arbitrary;
use content::manifest::{is_safe_manifest_path, parse};
use content::range::{Range, if_none_match_hits, if_range_allows_partial, parse_range};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Input<'a> {
    range: &'a str,
    size: u64,
    etag: &'a str,
    manifest: &'a str,
}

fuzz_target!(|input: Input<'_>| {
    if let Range::Partial { start, end } = parse_range(Some(input.range), input.size) {
        assert!(start <= end && end < input.size, "{start}-{end} of {}", input.size);
    }
    let _ = if_range_allows_partial(Some(input.range), input.etag);
    let _ = if_none_match_hits(Some(input.range), input.etag);
    if let Ok(manifest) = parse(input.manifest) {
        for file in &manifest.files {
            assert!(
                is_safe_manifest_path(&file.path),
                "an accepted manifest holds {:?}",
                file.path
            );
            assert!(!file.path.split('/').any(|part| part == ".."), "{:?}", file.path);
        }
    }
});
