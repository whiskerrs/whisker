//! Visual-effect properties: opacity, visibility, overflow, shadow,
//! filter, cursor, pointer-events, clip-path.

use crate::data_type::{Color, Length, LengthPercentage};
use crate::keyword::{Cursor, Overflow, PointerEvents, Visibility};
use crate::css::Css;

impl Css {
    /// Sets `opacity`. Lynx clamps to `0.0..=1.0`. Default: `1`.
    /// <https://lynxjs.org/api/css/properties/opacity>
    pub fn opacity(self, v: f32) -> Self {
        self.push_raw("opacity", crate::to_css::number_to_string(v))
    }

    /// Sets `visibility`. Lynx default: `visible`. `collapse` is not
    /// supported.
    /// <https://lynxjs.org/api/css/properties/visibility>
    pub fn visibility(self, v: Visibility) -> Self {
        self.push("visibility", v)
    }

    /// Sets `overflow`. Lynx accepts only `visible` and `hidden`.
    /// <https://lynxjs.org/api/css/properties/overflow>
    pub fn overflow(self, v: Overflow) -> Self {
        let v = v;
        self.push("overflow-x", v).push("overflow-y", v)
    }

    /// Sets `overflow-x`.
    /// <https://lynxjs.org/api/css/properties/overflow-x>
    pub fn overflow_x(self, v: Overflow) -> Self {
        self.push("overflow-x", v)
    }

    /// Sets `overflow-y`.
    /// <https://lynxjs.org/api/css/properties/overflow-y>
    pub fn overflow_y(self, v: Overflow) -> Self {
        self.push("overflow-y", v)
    }

    /// Sets `cursor`.
    /// <https://lynxjs.org/api/css/properties/cursor>
    pub fn cursor(self, v: Cursor) -> Self {
        self.push("cursor", v)
    }

    /// Sets `pointer-events`.
    /// <https://lynxjs.org/api/css/properties/pointer-events>
    pub fn pointer_events(self, v: PointerEvents) -> Self {
        self.push("pointer-events", v)
    }

    /// Sets `box-shadow` to a single shadow. Pass `None` for inset
    /// to get an outer shadow.
    /// <https://lynxjs.org/api/css/properties/box-shadow>
    pub fn box_shadow(
        self,
        offset_x: Length,
        offset_y: Length,
        blur_radius: Length,
        spread_radius: Length,
        color: Color,
    ) -> Self {
        use crate::to_css::ToCss;
        let mut s = String::new();
        let _ = offset_x.to_css(&mut s);
        s.push(' ');
        let _ = offset_y.to_css(&mut s);
        s.push(' ');
        let _ = blur_radius.to_css(&mut s);
        s.push(' ');
        let _ = spread_radius.to_css(&mut s);
        s.push(' ');
        let _ = color.to_css(&mut s);
        self.push_raw("box-shadow", s)
    }

    /// Sets an inset `box-shadow`.
    /// <https://lynxjs.org/api/css/properties/box-shadow>
    pub fn box_shadow_inset(
        self,
        offset_x: Length,
        offset_y: Length,
        blur_radius: Length,
        spread_radius: Length,
        color: Color,
    ) -> Self {
        use crate::to_css::ToCss;
        let mut s = String::from("inset ");
        let _ = offset_x.to_css(&mut s);
        s.push(' ');
        let _ = offset_y.to_css(&mut s);
        s.push(' ');
        let _ = blur_radius.to_css(&mut s);
        s.push(' ');
        let _ = spread_radius.to_css(&mut s);
        s.push(' ');
        let _ = color.to_css(&mut s);
        self.push_raw("box-shadow", s)
    }

    /// Sets `filter` to a raw CSS filter list. Use raw because the
    /// `<filter-function>` grammar (blur, drop-shadow, etc.) is rich
    /// and rarely worth typing.
    /// <https://lynxjs.org/api/css/properties/filter>
    pub fn filter(self, value: impl Into<String>) -> Self {
        self.push_raw("filter", value)
    }

    /// Sets `mask-image` to a raw CSS value (URL or gradient).
    /// <https://lynxjs.org/api/css/properties/mask-image>
    pub fn mask_image(self, value: impl Into<String>) -> Self {
        self.push_raw("mask-image", value)
    }

    /// Sets `clip-path` to a raw CSS value.
    /// <https://lynxjs.org/api/css/properties/clip-path>
    pub fn clip_path(self, value: impl Into<String>) -> Self {
        self.push_raw("clip-path", value)
    }

    /// Sets `caret-color`.
    /// <https://lynxjs.org/api/css/properties/caret-color>
    pub fn caret_color(self, v: Color) -> Self {
        self.push("caret-color", v)
    }

    /// Sets `outline-width`.
    /// <https://lynxjs.org/api/css/properties/outline-width>
    pub fn outline_width(self, v: Length) -> Self {
        self.push("outline-width", v)
    }

    /// Sets `outline-color`.
    /// <https://lynxjs.org/api/css/properties/outline-color>
    pub fn outline_color(self, v: Color) -> Self {
        self.push("outline-color", v)
    }

    /// Sets `outline-style`.
    /// <https://lynxjs.org/api/css/properties/outline-style>
    pub fn outline_style(self, v: crate::keyword::BorderStyle) -> Self {
        self.push("outline-style", v)
    }

    /// Sets `outline-offset`.
    /// <https://lynxjs.org/api/css/properties/outline-offset>
    pub fn outline_offset(self, v: Length) -> Self {
        self.push("outline-offset", v)
    }

