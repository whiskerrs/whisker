# Router Design

Whisker's router is built on **two graphs**: a static **RouteTree** that
the `routes!` macro produces at compile time, and a dynamic **NavState**
that the runtime mutates as the user navigates. Everything the router
does — what URL a screen has, which screen is shown, where a `navigate`
pushes, where `back` returns — is *derived* from these two graphs plus a
single tie-break rule (declaration order). There is no hand-maintained
route table, no per-route priority config, and no separately-stored
"current screen" pointer.

This doc is the *design* of the router — the model and the "why". The
user-facing "how to declare routes" guide lives on
[whisker.rs/docs](https://whisker.rs/docs). The implementation lives in
`packages/whisker-router/*`.

> Status: **design** (this document). The existing
> `packages/whisker-router` is the prior, signal-backed-stack
> implementation; this document supersedes its model and is the target
> for the redesign.

## Why two graphs

Modern declarative routers (React Navigation's navigation-state tree,
Flutter Navigator 2.0's "stack as a function of state") all converge on
the same separation: a **static description** of the app's screen
structure, and a **runtime state** that drives what's on screen. Whisker
adopts this explicitly and pushes it to its minimal form:

- **RouteTree** — *what screens exist and how they nest.* Immutable,
  compile-time, produced by `routes!`. Determines URLs, the set of legal
  targets, the resolution rule, and per-screen animations.
- **NavState** — *what is live right now and how we got here.* Mutable,
  runtime, the RouteTree instantiated with state. Determines the current
  screen, where a push lands, and where a back returns.

The whole navigation domain closes over these two graphs and two ideas:
a **relative resolution rule** (which instance of an ambiguous route to
target) and a **derived `current`** (the shown screen is computed, never
stored).

## RouteTree (static)

The `routes!` macro lowers a nested block into a tree of three node
kinds:

| Node | Role | Children |
| --- | --- | --- |
| `Route` | A screen (leaf) | — |
| `Stack` | **Ordered** container: push/pop, has history | `Route` (+ `..spread`) |
| `Switch` | **Parallel** container: keeps all children alive, selects one, no history | containers (one per branch) |

`Tabs` is sugar for a `Switch` with a tab-bar UI; `Layout(X)` wraps a
container in a user-authored component (the Expo `_layout.tsx`
equivalent) that returns a navigator plus an `Outlet`. `Stack`, `Switch`
and `Route` are the only primitives.

**Containers are always "container + child" pairs.** A `Route` only
exists inside a `Stack` (or `Drawer`); a `Tab`/branch only inside a
`Switch`. A navigator never floats alone.

### URL derivation

A `Route`'s URL is the concatenation of the **named segments** along the
path from the root. Containers and tabs are typically *pathless* (they
contribute no segment), so they structure navigation without appearing
in the URL — the same trick as Expo Router's `(group)` folders.

```rust
let content = routes! {            // a reusable sub-route set is just a value
    Route("post/:id",    Post)
    Route("profile/:id", Profile)
};

let app = routes! {
    Stack {                                   // root stack
        Switch {                              // = Tabs (pathless)
            Stack { Route("", Timeline)        ..content }  // /  , /post/:id, /profile/:id
            Stack { Route("search", Search)    ..content }  // /search, /post/:id, ...
            Stack { Route("mypage", MyPage)    ..content }
        }
        Route("video/:id",  VideoPlayer)                    // /video/:id  (outside tabs)
        Route("compose",    Compose, present: Modal)        // /compose
    }
};
```

- The tabs are a `Switch`; each tab is its own `Stack` (independent
  history). `..content` spreads the shared sub-routes into each tab.
- `video`/`compose` are siblings of the `Switch` on the **root stack**, so
  they sit *above* the tabs (no tab bar) — purely a consequence of where
  they are in the tree.

### Shared routes are just a spread

`content` above is an ordinary `routes!` value spread with `..content`
into each tab's stack. There is no special "shared route" construct —
spreading the same group into N stacks creates N instances of
`/post/:id` that **dedupe to a single nav target** (`route::post(id)`),
because they share a group origin. Manually writing the same
`Route("post/:id", …)` in two stacks *without* a group is a compile error
(ambiguous).

### Per-screen animation

Transitions are **parameters of the `Route` in `routes!`**, not
attributes on the component, using forward/back slots modelled on Jetpack
Compose's `enter`/`exit`/`popEnter`/`popExit`:

```rust
routes! {
    Stack {
        Route("", Timeline)
        Route("post/:id", Post,
            enter     = Slide::from_right(),  // entering on navigate
            exit      = Slide::to_left(),     // covered by a pushed screen
            pop_enter = Slide::from_left(),   // revealed on back
            pop_exit  = Slide::to_right(),    // leaving on back
        )
        Route("compose", Compose, present: Modal,
            enter    = Slide::from_bottom(),
            pop_exit = Slide::to_bottom(),
        )
    }
}
```

The transition lives on the **`Route`, not the `#[component]`**, because
an animation is "how this *route* enters/leaves", not a property of the
UI itself. This matters for shared routes: the same `Post` component
spread into several stacks can be given a different transition at each
`Route` site. The component stays pure UI; `enter`/`exit`/`pop_enter`/
`pop_exit`/`present` are route parameters.

Containers (`Stack`/`Switch`) own *no* animation — they are passive views
that render whichever child the NavState selects. (See **Interactive
transitions** for the one nuance.)

### Chrome (tab bar) is a `Layout`, not the `Switch`

A `Switch` is **navigation logic only** — it decides which branch is
selected and renders it into an `Outlet`. It draws **no UI**: no tab bar,
no chrome. The bottom navigation (or top tabs, segmented control,
whatever) is drawn by the **`Layout(X)`** that wraps the `Switch`. This
keeps transition (the `Switch`) and chrome (the `Layout`) orthogonal.

The layout component renders the content area and the bar as an ordinary
flex column — `Outlet` (content, `flex_grow: 1`) above a fixed-height
bar:

```rust
#[layout]
#[component]
fn tabs_layout() -> Element {
    let active = use_active_tab();          // reads the Switch's `selected` (reactive)
    render! {
        view(style: css!(flex_grow: 1.0, display: Display::Flex,
                         flex_direction: FlexDirection::Column)) {
            view(style: css!(flex_grow: 1.0)) { Outlet {} }   // selected branch renders here
            view(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row,
                             height: px(56))) {                 // bottom navigation
                TabBarItem(icon: Icon::Home,   active: active == HomeTab::Home,
                           on_tap: move |_| nav.select_tab(HomeTab::Home))
                TabBarItem(icon: Icon::Search, active: active == HomeTab::Search,
                           on_tap: move |_| nav.select_tab(HomeTab::Search))
            }
        }
    }
}
```

```rust
routes! {
    Stack {
        Layout(TabsLayout) {        // draws the bottom-nav chrome
            Switch {                // selects which branch is shown
                Stack { Route("", Timeline)     ..content }
                Stack { Route("search", Search) ..content }
            }
        }
        Route("video/:id", VideoPlayer)   // outside the Layout ⇒ no tab bar
    }
}
```

Two properties fall out of this structure:

- **The bar is persistent.** The `Layout` is the *parent* of the
  `Switch`, so it stays mounted while the `Switch` swaps branch content —
  the bottom nav is not re-rendered on tab change.
- **"Outside the tabs" = outside the `Layout`.** `video` is a sibling of
  `Layout(TabsLayout)` on the root stack, so pushing it covers the whole
  `Layout` — content *and* bar disappear. "Tab bar visible?" is decided
  purely by tree position, not a flag.

`Tabs { … }` is exactly sugar for "a `Layout` with a **standard**
bottom-nav UI + a `Switch`". Reach for an explicit `Layout(X)` only when
you want custom chrome (top tabs, a segmented control, a styled bar); the
`Switch` underneath is unchanged either way.

## NavState (dynamic)

NavState is the RouteTree **instantiated with runtime state**. Only two
pieces of state exist:

| Node | State it carries | How "the active child" is chosen |
| --- | --- | --- |
| `Stack` | `history: [child, …]` | the **top** of `history` |
| `Switch` | `selected: branch` | `selected` |
| `Route` | none | (leaf) |

A `Stack`'s history entry may be a `Route` *instance* **or a whole
container subtree** (a `Switch`/`Stack`). This is the key that makes
"push a screen that lives outside the tabs" require no special case: the
tabs `Switch`, as one subtree, simply occupies one slot in the root
stack's history, and an outside `Route` occupies the next slot above it.

### `current` is derived, not stored

There is **no marker / current pointer**. The shown screen is *computed*
by walking from the root:

```
current = walk from root:
    at a Stack   → take history.top
    at a Switch  → take selected
    at a Route   → that's current
```

`current`, the "screen we'd go back to", and "is the tab bar visible" are
all **derived** from `history` + `selected`. Nothing else is the source
of truth. (In Whisker terms: `history`/`selected` are signals, `current`
is a `computed` — the idiomatic fine-grained shape, with no risk of a
stored pointer drifting out of sync.)

### Example: opening an outside route over the tabs

Starting in tab A on a post, `navigate(video)`:

```
rootStack:
  ├ [0] Switch(selected: A)          ← the whole tabs subtree, one slot
  │        ├ A: [Timeline, post]      ←   each branch keeps its own state
  │        └ B: [Search]
  └ [1] video   ← current            ← stacked above the tabs ⇒ no tab bar
```

`back()` pops `video`; `current` recomputes to `Switch(selected:A) →
A.top = post`. The tab bar reappears with tab A's post — derived purely
from the retained `selected`/`history`. The "which tab do we return to?"
question never arises because the `Switch` instance **retained
`selected: A`** when it was buried under `video`.

The only case where the return branch is undefined is a **never-visited
`Switch`** (e.g. a cold deep-link straight to `video`). That is resolved
by a declared default: `Switch(default: A) { … }`, falling back to the
first branch in declaration order.

## Resolution: which instance does a target hit?

When a target URL/route matches **multiple** NavState positions (e.g. the
shared `/post/:id` exists in every tab), the instance is chosen by
**relative resolution**:

> Among nodes matching the target, pick the one whose path shares the
> **deepest common ancestor with the current position**. Break ties by
> **declaration order** (first defined wins).

Operationally: walk up from the current screen; the first (deepest)
ancestor whose subtree contains a match resolves it; within that subtree,
declaration order breaks ties.

This single rule, derived only from graph shape + current position +
declaration order (no manual priorities), yields the intuitive
behaviours:

| Situation | Resolves to | Why |
| --- | --- | --- |
| From tab A's post, go to profile | tab A's profile | current tab's stack is the deepest common ancestor |
| From tab B, go to post | tab B's post | resolved within tab B's subtree |
| From outside (video), go to post | first-declared post | common ancestor is the root ⇒ declaration order |
| Cold deep-link `/post/42` | first-declared post | no current position ⇒ declaration order |

An explicit override (`within(scope)`) targets a specific branch
(`route::post(42).within(scope::search)` — the Expo `/(search)/post/42`
equivalent). This is the rare cross-tab case and is **deferred**; the
default relative rule covers the common ones.

## Operations

The runtime exposes five operations. Each is just a mutation of
`history`/`selected`; `current` recomputes afterward. (Named
`navigate`/`back` rather than `push`/`pop`: the operations act on the
whole graph — selecting `Switch` branches and pushing `Stack`s — so the
stack-only terms would mislead.)

| Op | Effect on NavState |
| --- | --- |
| `navigate(R)` | Along the path root→R: `Switch` → select toward R; `Stack` → **always push a new instance** of the toward-R child (never unwind to an existing one); a buried intermediate container is revealed (entries above it popped). |
| `back()` | Pop the top of the **deepest non-trivial `Stack`** on the active path. `Switch` selection is **not** on the back history. At a tab root with nothing to pop → no-op (deferred to a future `BackHandler`). |
| `replace(R)` | Swap the **top** of the current stack with R. **Same stack only.** |
| `popTo(R)` | Pop the current stack until R is the top. Same stack only. |
| `reset(R)` | Replace the **entire** current stack (or root) contents with `[R]`. The whole-stack-scope version of `replace`; used for auth/logout where the back stack must be cleared. |

Notes that pin down the corners:

- **`navigate` always pushes within a stack** (no dedup/unwind). It is
  *not* React Navigation's `navigate`. This keeps it predictable: a
  `navigate` always advances by one screen. Param-distinct screens
  (`post(1)` then `post(2)`) are different instances and stack normally.
- **`replace`/`popTo` are same-stack only.** A cross-`Switch` "replace"
  has no clean meaning (it would silently mutate another tab while
  switching), so it is disallowed — use `navigate` to cross branches.
- **`reset` = `replace` at stack/root scope.** `replace` swaps the top
  entry; `reset` swaps the whole history. Not a new primitive — the same
  idea at a wider scope. Justified because `navigate`/`back`/`replace`
  cannot clear a back stack (logout).
- **`back` only travels stack depth**, never `Switch` selection. "Back
  from a non-home tab returns to the home tab" is a product policy, not a
  graph primitive; it will be added later via a `BackHandler` that reads
  NavState and decides — kept out of the core pop rule so the graph stays
  clean.

## Modals

A modal is **a `Route`**, not a new concept: `Route("compose", Compose,
present: Modal)`. `present:` changes only the *presentation/animation*
(slide-up, swipe-to-dismiss), not the stack semantics. Placing modals as
direct children of the **root stack** (above the tabs `Switch`) matches
iOS, where a modal covers the whole window including the tab bar.

`dismissTo(href)` from Expo/React Navigation (dismiss the modal stack
until a target) is just **`popTo`** in this model, because a modal is an
ordinary stack route. No separate dismissal API is needed.

## Interactive transitions

Interactive, gesture-driven transitions (iOS swipe-back, modal
swipe-down, Android predictive back) have a **continuous intermediate
state** (finger progress 0..1, cancellable). That continuous state is
**out of NavState's scope** — NavState is discrete. The animation layer
reads `current` and the would-be back target from NavState and
interpolates between them.

The subtlety: a `Route`'s transition parameters define one screen's
enter/exit, but a gesture spans **two** routes. Resolution:

- **No separate "interactive animation" is defined.** A gesture *scrubs*
  the existing pop pair — `outgoing.pop_exit` + `incoming.pop_enter` —
  replacing time with finger progress (and allowing cancel/reverse).
  This is exactly how iOS interactive pop reuses the standard pop
  animation.
- **The runtime composes the pair** from the two routes' own transitions
  (read off NavState). The "spans two routes" problem is solved by
  computation, not by a new place to write a combined animation.
- **Gesture enablement** (which stacks allow swipe-back, edge, etc.) is a
  **`Stack`-level** option — "can you go back here" is a container
  concern.

So: animation *values* live on the `Route`; the *pairing* is derived by
the runtime; gesture *enablement* lives on the `Stack`; the intermediate
0..1 state lives only in the animation layer. A rare interactive-only
visual can override at the `Stack` level, but the default writes nothing
new.

## What `routes!` generates

1. **URL table** — concatenated paths from the tree (shared routes
   deduped).
2. **Typed nav targets** — `route::*(params).query(...).within(scope)`,
   so a missing route or wrong param type is a compile error.
3. **Branch identifiers** — an enum per `Switch` (`HomeTab::Home`, …) for
   typed `select`/`within`/`use_active_tab`.
4. **Outlet wiring** — container → child plumbing.
5. **Relative resolution** — the walk-up + declaration-order logic.
6. **Structure checks** — parent/child constraints (`Tab` only in
   `Switch`, `Route` only in `Stack`/`Drawer`) enforced at compile time.

## Drawer and overlays (not navigation)

`Drawer`, bottom sheets, and dialogs are **overlays, not screen
transitions**: they carry no back history and no route identity (a
`/drawer` URL is unnatural), and their state is `open`/`close`, not
push/pop. They are **not a separate concept** — an overlay is just what
the **outermost `Layout`** renders *around* its `Outlet`. (Drawer is a
persistent shell, exactly the job `Layout(X)` — the `_layout.tsx`
equivalent — exists for.)

```rust
#[layout]
#[component]
fn root_layout() -> Element {
    render! {
        Drawer(content: render!{ MyMenu {} }) {  // persistent overlay
            Outlet {}                             // the router's content
        }
    }
}
// routes!: Layout(RootLayout) { Stack { Switch { … } } }
// nav.toggle_drawer();   // open/close — not on the back stack
```

So a Drawer/sheet lives **inside `routes!` as the body of a `Layout`**,
not in a separate `AppShell` wrapper. The only thing that makes it "not
navigation" is that it has no NavState (no history, no route identity) —
it is shell chrome `open`/`close`d imperatively.

(Modals are the exception that *is* a route, because on iOS a modal is a
full screen and is deep-linkable — see **Modals**.)

## Prior art and positioning

This model is a re-derivation, in minimal form, of the
**navigation-state tree** that React Navigation uses (each navigator's
`{ index, routes }` ≙ `Switch.selected` / `Stack.history`), itself an
abstraction over UIKit's `UINavigationController` × `UITabBarController`
nesting and mirrored by Jetpack Navigation's nested graphs + multiple
back stacks, sharing Flutter Navigator 2.0 / go_router's
"stack-as-a-function-of-state" philosophy.

What is distinctive here: **three primitives + five operations + two
graphs**, with resolution and `current` *derived* from graph shape and
declaration order rather than configured; `Stack`/`Switch` unified as
depth/branch in one instance tree; and `current` as a `computed` over
`history`/`selected` (no stored marker), which fits Whisker's
fine-grained reactive runtime directly.

Known gaps the prior art has already solved and this design will grow
into: `Switch`-back history (the `BackHandler`), cold-start deep-link
stack synthesis, and the full interactive/predictive-back polish.

## Open items

- `within(scope)` explicit cross-branch targeting — deferred; default
  relative resolution covers the common cases.
- `BackHandler` for Android predictive back / "back to home tab" — reads
  NavState to decide; intentionally outside the core `back` rule.
- Cold deep-link: synthesising a sensible back stack when entering deep
  into a nested structure (use `Switch(default:)` + declaration order as
  the seed).
- Dynamic-segment notation (`":id"` string vs typed field) and the
  preset-vs-custom transition surface.
- Rendering substrate: implemented in-Lynx (single runtime, shared
  state) for now; the API is intended to keep the door open to a future
  native-container substrate (Lynx multi-surface) without changing
  `routes!`. See `docs/lynx-integration.md` for the multi-surface
  investigation.
