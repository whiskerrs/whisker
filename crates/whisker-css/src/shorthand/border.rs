//! `border` shorthand builder.

use crate::css::Css;
use crate::data_type::{Color, LengthPercentage};
use crate::keyword::BorderStyle;

/// Builder for the `border` family. Any subset of width/style/color
/// can be set; only the populated dimensions are pushed to the
/// style. Use [`Css::border`] to apply to all four sides;
/// [`Css::border_top`], [`Css::border_right`],
/// [`Css::border_bottom`], [`Css::border_left`] target one side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Border {
    /// Width to apply.
    pub width: Option<LengthPercentage>,
    /// Line style to apply.
    pub style: Option<BorderStyle>,
    /// Color to apply.
    pub color: Option<Color>,
}

impl Border {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the border width.
    pub fn width(mut self, v: impl Into<LengthPercentage>) -> Self {
        self.width = Some(v.into());
        self
    }

    /// Set the border style.
    pub fn style(mut self, v: BorderStyle) -> Self {
        self.style = Some(v);
        self
    }

    /// Set the border style to [`BorderStyle::Solid`].
    pub fn solid(self) -> Self {
        self.style(BorderStyle::Solid)
    }

    /// Set the border style to [`BorderStyle::Dashed`].
    pub fn dashed(self) -> Self {
        self.style(BorderStyle::Dashed)
    }

    /// Set the border style to [`BorderStyle::Dotted`].
    pub fn dotted(self) -> Self {
        self.style(BorderStyle::Dotted)
    }

    /// Set the border color.
    pub fn color(mut self, v: Color) -> Self {
        self.color = Some(v);
        self
    }
}

impl Css {
    /// Sets `border` for all four sides. Equivalent to setting
    /// `border-top`, `border-right`, `border-bottom`, `border-left`
    /// individually.
    /// <https://lynxjs.org/api/css/properties/border>
    pub fn border(self, b: Border) -> Self {
        self.border_top(b.clone())
            .border_right(b.clone())
            .border_bottom(b.clone())
            .border_left(b)
    }

    /// Sets `border-top` only.
    /// <https://lynxjs.org/api/css/properties/border-top>
    pub fn border_top(mut self, b: Border) -> Self {
        if let Some(w) = b.width {
            self = self.border_top_width(w);
        }
        if let Some(s) = b.style {
            self = self.border_top_style(s);
        }
        if let Some(c) = b.color {
            self = self.border_top_color(c);
        }
        self
    }

    /// Sets `border-right` only.
    /// <https://lynxjs.org/api/css/properties/border-right>
    pub fn border_right(mut self, b: Border) -> Self {
        if let Some(w) = b.width {
            self = self.border_right_width(w);
        }
        if let Some(s) = b.style {
            self = self.border_right_style(s);
        }
        if let Some(c) = b.color {
            self = self.border_right_color(c);
        }
        self
    }

    /// Sets `border-bottom` only.
    /// <https://lynxjs.org/api/css/properties/border-bottom>
    pub fn border_bottom(mut self, b: Border) -> Self {
        if let Some(w) = b.width {
            self = self.border_bottom_width(w);
        }
        if let Some(s) = b.style {
            self = self.border_bottom_style(s);
        }
        if let Some(c) = b.color {
            self = self.border_bottom_color(c);
        }
        self
    }

    /// Sets `border-left` only.
    /// <https://lynxjs.org/api/css/properties/border-left>
    pub fn border_left(mut self, b: Border) -> Self {
        if let Some(w) = b.width {
            self = self.border_left_width(w);
        }
        if let Some(s) = b.style {
            self = self.border_left_style(s);
        }
        if let Some(c) = b.color {
            self = self.border_left_color(c);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type::Color;
    use crate::ext::*;
    use crate::keyword::BorderStyle;
    use crate::Css;

    use super::*;

    #[test]
    fn border_full() {
        let s = Css::new().border(
            Border::new()
                .width(px(1))
                .solid()
                .color(Color::hex(0xCCCCCC)),
        );
        assert_eq!(
            s.to_string(),
            "border-top-width: 1px; border-top-style: solid; border-top-color: rgb(204, 204, 204); border-right-width: 1px; border-right-style: solid; border-right-color: rgb(204, 204, 204); border-bottom-width: 1px; border-bottom-style: solid; border-bottom-color: rgb(204, 204, 204); border-left-width: 1px; border-left-style: solid; border-left-color: rgb(204, 204, 204);"
        );
    }

    #[test]
    fn border_partial_only_style() {
        let s = Css::new().border(Border::new().solid());
        assert_eq!(
            s.to_string(),
            "border-top-style: solid; border-right-style: solid; border-bottom-style: solid; border-left-style: solid;"
        );
    }

    #[test]
    fn border_bottom_overrides_border() {
        let s = Css::new()
            .border(
                Border::new()
                    .width(px(1))
                    .solid()
                    .color(Color::hex(0x000000)),
            )
            .border_bottom(Border::new().width(px(3)).color(Color::hex(0xFF0000)));
        // Last-write-wins keeps the first 3 sides at width 1px and
        // overrides bottom to 3px / red.
        let css = s.to_string();
        assert!(css.contains("border-bottom-width: 3px"));
        assert!(css.contains("border-bottom-color: rgb(255, 0, 0)"));
        assert!(css.contains("border-bottom-style: solid"));
        assert!(css.contains("border-top-width: 1px"));
    }

    #[test]
    fn border_style_constructors() {
        let solid = Border::new().solid();
        let dashed = Border::new().dashed();
        let dotted = Border::new().dotted();
        assert_eq!(solid.style, Some(BorderStyle::Solid));
        assert_eq!(dashed.style, Some(BorderStyle::Dashed));
        assert_eq!(dotted.style, Some(BorderStyle::Dotted));
    }

    #[test]
    fn border_per_side_individual() {
        let s = Css::new()
            .border_top(Border::new().width(px(1)).solid())
            .border_right(Border::new().width(px(2)).dashed())
            .border_bottom(Border::new().width(px(3)).dotted())
            .border_left(Border::new().width(px(4)).style(BorderStyle::Double));
        assert_eq!(
            s.to_string(),
            "border-top-width: 1px; border-top-style: solid; border-right-width: 2px; border-right-style: dashed; border-bottom-width: 3px; border-bottom-style: dotted; border-left-width: 4px; border-left-style: double;"
        );
    }
}
