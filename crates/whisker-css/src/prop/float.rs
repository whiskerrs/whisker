//! CSS block formatting-context float properties.

use crate::css::Css;
use crate::keyword::{Clear, Float};

impl Css {
    /// Sets the physical side to which this block floats.
    pub fn float(self, value: Float) -> Self {
        self.push_typed(crate::StyleProperty::Float, value)
    }

    /// Sets which preceding floats this block clears.
    pub fn clear(self, value: Clear) -> Self {
        self.push_typed(crate::StyleProperty::Clear, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_and_clear_are_typed() {
        let css = Css::new().float(Float::Left).clear(Clear::Both);
        assert_eq!(css.to_string(), "float: left; clear: both;");
        css.to_specified_style();
    }
}
