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

use podcast_domain::Podcast;
use podcast_feature_browse::{BrowseScreen, BrowseScreenProps};
use podcast_feature_detail::{DetailScreen, DetailScreenProps};
use podcast_routing::{AppRoute, Navigator};
use whisker::css::{Display, FlexDirection};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_router::stack::{route_stack, RouteStack};

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

/// Build a [`Navigator`] backed by the given route stack. Defined
/// in the shell (not in `podcast-routing`) because it's the only
/// piece that knows how to wire the closures to a concrete
/// [`whisker_router::RouteStack`] — keeps the routing crate free
/// of the `whisker-router::push` import too.
fn navigator_from_stack(stack: RouteStack<AppRoute>) -> Navigator {
    let stack_for_push = stack.clone();
    let stack_for_back = stack;
    Navigator {
        show_detail: Rc::new(move |id| {
            stack_for_push.push(AppRoute::Detail { id });
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

    // Push the shared state into context once, on the `app()`
    // owner — the ancestor of every screen rendered below.
    provide_context(navigator);
    provide_context(index);

    // We bypass `whisker_router::StackLayout` and dispatch on the
    // current route by reading the stack's entries signal in a
    // reactive child. `StackLayout` mounts each screen in a
    // detached `create_owner(None)` (see
    // `packages/whisker-router/src/layouts/stack.rs:184`), and any
    // screen heavier than a trivial view panics on the first tick
    // — see follow-up issue. Routing still goes through the typed
    // `RouteStack` so the back-button machinery and deep-link
    // surface stay intact; we just don't get the animated push /
    // pop transitions until the panic is fixed.
    let entries = stack.entries();
    render! {
        page(style: css!(
            width: vw(100),
            height: vh(100),
            background_color: podcast_theme::BG,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            DynRoute(entries: entries)
        }
    }
}

/// Re-renders whichever screen [`AppRoute`] picks. Read the
/// `entries` signal inside a `#[component]` body so the macro's
/// reactive remount wrapper re-fires when the route changes.
#[component]
fn dyn_route(entries: ReadSignal<Vec<whisker_router::stack::RouteEntry<AppRoute>>>) -> Element {
    let route = entries
        .get()
        .last()
        .map(|e| e.route.clone())
        .unwrap_or(AppRoute::Browse);
    match route {
        AppRoute::Browse => render! { BrowseScreen() },
        AppRoute::Detail { id } => render! { DetailScreen(id: id) },
    }
}
