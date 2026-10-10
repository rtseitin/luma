//! The Luma browser engine entry point.
//!
//! `luma <path-or-url>` loads a document, loads its linked stylesheets,
//! runs the parse, style, and layout stages, and prints the requested
//! dumps (`--dump-text` and `--dump-layout` also select fonts and shape
//! the text). Run `luma --help` for the flags.
//!
//! Problems with the document's content never make the run fail: HTML and
//! CSS errors are recovered from, and stylesheets that fail to load are
//! reported on stderr and skipped. Only failing to load the document itself
//! exits with a non-zero status.

mod cli;
mod dump;
mod stylesheets;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;
use luma_css::Stylesheet;
use luma_dom::Document;
use luma_text::FontContext;

use crate::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();

    let url = luma_loader::url_from_cli_arg(&cli.input);
    let resource = match luma_loader::load(&url) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("luma: error: {}: {e}", cli.input);
            return ExitCode::FAILURE;
        }
    };

    let (doc, html_errors) = luma_html::parse_with_errors(&resource.text());
    if cli.verbose {
        eprintln!(
            "luma: loaded {} ({} bytes, {} HTML parse errors)",
            resource.url,
            resource.bytes.len(),
            html_errors.len()
        );
        let style_errors = inline_style_error_count(&doc);
        if style_errors > 0 {
            eprintln!("luma: {style_errors} CSS parse errors in <style> elements");
        }
    }

    let (linked, warnings) = stylesheets::load_linked_stylesheets(&doc, &resource.url);
    for warning in warnings {
        eprintln!("luma: warning: {warning}");
    }
    if cli.verbose {
        for sheet in &linked {
            eprintln!(
                "luma: loaded stylesheet {} ({} CSS parse errors)",
                sheet.url, sheet.error_count
            );
        }
    }
    let extra: Vec<Stylesheet> = linked.into_iter().map(|s| s.sheet).collect();
    let styles = luma_style::style_document(&doc, &extra, cli.viewport);

    let fonts = (cli.dump_text || cli.dump_layout).then(|| {
        if cli.system_fonts {
            FontContext::with_system_fonts()
        } else {
            FontContext::new()
        }
    });

    let mut out = String::new();
    if cli.dump_dom || !(cli.dump_style || cli.dump_text || cli.dump_layout) {
        out.push_str(&doc.dump());
    }
    if cli.dump_style {
        out.push_str(&dump::style_dump(&doc, &styles));
    }
    if cli.dump_layout {
        let fonts = fonts.as_ref().expect("fonts created for --dump-layout");
        let tree = luma_layout::layout(&doc, &styles, fonts, cli.viewport);
        out.push_str(&tree.dump(Some(&doc)));
    }
    if cli.dump_text {
        let fonts = fonts.as_ref().expect("fonts created for --dump-text");
        out.push_str(&dump::text_dump(&doc, &styles, fonts));
    }
    match io::stdout().lock().write_all(out.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        // `luma page.html | head` closing the pipe early is not a failure.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("luma: error: writing output: {e}");
            ExitCode::FAILURE
        }
    }
}

/// CSS errors in the document's `<style>` elements. `luma-style` parses
/// these itself and discards the errors, so this re-parses them.
fn inline_style_error_count(doc: &Document) -> usize {
    doc.descendants(doc.root())
        .filter(|&id| doc.element(id).is_some_and(|e| e.name == "style"))
        .map(|id| {
            luma_css::parse_stylesheet_with_errors(&doc.text_content(id))
                .1
                .len()
        })
        .sum()
}
