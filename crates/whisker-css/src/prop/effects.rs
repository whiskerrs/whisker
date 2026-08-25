//! Visual-effect properties: opacity, visibility, overflow, shadow,
//! filter, cursor, pointer-events, clip-path.

use crate::css::Css;
use crate::data_type::{Color, Length};
use crate::keyword::{Cursor, ImageRendering, Overflow, PointerEvents, Visibility};
use crate::value::BackdropFilter;

impl Css {
    /// Sets `opacity`. Lynx clamps to `0.0..=1.0`. Default: `1`.
    /// <https://lynxjs.org/api/css/properties/opacity>
    pub fn opacity(self, v: f32) -> Self {
        self.push_semantic(
            crate::StyleProperty::Opacity,
            whisker_style::StyleValue::Number(whisker_style::StyleNumber::new(v)),
            crate::to_css::number_to_string(v),
        )
    }

    /// Sets `visibility`. Lynx default: `visible`. `collapse` is not
    /// supported.
    /// <https://lynxjs.org/api/css/properties/visibility>
    pub fn visibility(self, v: Visibility) -> Self {
        self.push_typed(crate::StyleProperty::Visibility, v)
    }

    /// Sets `overflow`. Lynx accepts only `visible` and `hidden`.
    /// <https://lynxjs.org/api/css/properties/overflow>
    pub fn overflow(self, v: Overflow) -> Self {
        self.push_typed(crate::StyleProperty::OverflowX, v)
            .push_typed(crate::StyleProperty::OverflowY, v)
    }

    /// Sets `overflow-x`.
    /// <https://lynxjs.org/api/css/properties/overflow-x>
    pub fn overflow_x(self, v: Overflow) -> Self {
        self.push_typed(crate::StyleProperty::OverflowX, v)
    }

    /// Sets `overflow-y`.
    /// <https://lynxjs.org/api/css/properties/overflow-y>
    pub fn overflow_y(self, v: Overflow) -> Self {
        self.push_typed(crate::StyleProperty::OverflowY, v)
    }

    /// Sets `cursor`.
    /// <https://lynxjs.org/api/css/properties/cursor>
    pub fn cursor(self, v: Cursor) -> Self {
        self.push(crate::StyleProperty::Cursor, v)
    }

    /// Sets `pointer-events`.
    /// <https://lynxjs.org/api/css/properties/pointer-events>
    pub fn pointer_events(self, v: PointerEvents) -> Self {
        self.push(crate::StyleProperty::PointerEvents, v)
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
        self.push_raw(crate::StyleProperty::BoxShadow, s)
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
        self.push_raw(crate::StyleProperty::BoxShadow, s)
    }

    /// Sets the supported `backdrop-filter` subset: `none` or one
    /// `blur(<length>)` function.
    pub fn backdrop_filter(self, value: BackdropFilter) -> Self {
        self.push_typed(crate::StyleProperty::BackdropFilter, value)
    }

    /// Sets raster-image interpolation for this element's image paint.
    /// <https://lynxjs.org/api/css/properties/image-rendering>
    pub fn image_rendering(self, value: ImageRendering) -> Self {
        self.push_typed(crate::StyleProperty::ImageRendering, value)
    }

    /// Sets `mask-image` to a raw CSS value (URL or gradient).
    /// <https://lynxjs.org/api/css/properties/mask-image>
    pub fn mask_image(self, value: impl Into<String>) -> Self {
        self.push_raw(crate::StyleProperty::MaskImage, value)
    }

    /// Sets `clip-path` to a raw CSS value.
    /// <https://lynxjs.org/api/css/properties/clip-path>
    pub fn clip_path(self, value: impl Into<String>) -> Self {
        self.push_raw(crate::StyleProperty::ClipPath, value)
    }

    /// Sets `caret-color`.
    /// <https://lynxjs.org/api/css/properties/caret-color>
    pub fn caret_color(self, v: Color) -> Self {
        self.push(crate::StyleProperty::CaretColor, v)
    }

    /// Sets `outline-width`.
    /// <https://lynxjs.org/api/css/properties/outline-width>
    pub fn outline_width(self, v: Length) -> Self {
        self.push_typed(crate::StyleProperty::OutlineWidth, v)
    }

    /// Sets `outline-color`.
    /// <https://lynxjs.org/api/css/properties/outline-color>
    pub fn outline_color(self, v: Color) -> Self {
        self.push(crate::StyleProperty::OutlineColor, v)
    }

    /// Sets `outline-style`.
    /// <https://lynxjs.org/api/css/properties/outline-style>
    pub fn outline_style(self, v: crate::keyword::BorderStyle) -> Self {
        self.push(crate::StyleProperty::OutlineStyle, v)
    }

    /// Sets `outline-offset`.
    /// <https://lynxjs.org/api/css/properties/outline-offset>
    pub fn outline_offset(self, v: Length) -> Self {
        self.push_typed(crate::StyleProperty::OutlineOffset, v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type::Color;
    use crate::ext::*;
    use crate::keyword::*;
    use crate::value::BackdropFilter;

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
        assert_eq!(s.to_string(), "cursor: pointer; pointer-events: none;");
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
        assert_eq!(s.to_string(), "box-shadow: 2px 4px 8px 0 rgb(0, 0, 0);");
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
    fn backdrop_blur_is_typed_while_clip_and_mask_remain_raw() {
        let s = Css::new()
            .backdrop_filter(BackdropFilter::blur(4.px()))
            .clip_path("circle(50%)")
            .mask_image("url(\"a.png\")");
        assert_eq!(
            s.to_string(),
            "backdrop-filter: blur(4px); clip-path: circle(50%); mask-image: url(\"a.png\");"
        );
        assert!(
            s.to_specified_style().is_err(),
            "clip/mask remain unmigrated"
        );

        let typed = Css::new().backdrop_filter(BackdropFilter::blur(4.px()));
        assert!(typed.to_specified_style().is_ok());
    }

    #[test]
    fn image_rendering_is_typed() {
        let style = Css::new().image_rendering(ImageRendering::Pixelated);
        assert_eq!(style.to_string(), "image-rendering: pixelated;");
        assert!(style.to_specified_style().is_ok());
        assert_eq!(
            Css::new().image_rendering(ImageRendering::Auto).to_string(),
            "image-rendering: auto;"
        );
        assert_eq!(
            Css::new()
                .image_rendering(ImageRendering::CrispEdges)
                .to_string(),
            "image-rendering: crisp-edges;"
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
        let s = Css::new().caret_color(Color::hex(0xFF00FF));
        assert_eq!(s.to_string(), "caret-color: rgb(255, 0, 255);");
    }
}
