//! `display` and `direction` properties.

use crate::css::Css;
use crate::keyword::{Direction, Display};

impl Css {
    /// Sets `display`. Lynx default: `linear`.
    /// <https://lynxjs.org/api/css/properties/display>
    pub fn display(self, v: Display) -> Self {
        self.push("display", v)
    }

    /// Sets `display: none` — element is removed from the layout tree.
    pub fn display_none(self) -> Self {
        self.push("display", Display::None)
    }

    /// Sets `display: flex` — opt into CSS flexbox.
    pub fn display_flex(self) -> Self {
        self.push("display", Display::Flex)
    }

    /// Sets `display: grid` — opt into CSS grid.
    pub fn display_grid(self) -> Self {
        self.push("display", Display::Grid)
    }

    /// Sets `display: linear` — Lynx default linear layout.
    pub fn display_linear(self) -> Self {
        self.push("display", Display::Linear)
    }

    /// Sets `display: relative` — Lynx relative-positioning container.
    pub fn display_relative(self) -> Self {
        self.push("display", Display::Relative)
    }

    /// Sets `direction`. Lynx default: `ltr`.
    /// <https://lynxjs.org/api/css/properties/direction>
    pub fn direction(self, v: Direction) -> Self {
        self.push("direction", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::keyword::{Direction, Display};

    #[test]
    fn display_keyword() {
        let s = Css::new().display(Display::Flex);
        assert_eq!(s.to_string(), "display: flex;");
    }

    #[test]
    fn display_shortcuts() {
        assert_eq!(Css::new().display_none().to_string(), "display: none;");
        assert_eq!(Css::new().display_flex().to_string(), "display: flex;");
        assert_eq!(Css::new().display_grid().to_string(), "display: grid;");
        assert_eq!(Css::new().display_linear().to_string(), "display: linear;");
        assert_eq!(
            Css::new().display_relative().to_string(),
            "display: relative;"
        );
    }

    #[test]
    fn direction_keywords() {
        assert_eq!(
            Css::new().direction(Direction::Ltr).to_string(),
            "direction: ltr;"
        );
        assert_eq!(
            Css::new().direction(Direction::Rtl).to_string(),
            "direction: rtl;"
        );
    }
}
