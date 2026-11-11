//! Map a file path's extension onto a `fig::Format`.
//!
//! fig's core names no language: a [`Format`] is a per-process handle a
//! language crate hands back when it registers. flower opens the formats of the
//! language crates it depends on — JSON (with JSONC and JSON5), YAML, TOML, ZON
//! and figl — and registers each on first use here.

use std::path::Path;

use fig::Format;

/// Detect the config format from `path`'s extension, or `None` if unrecognized.
pub fn detect(path: &Path) -> Option<Format> {
    from_extension(path.extension()?.to_str()?)
}

/// The format a file extension (or format name) names, case-insensitively:
/// `json`, `jsonc`, `json5`, `yaml`/`yml`, `toml`, `zon`, `figl` (or fig 5's
/// `fig`). `None` for anything else.
pub fn from_extension(ext: &str) -> Option<Format> {
    let ext = ext.to_ascii_lowercase();
    // Each crate registers its language on its first `format()`; JSON's
    // registers its JSONC and JSON5 dialects with it, found by name after.
    let registered = match ext.as_str() {
        "json" | "jsonc" | "json5" => fig_json::format().map(|_| ext.as_str()),
        "yaml" | "yml" => fig_yaml::format().map(|_| "yaml"),
        "toml" => fig_toml::format().map(|_| "toml"),
        "zon" => fig_zon::format().map(|_| "zon"),
        "fig" | "figl" => fig_figl::format().map(|_| "figl"),
        _ => return None,
    };
    // Registration refuses only a name already taken by another language, and
    // these are fig's own crates under their own names.
    let name = registered.unwrap_or_else(|e| panic!("fig refused to register {ext}: {e}"));
    Format::by_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_names_its_format() {
        for (ext, name) in [
            ("json", "json"),
            ("JSONC", "jsonc"),
            ("json5", "json5"),
            ("yml", "yaml"),
            ("yaml", "yaml"),
            ("toml", "toml"),
            ("zon", "zon"),
            ("fig", "figl"),
            ("figl", "figl"),
        ] {
            assert_eq!(from_extension(ext).map(Format::name), Some(name), "{ext}");
        }
        assert_eq!(from_extension("ini"), None);
        assert_eq!(detect(Path::new("a/b.yml")).map(Format::name), Some("yaml"));
        assert_eq!(detect(Path::new("Makefile")), None);
    }

    #[test]
    fn zon_and_figl_open() {
        let zon = from_extension("zon").unwrap();
        crate::FigBackend::open(b".{\n    .name = .flower,\n}\n", zon).expect("zon");
        let figl = from_extension("figl").unwrap();
        crate::FigBackend::open(b"name = flower\n", figl).expect("figl");
        crate::FigBackend::open(b"", figl).expect("empty figl");
    }
}
