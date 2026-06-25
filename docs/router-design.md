# Router Design

Whisker's router is built on **two graphs**: a static **RouteTree** that
the `routes!` macro produces at compile time, and a dynamic **RouteState**
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

> Status: **implemented** in `packages/whisker-router`. The model below
> matches the current code.

## Why two graphs

Modern declarative routers (React Navigation's navigation-state tree,
Flutter Navigator 2.0's "stack as a function of state") all converge on
the same separation: a **static description** of the app's screen
structure, and a **runtime state** that drives what's on screen. Whisker
adopts this explicitly and pushes it to its minimal form:

- **RouteTree** — *what screens exist and how they nest.* Immutable,
  compile-time, produced by `routes!`. Determines URLs, the set of legal
  targets, the resolution rule, and per-screen animations.
- **RouteState** — *what is live right now and how we got here.* Mutable,
  runtime, the RouteTree instantiated with state. Determines the current
  screen, where a push lands, and where a back returns.

The whole navigation domain closes over these two graphs and two ideas:
a **relative resolution rule** (which instance of an ambiguous route to
target) and a **derived `current`** (the shown screen is computed, never
stored).

### Naming: structure is *route*, the act of moving is *navigate*

To keep "routing" and "navigation" from blurring, the names split by part
of speech:

- **Nouns (the structure / state) use *route*:** `RouteTree` (static
  definition), `RouteState` (runtime state), and the node types `Route` /
  `Stack` / `Switch`.
- **Verbs (the act of moving) use *navigate* and friends:** the
  operations `navigate` / `select` / `back` / `replace` / `pop_to` /
  `reset`, and the handle you call them on, the `RouterHandle`
  (obtained via `use_navigator()`).

So: the *route* graph is the thing; *navigating* is what you do to it. No
type or field name mixes the two.

## RouteTree (static)

The `routes!` macro lowers a nested block into a tree of three node
kinds:

| Node | Role | Children |
| --- | --- | --- |
| `Route` | A screen (leaf) or a layout (with children) | optional |
| `Stack` | **Ordered** container: push/pop, has history | `Route` (+ `..spread`) |
| `Switch` | **Parallel** container: keeps all children alive, selects one, no history | containers (one per branch) |

A `Route` with both a `component` and children is a **layout route**
(the Expo `_layout.tsx` equivalent): its component renders with an
`Outlet` for the active child. A `Route` with a `path` but no component
is a **group route** (structural only, like Expo's `(group)` folders).
`Stack`, `Switch` and `Route` are the only primitives.

### URL derivation

A `Route`'s URL is the concatenation of **all segments** along the path
from the root — including group segments. Containers (`Stack`/`Switch`)
are pathless and contribute nothing; `Route` nodes always contribute
their segment.

**Group segments** `(name)` appear in the canonical URL but are
**optional during matching**. This means:

- The Home route's URL is `/(home)` (the group segment is included).
- `navigate("/detail/42")` still matches `/(home)/detail/:id` because
  `(home)` is skipped when absent from the input.
- `select("/(home)")` explicitly targets the group route for tab
  switching.

Group segments are "parenthesised paths that can be ignored when the URL
doesn't include them" — they organise the tree without forcing every URL
to spell them out.

```rust
let content = routes! {            // a reusable sub-route set is just a value
    Route(path: "post/:id",    component: Post)
    Route(path: "profile/:id", component: Profile)
};

let app = routes! {
    Route(component: TabsLayout) {                          // layout: tab bar chrome + Outlet
        Switch {
            Route(path: "(home)") {                         // group — URL: /(home)
                Stack {
                    Route(path: "", component: Timeline)    //   URL: /(home)  (= /)
                    ..content                               //   /post/:id, /profile/:id
                }
            }
            Route(path: "(search)") {                       // group — URL: /(search)
                Stack {
                    Route(path: "search", component: Search) // URL: /(search)/search
                    ..content
                }
            }
        }
    }
    Route(path: "video/:id",  component: VideoPlayer)       // /video/:id  (outside tabs)
    Route(path: "compose",    component: Compose)            // /compose
};
```

- The tabs are a `Switch` inside a `Route(component: TabsLayout)` layout;
  each tab is its own `Stack` (independent history). `..content` spreads
  the shared sub-routes into each tab.
- `Route(path: "(home)")` / `Route(path: "(search)")` are **group routes**
  (expo-router's `(group)` folders). They **do** appear in the derived
  URL — `/(home)`, `/(search)` — but are **optional during matching**: a
  `navigate("/post/42")` matches `/(home)/post/:id` because group
  segments are skipped when they don't appear in the input URL.
- `video`/`compose` sit *above* the tabs (no tab bar) — purely a
  consequence of where they are in the tree.

### Route nesting ≠ URL nesting

Nesting a `Route` inside another `Route` creates a **layout
relationship**, not just a URL prefix. The parent becomes a layout route
whose component must render an `Outlet` for the child to appear. The
child is NOT pushed onto a `Stack` — it lives inside the parent's
subtree and is always present in the state.

```rust
// ✗ Wrong — Detail is a child of Home (layout relationship).
//   navigate("/detail/1") modifies the child state in-place;
//   Home must render Outlet for Detail to appear; back() does not pop.
Stack {
    Route(path: "", component: Home) {
        Route(path: "detail/:id", component: Detail)
    }
}

// ✓ Correct — Home and Detail are siblings in the Stack.
//   navigate("/detail/1") pushes Detail; back() pops to Home.
Stack {
    Route(path: "", component: Home)
    Route(path: "detail/:id", component: Detail)
}
```

To share a URL prefix among push/pop screens, spell the full path on
each sibling — do not nest `Route` nodes:

```rust
Stack {
    Route(path: "settings", component: Settings)            // /settings
    Route(path: "settings/account", component: Account)     // /settings/account
    Route(path: "settings/privacy", component: Privacy)     // /settings/privacy
}
```

Reserve `Route` nesting for layout routes (tab bar, header chrome, etc.)
where the parent renders shared UI around an `Outlet`.

### Shared routes are just a spread

`content` above is an ordinary `routes!` value spread with `..content`
into each tab's stack. There is no special "shared route" construct —
spreading the same group into N stacks creates N instances of
`/post/:id` that **share a route id** (`"post"`). Relative resolution
(see below) picks the right instance at navigate time.

### Per-screen animation

Transitions are **parameters of the `Route` in `routes!`**, not
attributes on the component. A `Transition` trait value poses all four
directional slots (enter, exit, pop-enter, pop-exit) from a single
`pose(ctx)` method:

```rust
routes! {
    Stack {
        Route(path: "", component: Timeline)
        Route(path: "post/:id", component: Post,
              transition: RouteTransition::slide())     // platform-aware slide
        Route(path: "compose", component: Compose,
              transition: RouteTransition::modal())     // slide-up / slide-down
    }
}
```

The transition lives on the **`Route`, not the `#[component]`**, because
an animation is "how this *route* enters/leaves", not a property of the
UI itself. This matters for shared routes: the same `Post` component
spread into several stacks can be given a different transition at each
`Route` site. The component stays pure UI; `transition` is a route
parameter.

Containers (`Stack`/`Switch`) own *no* animation — they are passive views
that render whichever child the RouteState selects. (See **Interactive
transitions** for the one nuance.)

### Chrome (tab bar) is a layout `Route`, not the `Switch`

A `Switch` is **navigation logic only** — it decides which branch is
selected and renders it into an `Outlet`. It draws **no UI**: no tab bar,
no chrome. The bottom navigation (or top tabs, segmented control,
whatever) is drawn by the **layout route** (`Route(component: X)` with
children) that wraps the `Switch`. This keeps transition (the `Switch`)
and chrome (the layout) orthogonal.

The layout component renders the content area and the bar as an ordinary
flex column — `Outlet` (content, `flex_grow: 1`) above a fixed-height
bar. Tab switching uses `navigator.select("/(group-name)")`:

```rust
#[component]
fn tabs_layout() -> Element {
    let nav = use_navigator();
    let pathname = use_pathname();        // reactive current URL
    render! {
        view(style: css!(flex_grow: 1.0, display: Display::Flex,
                         flex_direction: FlexDirection::Column)) {
            view(style: css!(flex_grow: 1.0)) { Outlet {} }
            view(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row,
                             height: px(56))) {
                // select() with the group URL switches the tab
                view(on_tap: move |_| { let _ = nav.select("/(home)"); }) {
                    text(value: "Home")
                }
                view(on_tap: move |_| { let _ = nav.select("/(search)"); }) {
                    text(value: "Search")
                }
            }
        }
    }
}
```

```rust
routes! {
    Route(component: TabsLayout) {              // layout: chrome + Outlet
        Switch {
            Route(path: "(home)") {             // group → URL: /(home)
                Stack {
                    Route(path: "", component: Timeline)    ..content
                }
            }
            Route(path: "(search)") {           // group → URL: /(search)
                Stack {
                    Route(path: "search", component: Search) ..content
                }
            }
        }
    }
    Route(path: "video/:id", component: VideoPlayer)   // outside ⇒ no tab bar
}
```

Two properties fall out of this structure:

- **The bar is persistent.** The layout route is the *parent* of the
  `Switch`, so it stays mounted while the `Switch` swaps branch content —
  the bottom nav is not re-rendered on tab change.
- **"Outside the tabs" = outside the layout.** `video` is a sibling of
  the layout route on the root stack, so pushing it covers the whole
  layout — content *and* bar disappear. "Tab bar visible?" is decided
  purely by tree position, not a flag.

## RouteState (dynamic)

RouteState is the RouteTree **instantiated with runtime state**. Only two
pieces of state exist:

| Node | State it carries | How "the active child" is chosen |
| --- | --- | --- |
| `Stack` | `history: [child, …]` | the **top** of `history` |
| `Switch` | `selected: branch` | `selected` |
| `Route` | `params`, optional `children` | traverse children if present |

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

Starting in tab A on a post, `navigate("/video/1")`:

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

When a URL matches **multiple** RouteState positions (e.g. the shared
`/post/:id` exists in every tab), the instance is chosen by **relative
resolution**:

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
| From tab A, `navigate("/post/42")` | tab A's `/post/:id` | current tab's stack is the deepest common ancestor |
| From tab B, `navigate("/post/42")` | tab B's `/post/:id` | resolved within tab B's subtree |
| From outside (video), `navigate("/post/42")` | first-declared `/post/:id` | common ancestor is the root ⇒ declaration order |
| Cold deep-link `/post/42` | first-declared `/post/:id` | no current position ⇒ declaration order |

An explicit override (`within(scope)`) targets a specific branch.
This is the rare cross-tab case and is **deferred**; the default relative
rule covers the common ones.

## Operations

The runtime exposes six operations on the `RouterHandle`. **All
navigation targets are plain `&str` URLs** — there is no `Target` enum
or typed route constructors. Dynamic `:param` segments are extracted
automatically by matching the URL against the route patterns in the tree.

```rust
let nav = use_navigator();
nav.navigate("/detail/42");        // push — :id binds to "42"
nav.select("/(search)");           // switch tab
nav.replace("/detail/99");         // swap top
nav.back();                        // pop deepest stack
nav.pop_to("/");                   // pop to target
nav.reset("/");                    // clear stack
```

Each operation is just a mutation of `history`/`selected`; `current`
recomputes afterward. (Named `navigate`/`back` rather than `push`/`pop`:
the operations act on the whole graph — selecting `Switch` branches and
pushing `Stack`s — so the stack-only terms would mislead.)

| Op | Effect on RouteState |
| --- | --- |
| `navigate(url)` | Along the path root→target: `Switch` → select toward target; `Stack` → **always push a new instance** of the toward-target child (never unwind to an existing one); a buried intermediate container is revealed (entries above it popped). |
| `select(url)` | Select the `Switch` branch containing the target without pushing onto any stack. Used for tab switching: `select("/(home)")`. |
| `back()` | Pop the top of the **deepest non-trivial `Stack`** on the active path. `Switch` selection is **not** on the back history. At a tab root with nothing to pop → `Err(NothingToPop)`. |
| `replace(url)` | Swap the **top** of the current stack with the target. **Same stack only.** |
| `pop_to(url)` | Pop the current stack until the target is the top. Same stack only. |
| `reset(url)` | Replace the **entire** current stack contents with `[target]`. The whole-stack-scope version of `replace`; used for auth/logout where the back stack must be cleared. |

Notes that pin down the corners:

- **`navigate` always pushes within a stack** (no dedup/unwind). It is
  *not* React Navigation's `navigate`. This keeps it predictable: a
  `navigate` always advances by one screen. Param-distinct screens
  (`/post/1` then `/post/2`) are different instances and stack normally.
- **`replace`/`pop_to` are same-stack only.** A cross-`Switch` "replace"
  has no clean meaning (it would silently mutate another tab while
  switching), so it is disallowed — use `navigate` to cross branches.
- **`reset` = `replace` at stack/root scope.** `replace` swaps the top
  entry; `reset` swaps the whole history. Not a new primitive — the same
  idea at a wider scope. Justified because `navigate`/`back`/`replace`
  cannot clear a back stack (logout).
- **`back` only travels stack depth**, never `Switch` selection. "Back
  from a non-home tab returns to the home tab" is a product policy, not a
  graph primitive; it will be added later via a `BackHandler` that reads
  RouteState and decides — kept out of the core pop rule so the graph stays
  clean.

## Modals

A modal is **a `Route`**, not a new concept:
`Route(path: "compose", component: Compose, transition: RouteTransition::modal())`.
The transition changes only the *presentation/animation* (slide-up,
swipe-to-dismiss), not the stack semantics. Placing modals as direct
children of the **root stack** (above the tabs layout) matches iOS, where
a modal covers the whole window including the tab bar.

`dismissTo(href)` from Expo/React Navigation (dismiss the modal stack
until a target) is just **`pop_to`** in this model, because a modal is an
ordinary stack route. No separate dismissal API is needed.

## Interactive transitions

Interactive, gesture-driven transitions (iOS swipe-back, modal
swipe-down, Android predictive back) have a **continuous intermediate
state** (finger progress 0..1, cancellable). That continuous state is
**out of RouteState's scope** — RouteState is discrete. The animation layer
reads `current` and the would-be back target from RouteState and
interpolates between them.

The subtlety: a `Route`'s transition parameters define one screen's
enter/exit, but a gesture spans **two** routes. Resolution:

- **No separate "interactive animation" is defined.** A gesture *scrubs*
  the existing pop pair — `outgoing.pop_exit` + `incoming.pop_enter` —
  replacing time with finger progress (and allowing cancel/reverse).
  This is exactly how iOS interactive pop reuses the standard pop
  animation.
- **The runtime composes the pair** from the two routes' own transitions
  (read off RouteState). The "spans two routes" problem is solved by
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

1. **A `CompiledTree`** — the `RouteTree` with pre-computed URLs, node
   paths, and parent links. Group segments `(name)` are included in
   derived URLs and flagged with `is_group: true` for optional matching.
2. **A `RouteRegistry`** — the id → render-function + transition map.
3. **A `LayoutRegistry`** — layout routes (those with both a component
   and children) registered for `Outlet` wiring.
4. **Structure checks** — parent/child constraints (`Route` branches of a
   `Switch` must have children, etc.) enforced at compile time.
5. **RA integration** — keyword anchors for rust-analyzer go-to-definition
   and completion on `Stack`, `Switch`, `Route` keywords.

## Drawer and overlays (not navigation)

`Drawer`, bottom sheets, and dialogs are **overlays, not screen
transitions**: they carry no back history and no route identity (a
`/drawer` URL is unnatural), and their state is `open`/`close`, not
push/pop. They are **not a separate concept** — an overlay is just what
the **outermost `Layout`** renders *around* its `Outlet`. (Drawer is a
persistent shell, exactly the job `Layout(X)` — the `_layout.tsx`
equivalent — exists for.)

```rust
#[component]
fn root_layout() -> Element {
    render! {
        Drawer(content: render!{ MyMenu {} }) {  // persistent overlay
            Outlet {}                             // the router's content
        }
    }
}
// routes!: Route(component: RootLayout) { Stack { Switch { … } } }
// navigator.toggle_drawer();   // open/close — not on the back stack
```

So a Drawer/sheet lives **inside `routes!` as the body of a layout
route**, not in a separate `AppShell` wrapper. The only thing that makes
it "not navigation" is that it has no RouteState (no history, no route
identity) — it is shell chrome `open`/`close`d imperatively.

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

What is distinctive here: **three primitives + six operations + two
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
- Cold deep-link: synthesising a sensible back stack when entering deep
  into a nested structure (use `Switch(default:)` + declaration order as
  the seed).
- Rendering substrate: implemented in-Lynx (single runtime, shared
  state) for now; the API is intended to keep the door open to a future
  native-container substrate (Lynx multi-surface) without changing
  `routes!`. See `docs/lynx-integration.md` for the multi-surface
  investigation.
