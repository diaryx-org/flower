//! Map a file path's extension onto a `fig::Format`.

use std::path::Path;

use fig::Format;

/// Detect the config format from `path`'s extension, or `None` if unrecognized.
pub fn detect(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "json" => Format::Json,
        "jsonc" => Format::Jsonc,
        "json5" => Format::Json5,
        "yaml" | "yml" => Format::Yaml,
        "toml" => Format::Toml,
        "zon" => Format::Zon,
        "fig" | "figl" => Format::Fig,
        _ => return None,
    })
}
