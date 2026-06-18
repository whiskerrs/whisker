//! Procedural macros for `whisker-asset`.
//!
//! All three macros take a single string-literal *logical* asset path,
//! relative to the invoking crate's `assets/` directory, and validate at
//! compile time that the file actually exists. They differ only in what
//! they expand to:
//!
//! - [`asset!`] — a **runtime** call `::whisker_asset::resolve("<rel>")`
//!   that composes a platform-absolute path/URL from the process-global
//!   base. The file is *not* embedded; it is expected to be bundled by a
//!   later build-plugin phase.
//! - [`asset_str!`] — `include_str!` of the file content → `&'static str`.
//! - [`asset_bytes!`] — `include_bytes!` of the file content →
//!   `&'static [u8]`.
//!
//! # Path rules
//!
//! The logical path is validated (not just existence-checked):
//!
//! - must be a non-empty string literal,
//! - must not be absolute / start with `/` (it is *relative* to
//!   `assets/`),
//! - must not contain a `..` traversal component,
//! - must not contain a Windows drive prefix or backslashes.
//!
//! Existence is checked against
//! `${CARGO_MANIFEST_DIR}/assets/<rel>` where `CARGO_MANIFEST_DIR` is the
//! manifest dir of the crate in which the macro is *invoked* (cargo sets
//! it per-crate during compilation), so the check is always anchored at
//! the right crate regardless of where the source file lives.

use proc_macro::TokenStream;
use quote::quote;
use std::path::{Component, Path, PathBuf};
use syn::{LitStr, parse_macro_input};

/// Validate the logical path and return the absolute on-disk path of the
/// asset (`${CARGO_MANIFEST_DIR}/assets/<rel>`), or a `syn::Error` with a
/// clear message pointing at the literal's span.
fn validate(lit: &LitStr) -> syn::Result<(String, PathBuf)> {
    let rel = lit.value();
    let span = lit.span();

    if rel.is_empty() {
        return Err(syn::Error::new(span, "asset path must not be empty"));
    }
    if rel.contains('\\') {
        return Err(syn::Error::new(
            span,
            "asset path must use '/' separators, not '\\'",
        ));
    }
    if rel.starts_with('/') {
        return Err(syn::Error::new(
            span,
            "asset path must be relative to the crate's `assets/` dir; \
             remove the leading '/'",
        ));
    }

    // Reject absolute paths, drive prefixes (`C:\`), root dirs, and any
    // `..` parent-dir traversal. Only normal components are allowed.
    let rel_path = Path::new(&rel);
    for comp in rel_path.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(syn::Error::new(
                    span,
                    "asset path must not contain a `..` traversal component",
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(syn::Error::new(
                    span,
                    "asset path must be relative (no root or drive prefix)",
                ));
            }
        }
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
        syn::Error::new(
            span,
            "CARGO_MANIFEST_DIR is not set; asset macros require cargo \
             to provide the invoking crate's manifest directory",
        )
    })?;

    let abs = Path::new(&manifest_dir).join("assets").join(&rel);
    if !abs.exists() {
        return Err(syn::Error::new(
            span,
            format!(
                "asset not found: `{}` (looked for `{}`). Place the file \
                 under your crate's `assets/` directory.",
                rel,
                abs.display()
            ),
        ));
    }

    Ok((rel, abs))
}

/// Reference a bundled asset by logical path. Compile-time-validated;
/// expands to a runtime `::whisker_asset::resolve("<rel>")` call that
/// returns the platform-absolute path/URL as a `String`.
///
/// ```ignore
/// let url = asset!("images/logo.png");
/// ```
#[proc_macro]
pub fn asset(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let (rel, _abs) = match validate(&lit) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };
    quote! { ::whisker_asset::resolve(#rel) }.into()
}

/// Embed a text asset's content. Compile-time-validated; expands to
/// `include_str!` → `&'static str`.
///
/// ```ignore
/// const SVG: &str = asset_str!("icons/logo.svg");
/// ```
#[proc_macro]
pub fn asset_str(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let abs = match validate(&lit) {
        Ok((_rel, abs)) => abs,
        Err(e) => return e.to_compile_error().into(),
    };
    let abs = abs.to_string_lossy().into_owned();
    quote! { ::core::include_str!(#abs) }.into()
}

/// Embed a binary asset's content. Compile-time-validated; expands to
/// `include_bytes!` → `&'static [u8]`.
///
/// ```ignore
/// const ICON: &[u8] = asset_bytes!("data/x.bin");
/// ```
#[proc_macro]
pub fn asset_bytes(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let abs = match validate(&lit) {
        Ok((_rel, abs)) => abs,
        Err(e) => return e.to_compile_error().into(),
    };
    let abs = abs.to_string_lossy().into_owned();
    quote! { ::core::include_bytes!(#abs) }.into()
}
