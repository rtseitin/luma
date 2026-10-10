//! Command-line arguments.

use clap::Parser;
use luma_style::Viewport;

/// Load an HTML document and run it through the Luma pipeline.
///
/// With no `--dump-*` flag, the DOM tree is printed.
#[derive(Debug, Parser)]
#[command(name = "luma", version)]
pub struct Cli {
    /// Path or URL of the document to load.
    pub input: String,

    /// Print the DOM tree in html5lib test format.
    #[arg(long)]
    pub dump_dom: bool,

    /// Print the computed style of every element, one line each.
    #[arg(long)]
    pub dump_style: bool,

    /// Print the selected font and measured width of every visible text node.
    #[arg(long)]
    pub dump_text: bool,

    /// Print the layout tree: one indented line per box with its rectangle.
    #[arg(long)]
    pub dump_layout: bool,

    /// Use installed system fonts as well as the bundled ones.
    #[arg(long)]
    pub system_fonts: bool,

    /// Viewport size in CSS pixels, used for media queries and `vw`/`vh`.
    #[arg(long, value_name = "WxH", default_value = "1024x768", value_parser = parse_viewport)]
    pub viewport: Viewport,

    /// Report loaded resources and parse error counts on stderr.
    #[arg(short, long)]
    pub verbose: bool,
}

/// Parse `WIDTHxHEIGHT`, e.g. `800x600`.
pub fn parse_viewport(s: &str) -> Result<Viewport, String> {
    let err = || format!("expected WIDTHxHEIGHT with positive numbers, e.g. 1024x768, got `{s}`");
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(err)?;
    let dimension = |v: &str| {
        v.trim()
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(err)
    };
    Ok(Viewport {
        width: dimension(w)?,
        height: dimension(h)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_parses() {
        assert_eq!(
            parse_viewport("800x600"),
            Ok(Viewport {
                width: 800.0,
                height: 600.0
            })
        );
        assert_eq!(parse_viewport("320X480.5").map(|v| v.height), Ok(480.5));
    }

    #[test]
    fn viewport_rejects_garbage() {
        for bad in [
            "", "800", "x600", "800x", "0x600", "-1x2", "axb", "infx1", "NaNx1",
        ] {
            assert!(parse_viewport(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn default_viewport_matches_style_default() {
        let cli = Cli::parse_from(["luma", "page.html"]);
        assert_eq!(cli.viewport, Viewport::default());
        assert!(!cli.dump_dom && !cli.dump_style && !cli.dump_text && !cli.dump_layout);
        assert!(!cli.verbose);
        assert!(!cli.system_fonts);
    }
}
