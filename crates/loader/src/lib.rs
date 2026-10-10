//! Resource loading for Luma: turning URLs into bytes.
//!
//! [`load`] fetches a [`Url`] and returns its bytes as a [`Resource`].
//! [`resolve`] resolves a reference such as an `href` attribute against a
//! base URL, following the relative resolution algorithm of [RFC 3986
//! §5][rfc3986] as refined by the [WHATWG URL Standard][whatwg-url].
//! [`url_from_cli_arg`] turns a command-line argument, which may be either a
//! URL or a filesystem path, into an absolute URL.
//!
//! URL parsing and resolution are delegated to the [`url`] crate, which
//! implements the WHATWG URL Standard; nothing here hand-rolls URL syntax.
//!
//! [rfc3986]: https://www.rfc-editor.org/rfc/rfc3986#section-5
//! [whatwg-url]: https://url.spec.whatwg.org/
//!
//! # Simplifications
//!
//! - Only the `file` scheme is supported. Every other scheme fails with
//!   [`LoadError::UnsupportedScheme`].
//! - The content type of a file is guessed from its extension.
//! - Text is always decoded as UTF-8 ([`Resource::text`]), lossily. There is
//!   no charset sniffing and `<meta charset>` is ignored.

use std::borrow::Cow;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub use url::Url;

/// A loaded resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// The URL the bytes were loaded from. Relative references inside the
    /// resource resolve against this.
    pub url: Url,
    /// The raw contents.
    pub bytes: Vec<u8>,
    /// MIME type, e.g. `text/html`, if known.
    pub content_type: Option<String>,
}

impl Resource {
    /// The contents decoded as UTF-8, with invalid sequences replaced by
    /// U+FFFD and a leading byte order mark removed.
    pub fn text(&self) -> Cow<'_, str> {
        let bytes = self
            .bytes
            .strip_prefix(b"\xEF\xBB\xBF")
            .unwrap_or(&self.bytes);
        String::from_utf8_lossy(bytes)
    }
}

/// Why a resource could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Nothing exists at the URL.
    NotFound,
    /// The resource exists but could not be read. Holds the OS error text.
    Io(String),
    /// The URL's scheme is not one this loader handles. Holds the scheme.
    UnsupportedScheme(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound => f.write_str("not found"),
            LoadError::Io(msg) => f.write_str(msg),
            LoadError::UnsupportedScheme(scheme) => {
                write!(f, "unsupported URL scheme `{scheme}`")
            }
        }
    }
}

impl std::error::Error for LoadError {}

impl From<io::Error> for LoadError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => LoadError::NotFound,
            _ => LoadError::Io(e.to_string()),
        }
    }
}

/// Load the resource at `url`.
pub fn load(url: &Url) -> Result<Resource, LoadError> {
    match url.scheme() {
        "file" => load_file(url),
        other => Err(LoadError::UnsupportedScheme(other.to_owned())),
    }
}

fn load_file(url: &Url) -> Result<Resource, LoadError> {
    let path = url
        .to_file_path()
        .map_err(|()| LoadError::Io(format!("`{url}` is not a local file path")))?;
    let bytes = std::fs::read(&path)?;
    Ok(Resource {
        url: url.clone(),
        bytes,
        content_type: content_type_for(&path).map(str::to_owned),
    })
}

fn content_type_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" | "htm" | "xhtml" => "text/html",
        "css" => "text/css",
        "txt" => "text/plain",
        _ => return None,
    })
}

/// Resolve `reference` (e.g. an `href` value) against `base`.
///
/// Absolute references replace the base entirely; relative ones are merged
/// with it, and `.`/`..` segments are removed. Leading and trailing spaces
/// in `reference` are ignored, as in HTML attributes. Returns `None` if the
/// reference cannot be parsed.
pub fn resolve(base: &Url, reference: &str) -> Option<Url> {
    base.join(reference).ok()
}

