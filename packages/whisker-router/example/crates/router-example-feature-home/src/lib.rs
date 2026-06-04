//! Home screen used by the `StackLayout` crash repro.
//!
//! The body intentionally exercises the smallest pattern that
//! triggers the bug:
//!
//! 1. `safe_area_insets()` — allocates the global module-backed
//!    signal on first call. Inside `StackLayout`'s per-route owner
//!    that first call lands in a soon-to-be-disposed scope.
//! 2. `computed(move || ... insets.get() ...)` — registers a
//!    subscription to that signal. The `computed`'s `f()` runs
//!    inline at construction, before the surrounding effect's flush
//!    settles.
//!
//! Drop step 2 (and keep step 1) and the crash disappears; drop
//! the `StackLayout` wrapper in the shell and the crash also
//! disappears. See the cross-product in the shell's lib.rs.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_safe_area::safe_area_insets;

#[component]
pub fn home_screen() -> Element {
    let insets = safe_area_insets();
    let wrapper_style = computed(move || {
        format!(
            "display: flex; flex-direction: column; padding-top: {}px; \
             padding-left: 40px; padding-right: 40px; \
             background-color: #fef3c7; width: 100%; height: 100%;",
            insets.get().top as f32 + 16.0
        )
    });

    render! {
        view(style: wrapper_style) {
            text(style: "font-size: 24px;", value: "Home (cross-crate)".to_string())
        }
    }
}
