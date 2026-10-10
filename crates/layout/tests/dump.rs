//! Data-driven layout dump tests.
//!
//! Each `tests/fixtures/*.html` is laid out at a fixed 400×300 viewport
//! with the bundled fonts. The resulting [`LayoutTree::dump`] is compared
//! to the sibling `*.txt`. Numbers in the dump are already rounded to two
//! decimals.
//!
//! `UPDATE_FIXTURES=1 cargo test -p luma-layout --test dump` rewrites the
//! expected files.

use std::fs;
use std::path::Path;

use luma_layout::layout;
use luma_style::{Viewport, style_document};
use luma_text::FontContext;

const VIEWPORT: Viewport = Viewport {
    width: 400.0,
    height: 300.0,
};

#[test]
fn layout_dump_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    let fonts = FontContext::new();
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("fixtures directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "html"))
        .collect();
    paths.sort();

    let mut total = 0;
    let mut failures = Vec::new();

    for html_path in paths {
        total += 1;
        let name = html_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let html = fs::read_to_string(&html_path).unwrap();
        let doc = luma_html::parse(&html);
        let styles = style_document(&doc, &[], VIEWPORT);
        let tree = layout(&doc, &styles, &fonts, VIEWPORT);
        let actual = tree.dump(Some(&doc));

        let txt_path = html_path.with_extension("txt");
        if update {
            fs::write(&txt_path, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&txt_path).unwrap_or_else(|_| {
            panic!(
                "missing {} — run UPDATE_FIXTURES=1 cargo test -p luma-layout --test dump",
                txt_path.display()
            )
        });
        if actual != expected {
            failures.push(format!(
                "--- {name}\n#expected\n{expected}#actual\n{actual}"
            ));
        }
    }

    assert!(total > 0, "no fixture cases found in {}", dir.display());
    if !failures.is_empty() {
        panic!(
            "{} of {} layout dump fixtures failed\n\n{}",
            failures.len(),
            total,
            failures.join("\n")
        );
    }
}
