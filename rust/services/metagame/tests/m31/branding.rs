//! `branding.test.ts`: no text the metagame sends or logs calls the product Undaunted. Every string
//! literal in the Rust metagame's `src/` is checked; what stays are wire identifiers (the
//! /undaunted/api routes, the x-undaunted-* headers, `undaunted-metagame`, the `undaunted.db` file
//! name) and the paths `include_str!` reads the Node project's files from.

use std::path::{Path, PathBuf};

fn names_product(text: &str) -> bool {
    let wire = [
        regex::Regex::new(r"(?i)/undaunted/api\b").unwrap(),
        regex::Regex::new(r"\bx-undaunted-(user-api-key|gameserver-apikey)\b").unwrap(),
        regex::Regex::new(r"^undaunted-metagame$").unwrap(),
        regex::Regex::new(r"^undaunted\.db$").unwrap(),
        regex::Regex::new(r"legacy/Undaunted[A-Za-z]+/").unwrap(),
    ];
    let rest = wire.iter().fold(text.to_owned(), |rest, pattern| {
        pattern.replace_all(&rest, "").into_owned()
    });
    rest.to_lowercase().contains("undaunted")
}

fn source_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(source_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}

/// Every string literal outside comments, with its line: "…" with escapes, r"…" and r#"…"#.
fn strings(source: &str) -> Vec<(usize, String)> {
    let bytes = source.as_bytes();
    let (mut found, mut i, mut line) = (Vec::new(), 0, 1);
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => line += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = source[i..].find("*/").map_or(bytes.len(), |e| i + e + 2);
                line += source[i..end].matches('\n').count();
                i = end;
                continue;
            }
            // A character literal like '"' is not a string's start
            b'\'' if bytes.get(i + 2) == Some(&b'\'') => {
                i += 3;
                continue;
            }
            b'\'' if bytes.get(i + 1) == Some(&b'\\') && bytes.get(i + 3) == Some(&b'\'') => {
                i += 4;
                continue;
            }
            b'r' if (bytes.get(i + 1) == Some(&b'"') || bytes.get(i + 1) == Some(&b'#'))
                && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) =>
            {
                let hashes = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let start = i + 2 + hashes;
                    let close = format!("\"{}", "#".repeat(hashes));
                    let end = source[start..].find(&close).map_or(bytes.len(), |e| start + e);
                    found.push((line, source[start..end].to_owned()));
                    line += source[start..end].matches('\n').count();
                    i = end + close.len();
                    continue;
                }
            }
            b'"' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'"' {
                    if bytes[end] == b'\\' {
                        end += 1;
                    }
                    end += 1;
                }
                let text = &source[start..end.min(bytes.len())];
                found.push((line, text.to_owned()));
                line += text.matches('\n').count();
                i = end + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    found
}

#[test]
fn no_string_in_src_calls_the_product_undaunted_apart_from_wire_identifiers() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = source_files(&src);
    assert!(
        files.len() > 20,
        "expected the metagame's sources under {}",
        src.display()
    );
    let mut offenders = Vec::new();
    for file in &files {
        for (line, text) in strings(&std::fs::read_to_string(file).unwrap()) {
            if names_product(&text) {
                offenders.push(format!(
                    "{}:{line}: {text:?}",
                    file.strip_prefix(&src).unwrap().display()
                ));
            }
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

#[test]
fn would_notice_the_product_name_coming_back() {
    let probe = "use x; // \"Undaunted\" in a comment\nconst A: &str = \"x-undaunted-user-api-key\";\nfn b() -> String { format!(\"Welcome to Undaunted {}!\", r#\"raw\"#) }\n";
    let texts: Vec<String> = strings(probe).into_iter().map(|(_, t)| t).collect();
    assert_eq!(texts, ["x-undaunted-user-api-key", "Welcome to Undaunted {}!", "raw"]);
    for text in [
        "Welcome to Undaunted v0.0.5!",
        "Undaunted Metagame on ",
        "UNDAUNTED",
        "/undaunted/api/x by Undaunted",
    ] {
        assert!(names_product(text), "{text}");
    }
    for text in [
        "/undaunted/api/ServerStatus",
        "/UNDAUNTED/API/Register",
        "x-undaunted-user-api-key",
        "x-undaunted-gameserver-apikey",
        "undaunted-metagame",
        "Welcome to Dauntless Revived!",
    ] {
        assert!(!names_product(text), "{text}");
    }
}
