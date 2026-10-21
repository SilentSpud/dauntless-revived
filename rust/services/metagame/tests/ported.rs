#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The ledger of M31's port (`tests/ported.json`): every `node:test` test of the Node metagame is
//! listed with the Rust test that ports it, or why it does not apply, or "todo". This checks that no
//! TS test is missing from the list or listed twice, and that every Rust test it names exists.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// The names of `test('…')` / `it("…")` calls in a TS test file, in order.
fn test_names(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &source[at..];
        let (skip, found) = match (rest.find("test("), rest.find("it(")) {
            (Some(t), Some(i)) if i < t => (i, 3),
            (Some(t), _) => (t, 5),
            (None, Some(i)) => (i, 3),
            (None, None) => break,
        };
        let start = at + skip;
        at = start + found;
        let before = start.checked_sub(1).map(|i| bytes[i]);
        if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.') {
            continue;
        }
        let mut i = at;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let Some(&quote) = bytes.get(i) else { break };
        if !matches!(quote, b'\'' | b'"' | b'`') {
            continue;
        }
        let mut end = i + 1;
        while end < bytes.len() && bytes[end] != quote {
            if bytes[end] == b'\\' {
                end += 1;
            }
            end += 1;
        }
        names.push(source[i + 1..end.min(bytes.len())].to_owned());
        at = end + 1;
    }
    names
}

#[test]
fn every_node_test_is_listed_once_and_every_port_exists() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ts_dir = crate_dir.join("../../../legacy/UndauntedMetagame/test");
    let ledger: Value =
        serde_json::from_str(&std::fs::read_to_string(crate_dir.join("tests/ported.json")).unwrap()).unwrap();
    let files = ledger["files"].as_object().unwrap();

    let mut problems = Vec::new();
    let mut tally = BTreeMap::<&str, usize>::new();
    let mut on_disk = Vec::new();
    for entry in std::fs::read_dir(&ts_dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if name.ends_with(".test.ts") {
            on_disk.push(name);
        }
    }
    on_disk.sort();
    for file in &on_disk {
        let names = test_names(&std::fs::read_to_string(ts_dir.join(file)).unwrap());
        let Some(listed) = files.get(file).and_then(Value::as_object) else {
            problems.push(format!("{file} is not in the ledger"));
            continue;
        };
        for name in &names {
            if !listed.contains_key(name) {
                problems.push(format!("{file}: \"{name}\" is not in the ledger"));
            }
        }
        for (name, port) in listed {
            if !names.contains(name) {
                problems.push(format!("{file}: \"{name}\" is in the ledger but not in the file"));
            }
            let port = port.as_str().unwrap_or("");
            if port == "todo" {
                *tally.entry("todo").or_default() += 1;
            } else if port.starts_with("n/a: ") {
                *tally.entry("not applicable").or_default() += 1;
            } else if let Some((path, function)) = port.split_once("::") {
                let source = std::fs::read_to_string(crate_dir.join(path)).unwrap_or_default();
                if !source.contains(&format!("fn {function}(")) {
                    problems.push(format!("{file}: \"{name}\" names {port}, which does not exist"));
                }
                *tally.entry("ported").or_default() += 1;
            } else {
                problems.push(format!("{file}: \"{name}\" has an unreadable entry {port:?}"));
            }
        }
    }
    for file in files.keys() {
        if !on_disk.contains(file) {
            problems.push(format!(
                "{file} is in the ledger but not in legacy/UndauntedMetagame/test"
            ));
        }
    }
    println!("M31 ledger: {tally:?}");
    // M31_LEDGER_REFRESH=1 rewrites the ledger (names sorted): new tests as "todo", gone ones
    // dropped, every other entry kept
    if std::env::var("M31_LEDGER_REFRESH").as_deref() == Ok("1") {
        let mut refreshed = serde_json::Map::new();
        for file in &on_disk {
            let names = test_names(&std::fs::read_to_string(ts_dir.join(file)).unwrap());
            let mut entries = serde_json::Map::new();
            for name in names {
                let port = files
                    .get(file)
                    .and_then(|f| f.get(&name))
                    .cloned()
                    .unwrap_or_else(|| "todo".into());
                entries.insert(name, port);
            }
            refreshed.insert(file.clone(), Value::Object(entries));
        }
        let mut ledger = ledger.clone();
        ledger["files"] = Value::Object(refreshed);
        let text = serde_json::to_string_pretty(&ledger).unwrap();
        std::fs::write(
            crate_dir.join("tests/ported.json"),
            text + "
",
        )
        .unwrap();
        return;
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn test_names_are_read_like_node_test_sees_them() {
    let source = "test('a', () => {}); it(\"b \\\" c\", x); xtest('no'); foo.test('no'); test(`t ${n}`)";
    assert_eq!(test_names(source), ["a", "b \\\" c", "t ${n}"]);
}