    /// Sets `caret-width`. Lynx accepts a length controlling the
    /// rendered caret thickness.
    pub fn caret_width(self, v: Length) -> Self {
        self.push("caret-width", v)
    }

    /// Sets `-x-handle-color` — Lynx-only selection-handle color.
    /// <https://lynxjs.org/api/css/properties/-x-handle-color>
    pub fn x_handle_color(self, v: Color) -> Self {
        self.push("-x-handle-color", v)
    }

    /// Sets `-x-handle-size` — Lynx-only selection-handle size.
    /// <https://lynxjs.org/api/css/properties/-x-handle-size>
    pub fn x_handle_size(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("-x-handle-size", v.into())
    }

    /// Sets `-x-auto-font-size` — Lynx-only auto font-size flag.
    /// <https://lynxjs.org/api/css/properties/-x-auto-font-size>
    pub fn x_auto_font_size(self, enabled: bool) -> Self {
        self.push_raw("-x-auto-font-size", if enabled { "true" } else { "false" })
    }

    /// Sets `-x-auto-font-size-preset-sizes` — Lynx-only list of preset sizes.
    /// <https://lynxjs.org/api/css/properties/-x-auto-font-size-preset-sizes>
    pub fn x_auto_font_size_preset_sizes(self, sizes: impl IntoIterator<Item = Length>) -> Self {
        use crate::to_css::ToCss;
        let mut s = String::new();
        let mut first = true;
        for sz in sizes {
            if !first {
                s.push(' ');
            }
            let _ = sz.to_css(&mut s);
            first = false;
        }
        self.push_raw("-x-auto-font-size-preset-sizes", s)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type::Color;
    use crate::ext::*;
    use crate::keyword::*;
    use crate::Css;

    #[test]
    fn opacity_full_range() {
        assert_eq!(Css::new().opacity(0.0).to_string(), "opacity: 0;");
        assert_eq!(Css::new().opacity(0.5).to_string(), "opacity: 0.5;");
        assert_eq!(Css::new().opacity(1.0).to_string(), "opacity: 1;");
    }

    #[test]
    fn visibility_keyword() {
        assert_eq!(
            Css::new().visibility(Visibility::Hidden).to_string(),
            "visibility: hidden;"
        );
    }

    #[test]
    fn overflow_expands_to_both_axes() {
        let s = Css::new().overflow(Overflow::Hidden);
        assert_eq!(s.to_string(), "overflow-x: hidden; overflow-y: hidden;");
    }

    #[test]
    fn overflow_axis_individual_override() {
        let s = Css::new()
            .overflow(Overflow::Hidden)
            .overflow_y(Overflow::Visible);
        assert_eq!(s.to_string(), "overflow-x: hidden; overflow-y: visible;");
    }

    #[test]
    fn cursor_and_pointer_events() {
        let s = Css::new()
            .cursor(Cursor::Pointer)
            .pointer_events(PointerEvents::None);
        assert_eq!(
            s.to_string(),
            "cursor: pointer; pointer-events: none;"
        );
    }

    #[test]
    fn box_shadow_outer() {
        let s = Css::new().box_shadow(
            2.px(),
            4.px(),
            8.px(),
            crate::data_type::Length::Zero,
            Color::hex(0x000000),
        );
        assert_eq!(
            s.to_string(),
            "box-shadow: 2px 4px 8px 0 rgb(0, 0, 0);"
        );
    }

    #[test]
    fn box_shadow_inset() {
        let s = Css::new().box_shadow_inset(
            crate::data_type::Length::Zero,
            crate::data_type::Length::Zero,
            4.px(),
            crate::data_type::Length::Zero,
            Color::hex(0xFFFFFF),
        );
        assert_eq!(
            s.to_string(),
            "box-shadow: inset 0 0 4px 0 rgb(255, 255, 255);"
        );
    }

    #[test]
    fn filter_clip_path_mask_raw() {
        let s = Css::new()
            .filter("blur(4px)")
            .clip_path("circle(50%)")
            .mask_image("url(\"a.png\")");
        assert_eq!(
            s.to_string(),
            "filter: blur(4px); clip-path: circle(50%); mask-image: url(\"a.png\");"
        );
    }

    #[test]
    fn outline_props() {
        let s = Css::new()
            .outline_width(1.px())
            .outline_style(BorderStyle::Solid)
            .outline_color(Color::hex(0xFF0000))
            .outline_offset(2.px());
        assert_eq!(
            s.to_string(),
            "outline-width: 1px; outline-style: solid; outline-color: rgb(255, 0, 0); outline-offset: 2px;"
        );
    }

    #[test]
    fn caret_props() {
        let s = Css::new()
            .caret_color(Color::hex(0xFF00FF))
            .caret_width(2.px());
        assert_eq!(
            s.to_string(),
            "caret-color: rgb(255, 0, 255); caret-width: 2px;"
        );
    }

    #[test]
    fn x_handle_props() {
        let s = Css::new()
            .x_handle_color(Color::hex(0x00FF00))
            .x_handle_size(8.px());
        assert_eq!(
            s.to_string(),
            "-x-handle-color: rgb(0, 255, 0); -x-handle-size: 8px;"
        );
    }

    #[test]
    fn x_auto_font_size_flag_and_presets() {
        let s = Css::new()
            .x_auto_font_size(true)
            .x_auto_font_size_preset_sizes([12.px(), 14.px(), 16.px()]);
        assert_eq!(
            s.to_string(),
            "-x-auto-font-size: true; -x-auto-font-size-preset-sizes: 12px 14px 16px;"
        );
    }
}