/// Interpret a command-line argument as a URL.
///
/// An argument that names an existing file is always a path, even if it
/// also looks like a URL. Otherwise, anything that parses as an absolute URL
/// with a scheme longer than one character (so that Windows drive letters
/// like `C:` are not mistaken for schemes) is a URL. Everything else is a
/// path, made absolute against the current directory and canonicalised if it
/// exists, then converted to a `file://` URL.
pub fn url_from_cli_arg(arg: &str) -> Url {
    let path = Path::new(arg);
    if !path.exists()
        && let Ok(url) = Url::parse(arg)
        && url.scheme().len() > 1
    {
        return url;
    }
    file_url(path)
}

fn file_url(path: &Path) -> Url {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    let absolute = path
        .canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| PathBuf::from("/"));
    Url::from_file_path(&absolute)
        .unwrap_or_else(|()| Url::parse("file:///").expect("`file:///` is a valid URL"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn resolved(base: &str, reference: &str) -> String {
        resolve(&url(base), reference).unwrap().to_string()
    }

    const PAGE: &str = "file:///site/docs/index.html";

    #[test]
    fn resolve_sibling() {
        assert_eq!(resolved(PAGE, "style.css"), "file:///site/docs/style.css");
        assert_eq!(resolved(PAGE, "./style.css"), "file:///site/docs/style.css");
        assert_eq!(
            resolved(PAGE, "css/main.css"),
            "file:///site/docs/css/main.css"
        );
    }

    #[test]
    fn resolve_dot_dot() {
        assert_eq!(resolved(PAGE, "../style.css"), "file:///site/style.css");
        assert_eq!(
            resolved(PAGE, "a/../b/./c.css"),
            "file:///site/docs/b/c.css"
        );
        // `..` cannot climb above the root.
        assert_eq!(resolved(PAGE, "../../../../x.css"), "file:///x.css");
    }

    #[test]
    fn resolve_root_relative() {
        assert_eq!(resolved(PAGE, "/assets/x.css"), "file:///assets/x.css");
    }

    #[test]
    fn resolve_query_and_fragment() {
        assert_eq!(resolved(PAGE, "?v=2"), "file:///site/docs/index.html?v=2");
        assert_eq!(resolved(PAGE, "#top"), "file:///site/docs/index.html#top");
        assert_eq!(
            resolved("file:///a/b.html?q=1#f", "c.css?v=3#x"),
            "file:///a/c.css?v=3#x"
        );
        // A fragment-only reference keeps the base's query.
        assert_eq!(
            resolved("file:///a/b.html?q=1", "#x"),
            "file:///a/b.html?q=1#x"
        );
    }

    #[test]
    fn resolve_empty_reference_is_base_without_fragment() {
        assert_eq!(resolved("file:///a/b.html#f", ""), "file:///a/b.html");
    }

    #[test]
    fn resolve_absolute_overrides_base() {
        assert_eq!(
            resolved(PAGE, "https://example.com/x.css"),
            "https://example.com/x.css"
        );
        assert_eq!(resolved(PAGE, "file:///other.css"), "file:///other.css");
        assert_eq!(
            resolved("https://a.example/p/q", "//b.example/x"),
            "https://b.example/x"
        );
    }

    #[test]
    fn resolve_rfc3986_examples() {
        // RFC 3986 §5.4.1, normal examples.
        let base = "http://a/b/c/d;p?q";
        for (reference, expected) in [
            ("g", "http://a/b/c/g"),
            ("./g", "http://a/b/c/g"),
            ("g/", "http://a/b/c/g/"),
            ("/g", "http://a/g"),
            ("//g", "http://g/"),
            ("?y", "http://a/b/c/d;p?y"),
            ("g?y", "http://a/b/c/g?y"),
            ("#s", "http://a/b/c/d;p?q#s"),
            ("g#s", "http://a/b/c/g#s"),
            (";x", "http://a/b/c/;x"),
            ("", "http://a/b/c/d;p?q"),
            (".", "http://a/b/c/"),
            ("..", "http://a/b/"),
            ("../g", "http://a/b/g"),
            ("../..", "http://a/"),
            ("../../g", "http://a/g"),
        ] {
            assert_eq!(
                resolved(base, reference),
                expected,
                "reference {reference:?}"
            );
        }
    }

    #[test]
    fn resolve_trims_whitespace_and_rejects_garbage() {
        assert_eq!(
            resolved(PAGE, "  style.css \n"),
            "file:///site/docs/style.css"
        );
        assert_eq!(resolve(&url("https://a.example/"), "http://[::1"), None);
    }

    #[test]
    fn cli_arg_existing_relative_path() {
        // Tests run with the crate directory as the working directory.
        let u = url_from_cli_arg("Cargo.toml");
        assert_eq!(u.scheme(), "file");
        assert!(u.path().ends_with("/crates/loader/Cargo.toml"), "{u}");
        let path = u.to_file_path().unwrap();
        assert!(path.is_absolute());
        assert!(path.exists());
    }

    #[test]
    fn cli_arg_is_canonicalised() {
        assert_eq!(
            url_from_cli_arg("src/../Cargo.toml"),
            url_from_cli_arg("Cargo.toml")
        );
        assert_eq!(
            url_from_cli_arg("./Cargo.toml"),
            url_from_cli_arg("Cargo.toml")
        );
    }

    #[test]
    fn cli_arg_absolute_path() {
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        assert_eq!(url_from_cli_arg(manifest), url_from_cli_arg("Cargo.toml"));
    }

    #[test]
    fn cli_arg_missing_path_is_still_absolute() {
        let u = url_from_cli_arg("does/not/exist.html");
        assert_eq!(u.scheme(), "file");
        assert!(
            u.path().ends_with("/crates/loader/does/not/exist.html"),
            "{u}"
        );
        assert_eq!(load(&u), Err(LoadError::NotFound));
    }

    #[test]
    fn cli_arg_path_with_spaces_is_percent_encoded() {
        let u = url_from_cli_arg("my page.html");
        assert!(u.path().ends_with("/my%20page.html"), "{u}");
        assert!(u.to_file_path().unwrap().ends_with("my page.html"));
    }

    #[test]
    fn cli_arg_url_is_kept() {
        assert_eq!(
            url_from_cli_arg("https://example.com/a?b#c").as_str(),
            "https://example.com/a?b#c"
        );
        assert_eq!(
            url_from_cli_arg("file:///tmp/x.html").as_str(),
            "file:///tmp/x.html"
        );
    }

    #[test]
    fn cli_arg_drive_letter_is_not_a_scheme() {
        assert_eq!(url_from_cli_arg("c:/nope.html").scheme(), "file");
    }

    #[test]
    fn cli_arg_empty_is_current_directory() {
        assert_eq!(url_from_cli_arg(""), url_from_cli_arg("."));
    }

    #[test]
    fn load_file() {
        let u = url_from_cli_arg("Cargo.toml");
        let r = load(&u).unwrap();
        assert_eq!(r.url, u);
        assert!(r.text().contains("name = \"luma-loader\""));
        assert_eq!(r.content_type, None);
    }

    #[test]
    fn load_unsupported_scheme() {
        assert_eq!(
            load(&url("https://example.com/")),
            Err(LoadError::UnsupportedScheme("https".into()))
        );
    }

    #[test]
    fn load_directory_is_io_error() {
        assert!(matches!(
            load(&url_from_cli_arg("src")),
            Err(LoadError::Io(_))
        ));
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type_for(Path::new("a/b.HTML")), Some("text/html"));
        assert_eq!(content_type_for(Path::new("b.css")), Some("text/css"));
        assert_eq!(content_type_for(Path::new("b")), None);
    }

    #[test]
    fn text_is_lossy_and_strips_bom() {
        let r = Resource {
            url: url("file:///x"),
            bytes: b"\xEF\xBB\xBFa\xFFb".to_vec(),
            content_type: None,
        };
        assert_eq!(r.text(), "a\u{FFFD}b");
    }
}
