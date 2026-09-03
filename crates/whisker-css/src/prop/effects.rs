//! Visual-effect properties: opacity, visibility, overflow, shadow,
//! filter, cursor, pointer-events, clip-path.

use crate::css::Css;
use crate::data_type::{Color, Length};
use crate::keyword::{Cursor, ImageRendering, Overflow, PointerEvents, Visibility};
use crate::style_value::ToStyleValue;
use crate::to_css::ToCss;
use crate::value::{BackdropFilter, BoxShadow, ClipPath};

impl Css {
    /// Sets `opacity`. Values are clamped to `0.0..=1.0`. Default: `1`.
    pub fn opacity(self, v: f32) -> Self {
        self.push_semantic(
            crate::StyleProperty::Opacity,
            whisker_style::StyleValue::Number(whisker_style::StyleNumber::new(v)),
            crate::to_css::number_to_string(v),
        )
    }

    /// Sets `visibility`. Default: `visible`. `collapse` is not supported.
    pub fn visibility(self, v: Visibility) -> Self {
        self.push_typed(crate::StyleProperty::Visibility, v)
    }

    /// Sets `overflow`. Whisker accepts only `visible` and `hidden`.
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
        self.push_typed(crate::StyleProperty::Cursor, v)
    }

    /// Sets `pointer-events`.
    /// <https://lynxjs.org/api/css/properties/pointer-events>
    pub fn pointer_events(self, v: PointerEvents) -> Self {
        self.push_typed(crate::StyleProperty::PointerEvents, v)
    }

    /// Sets `box-shadow` to one outer shadow.
    pub fn box_shadow(
        self,
        offset_x: Length,
        offset_y: Length,
        blur_radius: Length,
        spread_radius: Length,
        color: Color,
    ) -> Self {
        self.box_shadows([BoxShadow::outer(
            offset_x,
            offset_y,
            blur_radius,
            spread_radius,
            color,
        )])
    }

    /// Sets an inset `box-shadow`.
    pub fn box_shadow_inset(
        self,
        offset_x: Length,
        offset_y: Length,
        blur_radius: Length,
        spread_radius: Length,
        color: Color,
    ) -> Self {
        self.box_shadows([BoxShadow::inset(
            offset_x,
            offset_y,
            blur_radius,
            spread_radius,
            color,
        )])
    }

    /// Replaces `box-shadow` with an ordered list of typed shadows.
    /// An empty iterator serializes to `none` and clears existing shadows.
    pub fn box_shadows(self, shadows: impl IntoIterator<Item = BoxShadow>) -> Self {
        let shadows: Vec<_> = shadows.into_iter().collect();
        let serialized = if shadows.is_empty() {
            "none".to_owned()
        } else {
            shadows
                .iter()
                .map(ToCss::to_css_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let values = shadows
            .into_iter()
            .flat_map(|shadow| match shadow.to_style_value() {
                whisker_style::StyleValue::BoxShadows(values) => values,
                _ => unreachable!("BoxShadow always produces BoxShadows"),
            })
            .collect();
        self.push_semantic(
            crate::StyleProperty::BoxShadow,
            whisker_style::StyleValue::BoxShadows(values),
            serialized,
        )
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

    /// Applies a structured basic-shape clip.
    pub fn clip_path(self, value: ClipPath) -> Self {
        self.push_typed(crate::StyleProperty::ClipPath, value)
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
    fn multiple_box_shadows_remain_structured() {
        let style = Css::new().box_shadows([
            crate::BoxShadow::outer(1.px(), 2.px(), 3.px(), 0.px(), Color::hex(0x112233)),
            crate::BoxShadow::inset(4.px(), 5.px(), 6.px(), 1.px(), Color::hex(0x445566)),
        ]);
        assert!(style.to_string().contains(", inset 4px 5px 6px 1px"));
        assert!(matches!(
            style.to_specified_style().declarations().next().map(|declaration| declaration.value()),
            Some(whisker_style::StyleValue::BoxShadows(values)) if values.len() == 2
        ));
    }

    #[test]
    fn backdrop_blur_and_clip_path_are_typed() {
        let s = Css::new()
            .backdrop_filter(BackdropFilter::blur(4.px()))
            .clip_path(crate::ClipPath::circle(50.percent()));
        assert_eq!(
            s.to_string(),
            "backdrop-filter: blur(4px); clip-path: circle(50% at 50% 50%) border-box;"
        );
        let _ = s.to_specified_style();
    }

    #[test]
    fn image_rendering_is_typed() {
        let style = Css::new().image_rendering(ImageRendering::Pixelated);
        assert_eq!(style.to_string(), "image-rendering: pixelated;");
        let _ = style.to_specified_style();
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
}
