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
//! The top-level crate (this one) owns the [`AppRoute`] enum, the
//! [`RouteStack`] that drives screen-to-screen navigation, and the
//! shared [`PodcastIndex`] context. It also publishes a
//! [`Navigator`] context so the feature crates can call `show_detail`
//! / `go_back` without depending on `whisker-router` directly —
//! keeps Browse and Detail unaware of the routing layer.
//!
//! Adding a new screen later (Now Playing, Library) means a new
//! `podcast-feature-*` crate, a new [`AppRoute`] variant, and a
//! match arm in the `StackLayout`'s `render` closure here — plus a
//! method on `Navigator` to push it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use podcast_domain::{NowPlaying, Podcast};
use podcast_feature_browse::BrowseScreen;
use podcast_feature_detail::DetailScreen;
use podcast_feature_search::SearchScreen;
use podcast_routing::{AppRoute, Navigator};
use podcast_ui_kit::MiniPlayer;
use whisker::css::{Display, FlexDirection, PositionKind};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::ArcRwSignal;
use whisker_audio::Player;
use whisker_router::stack::{route_stack, RouteStack};
use whisker_router::{
    AndroidPredictiveBack, IosSwipeBack, RouteProvider, RouteRenderFn, StackLayout,
};

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

/// Build a [`Navigator`] backed by the given route stack. Defined
/// in the shell (not in `podcast-routing`) because it's the only
/// piece that knows how to wire the closures to a concrete
/// [`whisker_router::RouteStack`] — keeps the routing crate free
/// of the `whisker-router::push` import too.
fn navigator_from_stack(stack: RouteStack<AppRoute>) -> Navigator {
    let stack_for_detail = stack.clone();
    let stack_for_search = stack.clone();
    let stack_for_back = stack;
    Navigator {
        show_detail: Rc::new(move |id| {
            stack_for_detail.push(AppRoute::Detail { id });
        }),
        show_search: Rc::new(move || {
            stack_for_search.push(AppRoute::Search);
        }),
        go_back: Rc::new(move || {
            stack_for_back.back();
        }),
    }
}

#[whisker::main]
fn app() -> Element {
    let stack = route_stack(AppRoute::Browse);
    let index: PodcastIndex = Rc::new(RefCell::new(HashMap::new()));
    let navigator = navigator_from_stack(stack.clone());

    // One process-wide audio player. Constructed with no source —
    // the detail screen calls `set_source` + `play` when the user
    // taps an episode. The `Player` handle is `Clone` (Rc-backed);
    // every consumer that pulls it from context gets a fresh
    // ref-counted handle pointing at the same native player, so
    // play / pause from the mini-player and the detail screen both
    // drive the same playback.
    let player = Player::new("");
    let now_playing: NowPlayingSignal = ArcRwSignal::new(None);

    // Push the shared state into context once, on the `app()`
    // owner — the ancestor of every screen rendered below.
    provide_context(navigator);
    provide_context(index);
    provide_context(player);
    provide_context(now_playing);

    let render: RouteRenderFn<AppRoute> = (|r: AppRoute| match r {
        AppRoute::Browse => render! { BrowseScreen() },
        AppRoute::Detail { id } => render! { DetailScreen(id: id) },
        AppRoute::Search => render! { SearchScreen() },
    })
    .into();

    render! {
        // `position: relative` anchors the absolutely-positioned
        // mini-player to the page itself, so the bar floats above
        // *every* route — browse and detail alike — instead of
        // being scoped to one screen's layout.
        page(style: css!(
            width: vw(100),
            height: vh(100),
            background_color: podcast_theme::BG,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            position: PositionKind::Relative,
        )) {
            RouteProvider(stack: stack) {
                StackLayout(render: render.clone()) {
                    IosSwipeBack()
                    AndroidPredictiveBack()
                }
            }
            // The mini-player renders AFTER the route stack in DOM
            // order so it paints on top — Lynx's animator quirk on
            // z-index during sibling animations makes paint order
            // the reliable lever. `MiniPlayer` itself is hidden
            // (Show fallback → fragment) while `NowPlayingSignal`
            // is `None`, so this mount is invisible on cold start.
            MiniPlayer()
        }
    }
}
