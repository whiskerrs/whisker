//! Border longhand properties + `border-radius` corners.

use crate::css::Css;
use crate::data_type::{Color, LengthPercentage};
use crate::keyword::BorderStyle;

impl Css {
    // ---------- border-width longhands ----------

    /// Sets `border-top-width`.
    /// <https://lynxjs.org/api/css/properties/border-top-width>
    pub fn border_top_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-top-width", v.into())
    }

    /// Sets `border-right-width`.
    /// <https://lynxjs.org/api/css/properties/border-right-width>
    pub fn border_right_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-right-width", v.into())
    }

    /// Sets `border-bottom-width`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-width>
    pub fn border_bottom_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-bottom-width", v.into())
    }

    /// Sets `border-left-width`.
    /// <https://lynxjs.org/api/css/properties/border-left-width>
    pub fn border_left_width(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-left-width", v.into())
    }

    // ---------- border-style longhands ----------

    /// Sets `border-top-style`.
    /// <https://lynxjs.org/api/css/properties/border-top-style>
    pub fn border_top_style(self, v: BorderStyle) -> Self {
        self.push("border-top-style", v)
    }

    /// Sets `border-right-style`.
    /// <https://lynxjs.org/api/css/properties/border-right-style>
    pub fn border_right_style(self, v: BorderStyle) -> Self {
        self.push("border-right-style", v)
    }

    /// Sets `border-bottom-style`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-style>
    pub fn border_bottom_style(self, v: BorderStyle) -> Self {
        self.push("border-bottom-style", v)
    }

    /// Sets `border-left-style`.
    /// <https://lynxjs.org/api/css/properties/border-left-style>
    pub fn border_left_style(self, v: BorderStyle) -> Self {
        self.push("border-left-style", v)
    }

    // ---------- border-color longhands ----------

    /// Sets `border-top-color`.
    /// <https://lynxjs.org/api/css/properties/border-top-color>
    pub fn border_top_color(self, v: Color) -> Self {
        self.push("border-top-color", v)
    }

    /// Sets `border-right-color`.
    /// <https://lynxjs.org/api/css/properties/border-right-color>
    pub fn border_right_color(self, v: Color) -> Self {
        self.push("border-right-color", v)
    }

    /// Sets `border-bottom-color`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-color>
    pub fn border_bottom_color(self, v: Color) -> Self {
        self.push("border-bottom-color", v)
    }

    /// Sets `border-left-color`.
    /// <https://lynxjs.org/api/css/properties/border-left-color>
    pub fn border_left_color(self, v: Color) -> Self {
        self.push("border-left-color", v)
    }

    // ---------- border-radius corners ----------

    /// Sets `border-top-left-radius`.
    /// <https://lynxjs.org/api/css/properties/border-top-left-radius>
    pub fn border_top_left_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-top-left-radius", v.into())
    }

    /// Sets `border-top-right-radius`.
    /// <https://lynxjs.org/api/css/properties/border-top-right-radius>
    pub fn border_top_right_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-top-right-radius", v.into())
    }

    /// Sets `border-bottom-right-radius`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-right-radius>
    pub fn border_bottom_right_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-bottom-right-radius", v.into())
    }

    /// Sets `border-bottom-left-radius`.
    /// <https://lynxjs.org/api/css/properties/border-bottom-left-radius>
    pub fn border_bottom_left_radius(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("border-bottom-left-radius", v.into())
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
    /// Pushed as the shorthand because Lynx serializes elliptical
    /// corners via the `/` separator that has no longhand form.
    /// <https://lynxjs.org/api/css/properties/border-radius>
    pub fn border_radius_full(self, v: crate::BorderRadius) -> Self {
        self.push("border-radius", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type::Color;
    use crate::ext::*;
    use crate::keyword::BorderStyle;
    use crate::Css;

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
}
