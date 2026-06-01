//! End-to-end check that the `#[whisker_router::route]` macro
//! generates a `Route` impl byte-for-byte equivalent to the
//! hand-written one in `whisker_router::route::tests`.

use whisker_router::route::{Route, RouteError};

#[whisker_router::route]
#[derive(Clone, Debug, PartialEq)]
pub enum AppRoute {
    #[at("/")]
    Home,
    #[at("/profile/:id")]
    Profile { id: u64 },
    #[at("/profile/:id/posts/:slug")]
    Post { id: u64, slug: String },
    #[at("/settings")]
    Settings,
}

#[test]
fn home_round_trips() {
    let r = AppRoute::parse("/").unwrap();
    assert_eq!(r, AppRoute::Home);
    assert_eq!(r.to_path(), "/");
}

#[test]
fn profile_round_trips_with_numeric_param() {
    let r = AppRoute::parse("/profile/42").unwrap();
    assert_eq!(r, AppRoute::Profile { id: 42 });
    assert_eq!(r.to_path(), "/profile/42");
}

#[test]
fn settings_round_trips() {
    let r = AppRoute::parse("/settings").unwrap();
    assert_eq!(r, AppRoute::Settings);
    assert_eq!(r.to_path(), "/settings");
}

#[test]
fn multi_param_with_string_field() {
    let r = AppRoute::parse("/profile/7/posts/hello-world").unwrap();
    assert_eq!(
        r,
        AppRoute::Post {
            id: 7,
            slug: "hello-world".into()
        }
    );
    assert_eq!(r.to_path(), "/profile/7/posts/hello-world");
}

#[test]
fn empty_path_resolves_to_home() {
    assert_eq!(AppRoute::parse("").unwrap(), AppRoute::Home);
}

#[test]
fn trailing_slash_tolerated() {
    assert_eq!(AppRoute::parse("/settings/").unwrap(), AppRoute::Settings);
}

#[test]
fn no_match_returns_no_match_error() {
    let err = AppRoute::parse("/blog/123").unwrap_err();
    assert!(matches!(err, RouteError::NoMatch(_)));
}

#[test]
fn bad_param_surfaces_field_name() {
    let err = AppRoute::parse("/profile/notanumber").unwrap_err();
    match err {
        RouteError::BadParam { param, value } => {
            assert_eq!(param, "id");
            assert_eq!(value, "notanumber");
        }
        other => panic!("expected BadParam, got {other:?}"),
    }
}
