//! Podcast browser — production-layered Whisker example.
//!
//! ## Architecture
//!
//! Composition root only. Real work lives in the sub-crates:
//!
//! ```text
//! podcast-theme               ← design tokens (colors, type scale, spacing)
//!                                no deps; pure consts
//! podcast-domain              ← Podcast / ChartSection value types
//!                                no deps; pure types + serde
//! podcast-data                ← iTunes Search API client + repositories
//!                                depends on: domain, ureq, serde
//! podcast-ui-kit              ← reusable atomic widgets
//!                                depends on: whisker, theme, domain
//! podcast-feature-browse      ← Browse screen (sections + cards)
//!                                depends on: whisker, theme, domain,
//!                                            data, ui-kit
//! podcast-feature-detail      ← Show detail screen
//!                                depends on: whisker, theme, domain,
//!                                            ui-kit
//! ```
//!
//! The top-level crate (this one) owns the route tree (the `routes! { … }`
//! that drives screen-to-screen navigation) and provides the shared
//! [`PodcastIndex`] context. It also publishes a [`Navigator`] context so
//! the feature crates can call `show_detail` / `go_back` without depending
//! on `whisker-router` directly — keeps Browse and Detail unaware of the
//! routing layer.
//!
//! Adding a new screen later (Now Playing, Library) means a new
//! `podcast-feature-*` crate, a new `Route(..)` line in `routes!`
//! here, and a method on `Navigator` to navigate to it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use podcast_domain::{NowPlaying, Podcast};
use podcast_feature_browse::BrowseScreen;
use podcast_feature_detail::DetailScreen;
use podcast_feature_search::SearchScreen;
use podcast_routing::Navigator;
use podcast_ui_kit::MiniPlayer;
use whisker::ArcRwSignal;
use whisker::css::{Display, FlexDirection, PositionKind};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_audio::Player;
use whisker_router::{AndroidPredictiveBack, Outlet, Router, SwipeBack, routes, use_navigator};

/// Process-wide table mapping a podcast `id` to its full [`Podcast`]
/// value. `Browse` populates it from the resource result as soon as
/// the iTunes Search response lands; `Detail` looks an entry up at
/// render time. Stored in [`provide_context`] so neither feature
/// crate has to thread the registry through component props.
///
/// `Rc<RefCell<...>>` instead of a [`RwSignal`] because the
/// detail screen's lookup is one-shot — it reads the podcast at
/// render and binds the immutable value into the component body.
/// A reactive signal would force the whole detail tree to re-render
/// every time browse pushes a new chart row.
pub type PodcastIndex = Rc<RefCell<HashMap<u64, Podcast>>>;

/// Process-wide reactive "what's playing right now". `None` when
/// nothing has been queued yet (cold start, before the user taps
/// an episode); `Some` while the mini-player is showing a track.
///
/// Exposed as a type alias — same TypeId across crates — so the
/// detail screen (writes on tap) and the mini-player (reads to
/// render) match on `use_context<NowPlayingSignal>()` without
/// importing it from this shell crate.
pub type NowPlayingSignal = ArcRwSignal<Option<NowPlaying>>;

/// Build a [`Navigator`] from `use_navigator()`. Must be called inside a
/// `Router` subtree (where the handle is in context). Defined in the shell
/// (not in `podcast-routing`) because it's the only piece that knows the
/// concrete URLs the route tree exposes — keeps the routing crate free of
/// the `whisker-router` import.
fn build_navigator() -> Navigator {
    let nav_detail = use_navigator();
    let nav_search = use_navigator();
    let nav_back = use_navigator();
    Navigator {
        show_detail: Rc::new(move |id| {
            let _ = nav_detail.navigate(format!("/podcast/{id}").as_str());
        }),
        show_search: Rc::new(move || {
            let _ = nav_search.navigate("/search");
        }),
        go_back: Rc::new(move || {
            let _ = nav_back.back();
        }),
    }
}

#[whisker::main]
fn app() -> Element {
    let index: PodcastIndex = Rc::new(RefCell::new(HashMap::new()));
    let player = Player::new("");
    let now_playing: NowPlayingSignal = ArcRwSignal::new(None);

    provide_context(index);
    provide_context(player);
    provide_context(now_playing);

    render! {
        view(style: css!(
            flex_grow: 1.0,
            width: vw(100),
            height: vh(100),
            background_color: podcast_theme::BG,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            position: PositionKind::Relative,
        )) {
            Router(routes: routes! {
                Stack {
                    Route(path: "", component: BrowseScreen)
                    Route(path: "podcast/:id", component: DetailScreen)
                    Route(path: "search", component: SearchScreen)
                }
            }) {
                PodcastRouter {
                    Outlet {}
                    SwipeBack {}
                    AndroidPredictiveBack {}
                }
            }
            MiniPlayer()
        }
    }
}

#[component]
fn podcast_router(children: Children) -> Element {
    provide_context(build_navigator());
    render! { children() }
}
