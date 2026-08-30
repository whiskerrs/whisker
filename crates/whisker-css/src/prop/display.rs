//! `display` and `direction` properties.

use crate::css::Css;
use crate::keyword::{Direction, Display};

impl Css {
    /// Sets `display`. Whisker defaults to flex layout.
    pub fn display(self, v: Display) -> Self {
        self.push_typed(crate::StyleProperty::Display, v)
    }

    /// Sets `display: none` — element is removed from the layout tree.
    pub fn display_none(self) -> Self {
        self.push_typed(crate::StyleProperty::Display, Display::None)
    }

    /// Sets `display: flex` — opt into CSS flexbox.
    pub fn display_flex(self) -> Self {
        self.push_typed(crate::StyleProperty::Display, Display::Flex)
    }

    /// Sets `display: grid` — opt into CSS grid.
    pub fn display_grid(self) -> Self {
        self.push_typed(crate::StyleProperty::Display, Display::Grid)
    }

    /// Sets `display: block`.
    pub fn display_block(self) -> Self {
        self.push_typed(crate::StyleProperty::Display, Display::Block)
    }

    /// Sets `display: flow-root` to establish an independent block formatting context.
    pub fn display_flow_root(self) -> Self {
        self.push_typed(crate::StyleProperty::Display, Display::FlowRoot)
    }

    /// Sets `direction`. Default: `ltr`.
    pub fn direction(self, v: Direction) -> Self {
        self.push_typed(crate::StyleProperty::Direction, v)
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
        assert_eq!(Css::new().display_block().to_string(), "display: block;");
        assert_eq!(
            Css::new().display_flow_root().to_string(),
            "display: flow-root;"
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
