//! End-to-end tests of the `luma` binary against the example pages.

use std::path::PathBuf;
use std::process::{Command, Output};

fn example(name: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "examples", name]
        .iter()
        .collect();
    path.to_string_lossy().into_owned()
}

fn luma(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_luma"))
        .args(args)
        .output()
        .expect("run luma")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn line_for<'a>(dump: &'a str, tag: &str) -> &'a str {
    dump.lines()
        .find(|l| l.trim_start().starts_with(tag))
        .unwrap_or_else(|| panic!("no line for {tag} in\n{dump}"))
}

#[test]
fn every_example_runs_cleanly() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "html") {
            let o = luma(&[
                path.to_str().unwrap(),
                "--dump-dom",
                "--dump-style",
                "--dump-layout",
            ]);
            assert!(o.status.success(), "{}: {}", path.display(), stderr(&o));
            assert!(stdout(&o).starts_with("#document\n"));
            count += 1;
        }
    }
    assert!(count >= 4);
}

#[test]
fn default_output_is_the_dom() {
    let o = luma(&[&example("hello.html")]);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(
        out.starts_with("#document\n| <!DOCTYPE html>\n| <html>\n"),
        "{out}"
    );
    assert!(!out.contains("display="));
    assert!(stderr(&o).is_empty(), "{}", stderr(&o));
}

#[test]
fn dump_style_applies_linked_stylesheet() {
    let o = luma(&[&example("hello.html"), "--dump-style"]);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(
        !out.contains("#document"),
        "--dump-style alone omits the DOM"
    );

    let body = line_for(&out, "<body>");
    assert!(body.contains(" color=#222222 "), "{body}");
    assert!(body.contains(" background-color=#fafafa "), "{body}");
    assert!(body.contains(" margin=0 "), "{body}");

    let main = line_for(&out, "<main id=\"main\">");
    assert!(main.starts_with("    <main"), "indented by depth: {main:?}");
    assert!(main.contains(" margin=[0,auto,0,auto] "), "{main}");
    assert!(main.contains(" padding=[24,16,24,16] "), "{main}");

    // Inline <style> and style="" still apply alongside the linked sheet.
    assert!(line_for(&out, "<p class=\"note\">").contains(" color=#555555 "));
    assert!(line_for(&out, "<span>").contains(" color=#663399 "));
}

#[test]
fn viewport_drives_media_queries() {
    let wide = luma(&[&example("hello.html"), "--dump-style"]);
    let narrow = luma(&[
        &example("hello.html"),
        "--dump-style",
        "--viewport",
        "500x400",
    ]);
    assert!(line_for(&stdout(&wide), "<h1>").contains(" font-size=32 "));
    assert!(line_for(&stdout(&narrow), "<h1>").contains(" font-size=24 "));
}

#[test]
fn missing_document_fails_with_one_line() {
    let o = luma(&[&example("no-such-page.html")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).is_empty());
    let err = stderr(&o);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(
        err.contains("no-such-page.html") && err.contains("not found"),
        "{err}"
    );
}

#[test]
fn missing_stylesheet_warns_and_continues() {
    let o = luma(&[&example("broken.html"), "--dump-style"]);
    assert_eq!(o.status.code(), Some(0));
    let err = stderr(&o);
    assert!(
        err.contains("warning") && err.contains("does-not-exist.css"),
        "{err}"
    );
    // The second, valid <link> after the missing one is still applied.
    assert!(line_for(&stdout(&o), "<body>").contains(" color=#222222 "));
    assert!(line_for(&stdout(&o), "<div class=\"ok\">").contains(" color=#00aa00 "));
}

#[test]
fn verbose_reports_error_counts() {
    let quiet = luma(&[&example("broken.html")]);
    assert!(!stderr(&quiet).contains("parse errors"));

    let o = luma(&[&example("broken.html"), "--verbose"]);
    assert!(o.status.success());
    let err = stderr(&o);
    assert!(err.contains("HTML parse errors"), "{err}");
    assert!(err.contains("CSS parse errors in <style>"), "{err}");
    assert!(
        err.contains("loaded stylesheet") && err.contains("hello.css"),
        "{err}"
    );
}

#[test]
fn dump_text_measures_with_bundled_fonts() {
    let o = luma(&[&example("hello.html"), "--dump-text"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(
        !out.contains("#document"),
        "--dump-text alone omits the DOM"
    );
    assert!(
        !out.contains("Hello, Luma\" font=\"DejaVu Sans\" weight=400"),
        "title is hidden"
    );
    let h1 = line_for(&out, "\"Hello, Luma\"");
    assert!(
        h1.contains(" font=\"DejaVu Sans\" weight=700 style=Normal size=32 glyphs=11 "),
        "{h1}"
    );
    assert!(line_for(&out, "\"<link>\"").contains("font=\"DejaVu Sans Mono\""));

    // Deterministic: the same numbers every run.
    assert_eq!(stdout(&luma(&[&example("hello.html"), "--dump-text"])), out);
}

#[test]
fn dump_layout_prints_boxes() {
    let o = luma(&[&example("hello.html"), "--dump-layout"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(
        !out.contains("#document"),
        "--dump-layout alone omits the DOM"
    );
    assert!(out.starts_with("Block <html> at (0,0) size 1024x"), "{out}");
    assert!(out.contains("\n  Block <body> at "), "{out}");
    assert!(out.contains("\n    Block <main#main> at "), "{out}");
    assert!(out.contains("\n        Line at "), "{out}");
    assert!(out.contains("Text \"Hello, Luma\""), "{out}");
    assert!(out.contains("Marker \"1. \""), "{out}");
    assert_eq!(
        stdout(&luma(&[&example("hello.html"), "--dump-layout"])),
        out
    );
}

#[test]
fn bad_viewport_is_a_usage_error() {
    let o = luma(&[&example("hello.html"), "--viewport", "wide"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("--viewport"));
}
