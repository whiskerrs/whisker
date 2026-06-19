//! Box model properties: sizing, padding, margin, gap, aspect-ratio.
//!
//! Lynx property index:
//! <https://lynxjs.org/api/css/properties>

use crate::css::Css;
use crate::data_type::LengthPercentage;
use crate::keyword::BoxSizing;
use crate::shorthand::padding_margin::MarginValue;
use crate::value::Size;

impl Css {
    // ---------- Width / Height ----------

    /// Sets `width`. Lynx default: `auto`.
    /// <https://lynxjs.org/api/css/properties/width>
    pub fn width(self, v: impl Into<Size>) -> Self {
        self.push("width", v.into())
    }

    /// Sets `height`. Lynx default: `auto`.
    /// <https://lynxjs.org/api/css/properties/height>
    pub fn height(self, v: impl Into<Size>) -> Self {
        self.push("height", v.into())
    }

    /// Sets `min-width`. Lynx default: `0`.
    /// <https://lynxjs.org/api/css/properties/min-width>
    pub fn min_width(self, v: impl Into<Size>) -> Self {
        self.push("min-width", v.into())
    }

    /// Sets `min-height`. Lynx default: `0`.
    /// <https://lynxjs.org/api/css/properties/min-height>
    pub fn min_height(self, v: impl Into<Size>) -> Self {
        self.push("min-height", v.into())
    }

    /// Sets `max-width`. Lynx default: `none`.
    /// <https://lynxjs.org/api/css/properties/max-width>
    pub fn max_width(self, v: impl Into<Size>) -> Self {
        self.push("max-width", v.into())
    }

    /// Sets `max-height`. Lynx default: `none`.
    /// <https://lynxjs.org/api/css/properties/max-height>
    pub fn max_height(self, v: impl Into<Size>) -> Self {
        self.push("max-height", v.into())
    }

    // ---------- box-sizing / aspect-ratio ----------

    /// Sets `box-sizing`. Lynx default: `border-box`.
    /// <https://lynxjs.org/api/css/properties/box-sizing>
    pub fn box_sizing(self, v: BoxSizing) -> Self {
        self.push("box-sizing", v)
    }

    /// Sets `aspect-ratio` to `<width> / <height>`.
    /// <https://lynxjs.org/api/css/properties/aspect-ratio>
    pub fn aspect_ratio(self, width: f32, height: f32) -> Self {
        self.push_raw("aspect-ratio", format!("{width} / {height}"))
    }

    // ---------- Padding longhand ----------

    /// Sets `padding-top`. Negative values are clamped to zero by Lynx.
    /// <https://lynxjs.org/api/css/properties/padding-top>
    pub fn padding_top(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("padding-top", v.into())
    }

    /// Sets `padding-right`. Negative values are clamped to zero by Lynx.
    /// <https://lynxjs.org/api/css/properties/padding-right>
    pub fn padding_right(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("padding-right", v.into())
    }

    /// Sets `padding-bottom`. Negative values are clamped to zero by Lynx.
    /// <https://lynxjs.org/api/css/properties/padding-bottom>
    pub fn padding_bottom(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("padding-bottom", v.into())
    }

    /// Sets `padding-left`. Negative values are clamped to zero by Lynx.
    /// <https://lynxjs.org/api/css/properties/padding-left>
    pub fn padding_left(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("padding-left", v.into())
    }

    // ---------- Margin longhand ----------

    /// Sets `margin-top`. Lynx allows negative values and `auto`.
    /// <https://lynxjs.org/api/css/properties/margin-top>
    pub fn margin_top(self, v: impl Into<MarginValue>) -> Self {
        self.push("margin-top", v.into())
    }

    /// Sets `margin-right`. Lynx allows negative values and `auto`.
    /// <https://lynxjs.org/api/css/properties/margin-right>
    pub fn margin_right(self, v: impl Into<MarginValue>) -> Self {
        self.push("margin-right", v.into())
    }

