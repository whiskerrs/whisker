//! Lynx-only `relative-*` properties for the `display: relative`
//! layout container.
//!
//! `relative-id` identifies children; the other properties refer to
//! sibling `relative-id`s to anchor a child to a sibling edge.

use crate::css::Css;

impl Css {
    /// Sets `relative-id` — identifies the element for sibling-edge
    /// references.
    /// <https://lynxjs.org/api/css/properties/relative-id>
    pub fn relative_id(self, v: i32) -> Self {
        self.push_raw("relative-id", v.to_string())
    }

    /// Sets `relative-align-top` — id of the sibling to top-align with.
    /// <https://lynxjs.org/api/css/properties/relative-align-top>
    pub fn relative_align_top(self, v: i32) -> Self {
        self.push_raw("relative-align-top", v.to_string())
    }

    /// Sets `relative-align-right` — id of the sibling to right-align with.
    /// <https://lynxjs.org/api/css/properties/relative-align-right>
    pub fn relative_align_right(self, v: i32) -> Self {
        self.push_raw("relative-align-right", v.to_string())
    }

    /// Sets `relative-align-bottom`.
    /// <https://lynxjs.org/api/css/properties/relative-align-bottom>
    pub fn relative_align_bottom(self, v: i32) -> Self {
        self.push_raw("relative-align-bottom", v.to_string())
    }

    /// Sets `relative-align-left`.
    /// <https://lynxjs.org/api/css/properties/relative-align-left>
    pub fn relative_align_left(self, v: i32) -> Self {
        self.push_raw("relative-align-left", v.to_string())
    }

    /// Sets `relative-top-of` — id of the sibling this element sits below.
    /// <https://lynxjs.org/api/css/properties/relative-top-of>
    pub fn relative_top_of(self, v: i32) -> Self {
        self.push_raw("relative-top-of", v.to_string())
    }

    /// Sets `relative-right-of` — id of the sibling this element sits to the right of.
    /// <https://lynxjs.org/api/css/properties/relative-right-of>
    pub fn relative_right_of(self, v: i32) -> Self {
        self.push_raw("relative-right-of", v.to_string())
    }

    /// Sets `relative-bottom-of`.
    /// <https://lynxjs.org/api/css/properties/relative-bottom-of>
    pub fn relative_bottom_of(self, v: i32) -> Self {
        self.push_raw("relative-bottom-of", v.to_string())
    }

    /// Sets `relative-left-of`.
    /// <https://lynxjs.org/api/css/properties/relative-left-of>
    pub fn relative_left_of(self, v: i32) -> Self {
        self.push_raw("relative-left-of", v.to_string())
    }

    /// Sets `relative-center` — centers the element relative to a sibling.
    /// <https://lynxjs.org/api/css/properties/relative-center>
    pub fn relative_center(self, v: i32) -> Self {
        self.push_raw("relative-center", v.to_string())
    }

    /// Sets `relative-center-horizontal` — centers horizontally only.
    /// <https://lynxjs.org/api/css/properties/relative-center-horizontal>
    pub fn relative_center_horizontal(self, v: i32) -> Self {
        self.push_raw("relative-center-horizontal", v.to_string())
    }

    /// Sets `relative-center-vertical` — centers vertically only.
    /// <https://lynxjs.org/api/css/properties/relative-center-vertical>
    pub fn relative_center_vertical(self, v: i32) -> Self {
        self.push_raw("relative-center-vertical", v.to_string())
    }

    /// Sets `relative-layout-once` — performs the relative-layout
    /// pass only on the first render.
    /// <https://lynxjs.org/api/css/properties/relative-layout-once>
    pub fn relative_layout_once(self, v: bool) -> Self {
        self.push_raw("relative-layout-once", if v { "true" } else { "false" })
    }

    /// Sets `relative-align-inline-start` (logical-direction alias).
    pub fn relative_align_inline_start(self, v: i32) -> Self {
        self.push_raw("relative-align-inline-start", v.to_string())
    }

    /// Sets `relative-align-inline-end` (logical-direction alias).
    pub fn relative_align_inline_end(self, v: i32) -> Self {
        self.push_raw("relative-align-inline-end", v.to_string())
    }

    /// Sets `relative-inline-start-of`.
    pub fn relative_inline_start_of(self, v: i32) -> Self {
        self.push_raw("relative-inline-start-of", v.to_string())
    }

    /// Sets `relative-inline-end-of`.
    pub fn relative_inline_end_of(self, v: i32) -> Self {
        self.push_raw("relative-inline-end-of", v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;

    #[test]
    fn relative_id_and_anchors() {
        let s = Css::new()
            .relative_id(1)
            .relative_align_top(2)
            .relative_left_of(3)
            .relative_center(4);
        assert_eq!(
            s.to_string(),
            "relative-id: 1; relative-align-top: 2; relative-left-of: 3; relative-center: 4;"
        );
    }

    #[test]
    fn relative_all_align_edges() {
        let s = Css::new()
            .relative_align_top(1)
            .relative_align_right(2)
            .relative_align_bottom(3)
            .relative_align_left(4);
        assert_eq!(
            s.to_string(),
            "relative-align-top: 1; relative-align-right: 2; relative-align-bottom: 3; relative-align-left: 4;"
        );
    }

    #[test]
    fn relative_all_of_edges() {
        let s = Css::new()
            .relative_top_of(1)
            .relative_right_of(2)
            .relative_bottom_of(3)
            .relative_left_of(4);
        assert_eq!(
            s.to_string(),
            "relative-top-of: 1; relative-right-of: 2; relative-bottom-of: 3; relative-left-of: 4;"
        );
    }

    #[test]
    fn relative_center_variants() {
        let s = Css::new()
            .relative_center(1)
            .relative_center_horizontal(2)
            .relative_center_vertical(3);
        assert_eq!(
            s.to_string(),
            "relative-center: 1; relative-center-horizontal: 2; relative-center-vertical: 3;"
        );
    }

    #[test]
    fn relative_layout_once_bool() {
        assert_eq!(
            Css::new().relative_layout_once(true).to_string(),
            "relative-layout-once: true;"
        );
        assert_eq!(
            Css::new().relative_layout_once(false).to_string(),
            "relative-layout-once: false;"
        );
    }

    #[test]
    fn relative_inline_aliases() {
        let s = Css::new()
            .relative_align_inline_start(1)
            .relative_align_inline_end(2)
            .relative_inline_start_of(3)
            .relative_inline_end_of(4);
        assert_eq!(
            s.to_string(),
            "relative-align-inline-start: 1; relative-align-inline-end: 2; relative-inline-start-of: 3; relative-inline-end-of: 4;"
        );
    }
}
