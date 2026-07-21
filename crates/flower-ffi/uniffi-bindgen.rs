//! The UniFFI bindings generator, in-tree so its version tracks the `uniffi`
//! runtime this crate links (a mismatch between generator and runtime is the
//! classic UniFFI footgun). Invoked by the scripts under `scripts/`:
//!
//! ```sh
//! cargo run -p flower-ffi --bin uniffi-bindgen -- \
//!   generate --library <libflower_ffi.dylib> --language swift --out-dir <dir>
//! ```
fn main() {
    uniffi::uniffi_bindgen_main()
}
