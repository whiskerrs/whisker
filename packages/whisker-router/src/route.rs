//! [`Route`] trait + [`RouteError`] — the typed-enum ↔ URL bridge.
//!
//! Implement [`Route`] manually for full control, or use the
//! [`#[route]`](macro@crate::route) attribute macro to derive `parse` and
//! `to_path` from per-variant `#[at("/...")]` patterns. The hidden
//! `hand_written_example` test module shows what a derived impl
//! looks like under the hood.

use core::fmt;

/// A typed routing target — the value pushed onto a
/// [`RouteStack`](crate::RouteStack).
///
/// Implementors round-trip between an in-memory enum value and the
/// canonical URL path. The crate uses `parse` to resolve deep links
/// and the `#[at(...)]` macro tests; `to_path` is the inverse used by
/// linking helpers and debug formatting.
///
/// # Bounds
///
/// `Clone + PartialEq + 'static` are required by
/// [`RouteStack`](crate::RouteStack) so the runtime can put values in
/// a signal, compare entries for equality, and ferry the value into
/// reactive closures without lifetime gymnastics.
///
/// # Example
///
/// The common path is the [`#[route]`](macro@crate::route) macro:
///
/// ```ignore
/// use whisker_router::route;
///
/// #[route]
/// #[derive(Clone, Debug, PartialEq)]
/// pub enum AppRoute {
///     #[at("/")]             Home,
///     #[at("/profile/:id")]  Profile { id: u64 },
/// }
/// ```
///
/// Hand-written impls are also supported — see this file's tests for
/// the canonical shape.
pub trait Route: Clone + PartialEq + 'static {
    /// Parse a URL path (e.g. `/profile/42`) into a route value.
    ///
    /// Implementations should tolerate a trailing slash and an
    /// optional leading slash — the macro-generated impl does both.
    fn parse(path: &str) -> Result<Self, RouteError>
    where
        Self: Sized;

    /// Canonical URL path for this route value. Must round-trip with
    /// [`Self::parse`].
    fn to_path(&self) -> String;
}

/// Errors surfaced from [`Route::parse`].
///
/// Implements [`std::error::Error`] so callers can bubble it through
/// `?` in deep-link handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// The input didn't match any defined route. Payload is the
    /// original path for diagnostics.
    NoMatch(String),
    /// A path parameter (e.g. `:id`) failed to parse into the field's
    /// declared type.
    BadParam {
        /// Parameter name whose conversion failed.
        param: &'static str,
        /// Raw segment value that didn't parse.
        value: String,
    },
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::NoMatch(p) => write!(f, "no route matches path {p:?}"),
            RouteError::BadParam { param, value } => {
                write!(f, "bad param {param} = {value:?}")
            }
        }
    }
}

impl std::error::Error for RouteError {}

// Canonical hand-written shape — kept compiled under cfg(test) so it
// stays in sync with the macro's emitted code and doubles as a
// reference for users who want to bypass the macro.
#[cfg(test)]
mod hand_written_example {
    use super::*;

    /// Typical app's top-level route.
    #[derive(Clone, Debug, PartialEq)]
    pub enum AppRoute {
        Home,
        Profile { id: u64 },
        Settings,
    }

    impl Route for AppRoute {
        fn parse(path: &str) -> Result<Self, RouteError> {
            // Strip trailing slash + the leading one so segments
            // line up regardless of how the URL was written.
            let normalized = path.trim_end_matches('/');
            let normalized = normalized.strip_prefix('/').unwrap_or(normalized);
            let segments: Vec<&str> = if normalized.is_empty() {
                Vec::new()
            } else {
                normalized.split('/').collect()
            };

            match segments.as_slice() {
                [] => Ok(AppRoute::Home),
                ["profile", id_str] => {
                    let id = id_str.parse::<u64>().map_err(|_| RouteError::BadParam {
                        param: "id",
                        value: (*id_str).to_string(),
                    })?;
                    Ok(AppRoute::Profile { id })
                }
                ["settings"] => Ok(AppRoute::Settings),
                _ => Err(RouteError::NoMatch(path.to_string())),
            }
        }

        fn to_path(&self) -> String {
            match self {
                AppRoute::Home => "/".into(),
                AppRoute::Profile { id } => format!("/profile/{id}"),
                AppRoute::Settings => "/settings".into(),
            }
        }
    }

    #[test]
    fn round_trip_home() {
        let r = AppRoute::parse("/").unwrap();
        assert_eq!(r, AppRoute::Home);
        assert_eq!(r.to_path(), "/");
    }

    #[test]
    fn round_trip_profile() {
        let r = AppRoute::parse("/profile/42").unwrap();
        assert_eq!(r, AppRoute::Profile { id: 42 });
        assert_eq!(r.to_path(), "/profile/42");
    }

    #[test]
    fn round_trip_settings() {
        let r = AppRoute::parse("/settings").unwrap();
        assert_eq!(r, AppRoute::Settings);
        assert_eq!(r.to_path(), "/settings");
    }

    #[test]
    fn trailing_slash_tolerated() {
        assert_eq!(AppRoute::parse("/settings/").unwrap(), AppRoute::Settings);
    }

    #[test]
    fn empty_path_is_home() {
        // Some callers strip everything before handing off.
        assert_eq!(AppRoute::parse("").unwrap(), AppRoute::Home);
    }

    #[test]
    fn unknown_path_no_match() {
        let err = AppRoute::parse("/blog/123").unwrap_err();
        assert!(matches!(err, RouteError::NoMatch(_)));
        assert_eq!(format!("{err}"), "no route matches path \"/blog/123\"");
    }

    #[test]
    fn bad_param_surfaces_name_and_value() {
        let err = AppRoute::parse("/profile/notanumber").unwrap_err();
        match err {
            RouteError::BadParam { param, value } => {
                assert_eq!(param, "id");
                assert_eq!(value, "notanumber");
            }
            other => panic!("expected BadParam, got {other:?}"),
        }
    }

    #[test]
    fn route_error_display_for_bad_param() {
        let err = RouteError::BadParam {
            param: "id",
            value: "x".into(),
        };
        assert_eq!(format!("{err}"), "bad param id = \"x\"");
    }
}
