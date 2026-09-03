//! Border longhand properties + `border-radius` corners.

use crate::css::Css;
use crate::data_type::{Color, LengthPercentage};
use crate::keyword::BorderStyle;

impl Css {
    // ---------- border-width longhands ----------

    /// Sets `border-top-width`.
    /// <https://lynxjs.org/api/css/properties/border-top-width>
    pub fn border_top_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderTopWidth, v.into())
    }

    /// Sets `border-right-width`.
    /// <https://lynxjs.org/api/css/properties/border-right-width>
    pub fn border_right_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderRightWidth, v.into())
    }

    /// Sets `border-bottom-width`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-width>
    pub fn border_bottom_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderBottomWidth, v.into())
    }

    /// Sets `border-left-width`.
    /// <https://lynxjs.org/api/css/properties/border-left-width>
    pub fn border_left_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderLeftWidth, v.into())
    }

    /// Sets `border-inline-start-width`; Rust resolves it using `direction`.
    pub fn border_inline_start_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderInlineStartWidth, v.into())
    }

    /// Sets `border-inline-end-width`; Rust resolves it using `direction`.
    pub fn border_inline_end_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderInlineEndWidth, v.into())
    }

    // ---------- border-style longhands ----------

    /// Sets `border-top-style`.
    /// <https://lynxjs.org/api/css/properties/border-top-style>
    pub fn border_top_style(self, v: BorderStyle) -> Self {
        self.push_typed(crate::StyleProperty::BorderTopStyle, v)
    }

    /// Sets `border-right-style`.
    /// <https://lynxjs.org/api/css/properties/border-right-style>
    pub fn border_right_style(self, v: BorderStyle) -> Self {
        self.push_typed(crate::StyleProperty::BorderRightStyle, v)
    }

    /// Sets `border-bottom-style`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-style>
    pub fn border_bottom_style(self, v: BorderStyle) -> Self {
        self.push_typed(crate::StyleProperty::BorderBottomStyle, v)
    }

    /// Sets `border-left-style`.
    /// <https://lynxjs.org/api/css/properties/border-left-style>
    pub fn border_left_style(self, v: BorderStyle) -> Self {
        self.push_typed(crate::StyleProperty::BorderLeftStyle, v)
    }

    /// Sets `border-inline-start-style`; Rust resolves it using `direction`.
    pub fn border_inline_start_style(self, v: BorderStyle) -> Self {
        self.push_typed(crate::StyleProperty::BorderInlineStartStyle, v)
    }

    /// Sets `border-inline-end-style`; Rust resolves it using `direction`.
    pub fn border_inline_end_style(self, v: BorderStyle) -> Self {
        self.push_typed(crate::StyleProperty::BorderInlineEndStyle, v)
    }

    // ---------- border-color longhands ----------

    /// Sets `border-top-color`.
    /// <https://lynxjs.org/api/css/properties/border-top-color>
    pub fn border_top_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BorderTopColor, v)
    }

    /// Sets `border-right-color`.
    /// <https://lynxjs.org/api/css/properties/border-right-color>
    pub fn border_right_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BorderRightColor, v)
    }

    /// Sets `border-bottom-color`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-color>
    pub fn border_bottom_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BorderBottomColor, v)
    }

    /// Sets `border-left-color`.
    /// <https://lynxjs.org/api/css/properties/border-left-color>
    pub fn border_left_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BorderLeftColor, v)
    }

    /// Sets `border-inline-start-color`; Rust resolves it using `direction`.
    pub fn border_inline_start_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BorderInlineStartColor, v)
    }

    /// Sets `border-inline-end-color`; Rust resolves it using `direction`.
    pub fn border_inline_end_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BorderInlineEndColor, v)
    }

    // ---------- border-radius corners ----------

    /// Sets `border-top-left-radius`.
    /// <https://lynxjs.org/api/css/properties/border-top-left-radius>
    pub fn border_top_left_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderTopLeftRadius, v.into())
    }

    /// Sets `border-top-right-radius`.
    /// <https://lynxjs.org/api/css/properties/border-top-right-radius>
    pub fn border_top_right_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderTopRightRadius, v.into())
    }

    /// Sets `border-bottom-right-radius`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-right-radius>
    pub fn border_bottom_right_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderBottomRightRadius, v.into())
    }

    /// Sets `border-bottom-left-radius`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-left-radius>
    pub fn border_bottom_left_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderBottomLeftRadius, v.into())
    }

    /// Sets `border-start-start-radius`; Rust resolves it using `direction`.
    pub fn border_start_start_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderStartStartRadius, v.into())
    }

    /// Sets `border-start-end-radius`; Rust resolves it using `direction`.
    pub fn border_start_end_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderStartEndRadius, v.into())
    }

    /// Sets `border-end-start-radius`; Rust resolves it using `direction`.
    pub fn border_end_start_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderEndStartRadius, v.into())
    }

    /// Sets `border-end-end-radius`; Rust resolves it using `direction`.
    pub fn border_end_end_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BorderEndEndRadius, v.into())
    }

    /// Sets `border-radius` shorthand. Expands to the four corner
    /// longhand properties so subsequent per-corner overrides win.
    /// <https://lynxjs.org/api/css/properties/border-radius>
    pub fn border_radius(self, v: impl Into<LengthPercentage>) -> Self {
        let v = v.into();
        self.border_top_left_radius(v.clone())
            .border_top_right_radius(v.clone())
            .border_bottom_right_radius(v.clone())
            .border_bottom_left_radius(v)
    }

    /// Sets `border-radius` to a [`BorderRadius`](crate::BorderRadius)
    /// with per-corner control and optional elliptical second axis.
    /// Expands to semantic corner longhands; each corner longhand accepts
    /// the standard two-value horizontal/vertical form.
    /// <https://lynxjs.org/api/css/properties/border-radius>
    pub fn border_radius_full(self, v: crate::BorderRadius) -> Self {
        let vertical = v.vertical.as_ref().unwrap_or(&v.horizontal);
        let properties = [
            crate::StyleProperty::BorderTopLeftRadius,
            crate::StyleProperty::BorderTopRightRadius,
            crate::StyleProperty::BorderBottomRightRadius,
            crate::StyleProperty::BorderBottomLeftRadius,
        ];
        properties
            .into_iter()
            .enumerate()
            .fold(self, |css, (index, property)| {
                let horizontal = &v.horizontal[index];
                let vertical = &vertical[index];
                let serialized_value = if horizontal == vertical {
                    crate::ToCss::to_css_string(horizontal)
                } else {
                    format!(
                        "{} {}",
                        crate::ToCss::to_css_string(horizontal),
                        crate::ToCss::to_css_string(vertical)
                    )
                };
                css.push_semantic(
                    property,
                    crate::style_value::to_border_radius(horizontal, vertical),
                    serialized_value,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type::Color;
    use crate::ext::*;
    use crate::keyword::BorderStyle;

    #[test]
    fn border_width_per_side() {
        let s = Css::new()
            .border_top_width(px(1))
            .border_right_width(px(2))
            .border_bottom_width(px(3))
            .border_left_width(px(4));
        assert_eq!(
            s.to_string(),
            "border-top-width: 1px; border-right-width: 2px; border-bottom-width: 3px; border-left-width: 4px;"
        );
    }

    #[test]
    fn border_style_per_side() {
        let s = Css::new()
            .border_top_style(BorderStyle::Solid)
            .border_right_style(BorderStyle::Dashed)
            .border_bottom_style(BorderStyle::Dotted)
            .border_left_style(BorderStyle::Double);
        assert_eq!(
            s.to_string(),
            "border-top-style: solid; border-right-style: dashed; border-bottom-style: dotted; border-left-style: double;"
        );
    }

    #[test]
    fn border_color_per_side() {
        let red = Color::hex(0xFF0000);
        let blue = Color::hex(0x0000FF);
        let s = Css::new()
            .border_top_color(red)
            .border_right_color(blue)
            .border_bottom_color(red)
            .border_left_color(blue);
        assert_eq!(
            s.to_string(),
            "border-top-color: rgb(255, 0, 0); border-right-color: rgb(0, 0, 255); border-bottom-color: rgb(255, 0, 0); border-left-color: rgb(0, 0, 255);"
        );
    }

    #[test]
    fn border_radius_uniform_expands() {
        let s = Css::new().border_radius(px(8));
        assert_eq!(
            s.to_string(),
            "border-top-left-radius: 8px; border-top-right-radius: 8px; border-bottom-right-radius: 8px; border-bottom-left-radius: 8px;"
        );
    }

    #[test]
    fn border_radius_corners_individual_override() {
        let s = Css::new()
            .border_radius(px(8))
            .border_top_left_radius(px(0));
        assert_eq!(
            s.to_string(),
            "border-top-right-radius: 8px; border-bottom-right-radius: 8px; border-bottom-left-radius: 8px; border-top-left-radius: 0px;"
        );
    }

    #[test]
    fn logical_border_longhands_keep_typed_semantics() {
        let css = Css::new()
            .border_inline_start_width(px(2))
            .border_inline_end_width(px(3))
            .border_inline_start_style(BorderStyle::Dotted)
            .border_inline_end_style(BorderStyle::Double)
            .border_inline_start_color(Color::hex(0x112233))
            .border_inline_end_color(Color::hex(0x445566))
            .border_start_start_radius(px(4))
            .border_start_end_radius(px(5))
            .border_end_start_radius(px(6))
            .border_end_end_radius(px(7));
        assert_eq!(css.to_specified_style().len(), 10);
        assert_eq!(
            css.to_string(),
            "border-inline-start-width: 2px; border-inline-end-width: 3px; border-inline-start-style: dotted; border-inline-end-style: double; border-inline-start-color: rgb(17, 34, 51); border-inline-end-color: rgb(68, 85, 102); border-start-start-radius: 4px; border-start-end-radius: 5px; border-end-start-radius: 6px; border-end-end-radius: 7px;"
        );
    }
}
