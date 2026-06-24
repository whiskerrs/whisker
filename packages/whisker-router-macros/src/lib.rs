//! Procedural macros for `whisker-router`.
//!
//! Exports the declarative [`routes!`](macro@routes) macro, re-exported by
//! `whisker-router` as `whisker_router::routes`.

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod routes;

/// Lower a declarative route tree into a
/// [`RouteSet`](https://docs.rs/whisker-router/latest/whisker_router/render/struct.RouteSet.html)
/// (a compiled tree + its id → component registry), the value a
/// `RouterHandle::new(routes! { … })` consumes.
///
/// ```ignore
/// let handle = RouterHandle::new(routes! {
///     Switch {
///         Stack { Route("", Home)       Route("detail/:id", Detail) }
///         Stack { Route("list", List)   Route("detail/:id", Detail) }
///     }
/// });
/// ```
///
/// A `Route`'s id is its component name in snake_case (`Detail` → `detail`);
/// the same component routed in several stacks is one shared registry entry.
/// Components read their own `:param`s via `use_param`. The full grammar
/// (`Layout`, `transition =`, `..spread`, structure checks) lives in the
/// `routes` module.
#[proc_macro]
pub fn routes(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as routes::Routes);
    routes::expand(parsed).into()
}
