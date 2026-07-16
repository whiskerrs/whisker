//! Typed enums for built-in element attributes whose Lynx-side
//! contract is a closed set of strings.
//!
//! The element-builder setters take a typed enum directly —
//! raw strings no longer compile:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! scroll_view(scroll_orientation: ScrollOrientation::Vertical)
//! ```
//!
//! Reactivity still works the same way: anything implementing
//! `Into<Signal<EnumType>>` is accepted, so passing a
//! `RwSignal<ScrollOrientation>` flips the attribute live.
//!
//! ## Why typed enums
//!
//! Previously these props took `Signal<String>` and pumped whatever
//! string the caller wrote straight through `apply_attr` to Lynx.
//! A typo (`"verticle"`, `"List-Single"`, …) parsed fine on the Rust
//! side and was silently ignored on the Lynx side, so the failure
//! surfaced as a render glitch with no compile-time hint. Typed
//! enums turn those typos into compile errors.
//!
//! Each enum is `#[non_exhaustive]` so a future Lynx-side addition
//! (a new `pan-intercept-scope` keyword, an additional `list-type`,
//! …) can grow the enum without breaking match arms downstream.
//!
//! ## How the wire string is produced
//!
//! `apply_attr` is generic over `T: ToString`. Each enum implements
//! `Display` (via the `attr_enum!` macro), so `.to_string()` lands
//! the canonical Lynx wire literal at the bridge boundary without
//! any per-attribute conversion code.

// ---------------------------------------------------------------------------
// Macro: stamp out an enum + `as_str` + `Display`.
//
// Centralising the boilerplate keeps every attribute enum identical
// in shape (Copy, Clone, Debug, PartialEq, Eq, Hash, non_exhaustive)
// and removes the temptation to hand-roll the Display impl per
// variant. Adding a new attribute is a single `attr_enum!`
// invocation.
//
// The setter sends the enum through `apply_attr`'s `T: ToString`
// bound (which `Display` satisfies via the standard blanket impl),
// so no explicit `From<EnumType>` is needed — the wire string
// emerges naturally on each reactive read.
// ---------------------------------------------------------------------------