    /// Sets `margin-bottom`. Lynx allows negative values and `auto`.
    /// <https://lynxjs.org/api/css/properties/margin-bottom>
    pub fn margin_bottom(self, v: impl Into<MarginValue>) -> Self {
        self.push("margin-bottom", v.into())
    }

    /// Sets `margin-left`. Lynx allows negative values and `auto`.
    /// <https://lynxjs.org/api/css/properties/margin-left>
    pub fn margin_left(self, v: impl Into<MarginValue>) -> Self {
        self.push("margin-left", v.into())
    }

    // ---------- Gap ----------

    /// Sets `gap` — shorthand for `row-gap` and `column-gap`.
    /// <https://lynxjs.org/api/css/properties/gap>
    pub fn gap(self, v: impl Into<LengthPercentage>) -> Self {
        let v = v.into();
        self.push("row-gap", v.clone()).push("column-gap", v)
    }

    /// Sets `row-gap` — inline gap between rows in flex/grid layouts.
    /// <https://lynxjs.org/api/css/properties/row-gap>
    pub fn row_gap(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("row-gap", v.into())
    }

    /// Sets `column-gap` — gap between columns in flex/grid layouts.
    /// <https://lynxjs.org/api/css/properties/column-gap>
    pub fn column_gap(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("column-gap", v.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type::{FitContent, MaxContent};
    use crate::ext::*;
    use crate::keyword::BoxSizing;
    use crate::value::Size;

    #[test]
    fn width_height_basic() {
        let s = Css::new().width(px(100)).height(50.percent());
        assert_eq!(s.to_string(), "width: 100px; height: 50%;");
    }

    #[test]
    fn min_max_dimensions() {
        let s = Css::new()
            .min_width(px(50))
            .min_height(px(50))
            .max_width(percent(80))
            .max_height(Size::None);
        assert_eq!(
            s.to_string(),
            "min-width: 50px; min-height: 50px; max-width: 80%; max-height: none;"
        );
    }

    #[test]
    fn intrinsic_sizing_keywords() {
        let s = Css::new()
            .width(Size::Auto)
            .height(MaxContent)
            .min_width(Size::MinContent)
            .max_width(FitContent::keyword());
        assert_eq!(
            s.to_string(),
            "width: auto; height: max-content; min-width: min-content; max-width: fit-content;"
        );
    }

    #[test]
    fn box_sizing_keyword() {
        let s = Css::new().box_sizing(BoxSizing::BorderBox);
        assert_eq!(s.to_string(), "box-sizing: border-box;");
    }

    #[test]
    fn aspect_ratio_pair() {
        let s = Css::new().aspect_ratio(16.0, 9.0);
        assert_eq!(s.to_string(), "aspect-ratio: 16 / 9;");
    }

    #[test]
    fn padding_longhands() {
        let s = Css::new()
            .padding_top(px(2))
            .padding_right(px(4))
            .padding_bottom(px(6))
            .padding_left(px(8));
        assert_eq!(
            s.to_string(),
            "padding-top: 2px; padding-right: 4px; padding-bottom: 6px; padding-left: 8px;"
        );
    }

    #[test]
    fn margin_longhands_allow_negatives() {
        let s = Css::new()
            .margin_top(px(-4))
            .margin_right(0.percent())
            .margin_bottom(px(8))
            .margin_left(percent(-50.0));
        assert_eq!(
            s.to_string(),
            "margin-top: -4px; margin-right: 0%; margin-bottom: 8px; margin-left: -50%;"
        );
    }

    #[test]
    fn gap_expands_to_row_and_column() {
        let s = Css::new().gap(px(8));
        assert_eq!(s.to_string(), "row-gap: 8px; column-gap: 8px;");
    }

    #[test]
    fn row_and_column_gap_individual() {
        let s = Css::new().row_gap(px(4)).column_gap(px(12));
        assert_eq!(s.to_string(), "row-gap: 4px; column-gap: 12px;");
    }

    #[test]
    fn padding_top_override_via_last_write_wins() {
        let s = Css::new().padding_top(px(8)).padding_top(px(0));
        assert_eq!(s.to_string(), "padding-top: 0px;");
    }
}
