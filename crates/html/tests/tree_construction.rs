//! Data-driven tree construction tests.
//!
//! Reads every `tests/fixtures/*.dat` file in the html5lib-tests format:
//!
//! ```text
//! #data
//! <p>Hello
//! #errors
//! #document
//! | <html>
//! |   <head>
//! |   <body>
//! |     <p>
//! |       "Hello"
//!
//! ```
//!
//! Each case's input is parsed and `Document::dump()` is compared against
//! the `#document` section. The `#errors` section is ignored: error counts
//! are informational and we do not try to match them exactly.

use std::fs;
use std::path::Path;

struct Case {
    file: String,
    line: usize,
    data: String,
    expected: String,
}

fn parse_dat(file: &str, src: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut lines = src.lines().enumerate().peekable();

    while let Some((i, line)) = lines.next() {
        if line != "#data" {
            continue;
        }
        let start_line = i + 1;
        let mut section = "data";
        let mut data: Vec<&str> = Vec::new();
        let mut document: Vec<&str> = Vec::new();
        let mut skip = false;

        while let Some(&(_, l)) = lines.peek() {
            if l == "#data" {
                break;
            }
            if let Some(name) = l.strip_prefix('#') {
                section = match name {
                    "errors" | "new-errors" | "document" | "script-on" | "script-off" => name,
                    "document-fragment" => {
                        skip = true;
                        name
                    }
                    _ => section,
                };
                lines.next();
                continue;
            }
            match section {
                "data" => data.push(l),
                "document" => {
                    if l.is_empty() {
                        lines.next();
                        break;
                    }
                    document.push(l);
                }
                _ => {}
            }
            lines.next();
        }

        if skip {
            continue;
        }
        let mut expected = document.join("\n");
        expected.push('\n');
        cases.push(Case {
            file: file.to_owned(),
            line: start_line,
            data: data.join("\n"),
            expected,
        });
    }
    cases
}

#[test]
fn tree_construction_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("fixtures directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "dat"))
        .collect();
    paths.sort();

    let mut total = 0;
    let mut failures = Vec::new();

    for path in paths {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        for case in parse_dat(&file, &src) {
            total += 1;
            let doc = luma_html::parse(&case.data);
            let dump = doc.dump();
            let actual = dump.strip_prefix("#document\n").unwrap_or(&dump);
            if actual != case.expected {
                failures.push(format!(
                    "--- {}:{}\n#data\n{}\n#expected\n{}#actual\n{}",
                    case.file, case.line, case.data, case.expected, actual
                ));
            }
        }
    }

    assert!(total > 0, "no fixture cases found in {}", dir.display());
    if !failures.is_empty() {
        panic!(
            "{} of {} tree construction cases failed\n\n{}",
            failures.len(),
            total,
            failures.join("\n")
        );
    }
}