macro_rules! attr_enum {
    (
        $(#[$enum_attr:meta])*
        $name:ident { $( $(#[$variant_attr:meta])* $variant:ident => $literal:literal ),* $(,)? }
    ) => {
        $(#[$enum_attr])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$variant_attr])* $variant ),*
        }

        impl $name {
            /// Canonical wire string. This is exactly the value the
            /// Lynx side matches on; tests cross-check it against
            /// the native module's string dispatch table.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $literal ),*
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

// ---------------------------------------------------------------------------
// scroll-view
// ---------------------------------------------------------------------------

attr_enum! {
    /// Direction in which a `<scroll-view>` scrolls.
    ///
    /// Maps to Lynx's `scroll-orientation` attribute.
    ScrollOrientation {
        /// `"vertical"` — the default.
        Vertical => "vertical",
        /// `"horizontal"`.
        Horizontal => "horizontal",
    }
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

attr_enum! {
    /// Layout mode for a `<list>`.
    ///
    /// Maps to Lynx's `list-type` attribute.
    ListType {
        /// `"single"` — single-column linear list (the default).
        Single => "single",
        /// `"flow"` — grid-style flow layout.
        Flow => "flow",
        /// `"waterfall"` — staggered / Pinterest-style waterfall.
        Waterfall => "waterfall",
    }
}

attr_enum! {
    /// `<list>` data-update animation — maps to Lynx's
    /// `update-animation` attribute.
    ListUpdateAnimation {
        /// `"default"` — animate inserts / moves / removes.
        Default => "default",
        /// `"none"` — apply data updates without animation.
        None => "none",
    }
}

// ---------------------------------------------------------------------------
// pan-intercept-*
// ---------------------------------------------------------------------------

attr_enum! {
    /// Direction along which an element intercepts swipe gestures.
    ///
    /// Maps to Lynx's `pan-intercept-direction` attribute. Pair
    /// with [`PanInterceptScope`] to choose which elements in the
    /// hit-test chain the interception applies to.
    ///
    /// Sent as an **int** ([`Self::as_wire_int`]), not the
    /// `as_str`/`Display` string above — unlike most attribute enums
    /// in this file, Lynx's native prop setter for this one is
    /// integer-typed on both platforms (`LynxBaseUI.setPanInterceptDirection(int)`
    /// on Android, `setPanInterceptDirection(NSInteger)` on iOS via
    /// `LYNX_PROP_DEFINE`). Sending the string form is a **silent
    /// no-op** — confirmed on-device: the attribute never reached the
    /// native element at all, so the value stayed at its class
    /// default (`None`) regardless of what was set from Rust.
    PanInterceptDirection {
        /// `"horizontal"`.
        Horizontal => "horizontal",
        /// `"vertical"`.
        Vertical => "vertical",
        /// `"none"` — disable the intercept (default).
        None => "none",
    }
}

impl PanInterceptDirection {
    /// Wire ordinal — must match `LynxPanInterceptDirection` exactly
    /// (`LynxEventTarget.h` on iOS, `EventTarget.java` on Android;
    /// both declare the same order). See the type's own doc comment
    /// for why this (not `as_str`) is what actually reaches Lynx.
    pub const fn as_wire_int(self) -> i32 {
        match self {
            Self::Horizontal => 0,
            Self::Vertical => 1,
            Self::None => 2,
        }
    }
}

attr_enum! {
    /// Scope of [`PanInterceptDirection`] — which elements in the
    /// hit-test chain participate in the intercept.
    ///
    /// Maps to Lynx's `pan-intercept-scope` attribute. The variant
    /// names track the wire strings 1:1; `SelfElement` reads `"self"`
    /// (the variant rename dodges Rust's `Self` keyword without
    /// resorting to `r#Self`).
    ///
    /// Sent as an **int** ([`Self::as_wire_int`]) — see
    /// [`PanInterceptDirection`]'s doc comment; the same
    /// string-attribute-on-an-int-prop gap applies here.
    PanInterceptScope {
        /// `"self"` — intercept on this element only.
        SelfElement => "self",
        /// `"ancestors"`.
        Ancestors => "ancestors",
        /// `"descendants"`.
        Descendants => "descendants",
        /// `"self-and-ancestors"`.
        SelfAndAncestors => "self-and-ancestors",
        /// `"self-and-descendants"`.
        SelfAndDescendants => "self-and-descendants",
        /// `"all"`.
        All => "all",
        /// `"none"`.
        None => "none",
    }
}

impl PanInterceptScope {
    /// Wire ordinal — must match `LynxPanInterceptScope` exactly
    /// (`LynxEventTarget.h` on iOS, `EventTarget.java` on Android;
    /// both declare the same order). See [`PanInterceptDirection::as_wire_int`].
    pub const fn as_wire_int(self) -> i32 {
        match self {
            Self::SelfElement => 0,
            Self::Ancestors => 1,
            Self::Descendants => 2,
            Self::SelfAndAncestors => 3,
            Self::SelfAndDescendants => 4,
            Self::All => 5,
            Self::None => 6,
        }
    }
}

// ---------------------------------------------------------------------------
// text
// ---------------------------------------------------------------------------

attr_enum! {
    /// Vertical alignment for a single-line `<text>` element.
    ///
    /// Maps to Lynx's `text-single-line-vertical-align` attribute.
    TextVerticalAlign {
        /// `"normal"` — the platform default (baseline-aligned).
        Normal => "normal",
        /// `"top"`.
        Top => "top",
        /// `"center"`.
        Center => "center",
        /// `"bottom"`.
        Bottom => "bottom",
    }
}

// ---------------------------------------------------------------------------
// accessibility
// ---------------------------------------------------------------------------

attr_enum! {
    /// Accessibility role advertised to platform a11y services
    /// (VoiceOver on iOS, TalkBack on Android).
    ///
    /// Maps to Lynx's `accessibility-trait` attribute.
    AccessibilityTrait {
        /// `"button"` — a tap target. Reads as "Button" on iOS.
        Button => "button",
        /// `"image"` — picture / icon, no inherent action.
        Image => "image",
        /// `"text"` — block of static text.
        Text => "text",
        /// `"none"` — the platform default.
        None => "none",
    }
}

// ---------------------------------------------------------------------------
// Tests — the wire strings are part of the public contract; lock them
// in so a typo in the enum or a Lynx-side rename surfaces here.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_orientation_wire_strings() {
        assert_eq!(ScrollOrientation::Vertical.as_str(), "vertical");
        assert_eq!(ScrollOrientation::Horizontal.as_str(), "horizontal");
    }

    #[test]
    fn list_type_wire_strings() {
        assert_eq!(ListType::Single.as_str(), "single");
        assert_eq!(ListType::Flow.as_str(), "flow");
        assert_eq!(ListType::Waterfall.as_str(), "waterfall");
    }

    #[test]
    fn pan_intercept_direction_wire_strings() {
        assert_eq!(PanInterceptDirection::Horizontal.as_str(), "horizontal");
        assert_eq!(PanInterceptDirection::Vertical.as_str(), "vertical");
        assert_eq!(PanInterceptDirection::None.as_str(), "none");
    }

    #[test]
    fn pan_intercept_scope_wire_strings() {
        // The kebab-case wire strings must match the Lynx dispatch
        // table on iOS (LynxPanInterceptScope) verbatim — a rename
        // here means breakage on device.
        assert_eq!(PanInterceptScope::SelfElement.as_str(), "self");
        assert_eq!(PanInterceptScope::Ancestors.as_str(), "ancestors");
        assert_eq!(PanInterceptScope::Descendants.as_str(), "descendants");
        assert_eq!(
            PanInterceptScope::SelfAndAncestors.as_str(),
            "self-and-ancestors"
        );
        assert_eq!(
            PanInterceptScope::SelfAndDescendants.as_str(),
            "self-and-descendants"
        );
        assert_eq!(PanInterceptScope::All.as_str(), "all");
        assert_eq!(PanInterceptScope::None.as_str(), "none");
    }

    #[test]
    fn pan_intercept_direction_wire_ints() {
        // Must match `LynxPanInterceptDirection`'s declared order
        // exactly (`LynxEventTarget.h` on iOS, `EventTarget.java` on
        // Android) — this is what actually reaches Lynx; the `as_str`
        // form above is a silent no-op on-device (int-typed native
        // prop setter on both platforms).
        assert_eq!(PanInterceptDirection::Horizontal.as_wire_int(), 0);
        assert_eq!(PanInterceptDirection::Vertical.as_wire_int(), 1);
        assert_eq!(PanInterceptDirection::None.as_wire_int(), 2);
    }

    #[test]
    fn pan_intercept_scope_wire_ints() {
        assert_eq!(PanInterceptScope::SelfElement.as_wire_int(), 0);
        assert_eq!(PanInterceptScope::Ancestors.as_wire_int(), 1);
        assert_eq!(PanInterceptScope::Descendants.as_wire_int(), 2);
        assert_eq!(PanInterceptScope::SelfAndAncestors.as_wire_int(), 3);
        assert_eq!(PanInterceptScope::SelfAndDescendants.as_wire_int(), 4);
        assert_eq!(PanInterceptScope::All.as_wire_int(), 5);
        assert_eq!(PanInterceptScope::None.as_wire_int(), 6);
    }

    #[test]
    fn text_vertical_align_wire_strings() {
        assert_eq!(TextVerticalAlign::Normal.as_str(), "normal");
        assert_eq!(TextVerticalAlign::Top.as_str(), "top");
        assert_eq!(TextVerticalAlign::Center.as_str(), "center");
        assert_eq!(TextVerticalAlign::Bottom.as_str(), "bottom");
    }

    #[test]
    fn accessibility_trait_wire_strings() {
        assert_eq!(AccessibilityTrait::Button.as_str(), "button");
        assert_eq!(AccessibilityTrait::Image.as_str(), "image");
        assert_eq!(AccessibilityTrait::Text.as_str(), "text");
        assert_eq!(AccessibilityTrait::None.as_str(), "none");
    }

    #[test]
    fn display_is_wire_string() {
        // `Display` is the standard "render as the canonical
        // string" path — should never diverge from `as_str`.
        assert_eq!(ScrollOrientation::Vertical.to_string(), "vertical");
        assert_eq!(PanInterceptScope::SelfElement.to_string(), "self");
    }
}
