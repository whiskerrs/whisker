//! `flex` shorthand.

use crate::css::Css;
use crate::to_css::number_to_string;
use crate::value::FlexBasis;

/// `flex` shorthand value. CSS allows `flex: <grow> <shrink>?
/// <basis>?` plus the keywords `none`, `auto`, `initial`. This enum
/// covers all five forms.
#[derive(Clone, Debug, PartialEq)]
pub enum Flex {
    /// `none` — equivalent to `0 0 auto`.
    None,
    /// `auto` — equivalent to `1 1 auto`.
    Auto,
    /// `initial` — equivalent to `0 1 auto`.
    Initial,
    /// `<number>` — equivalent to `<number> 1 0%`.
    Number(f32),
    /// Explicit triple `grow shrink basis`.
    Full {
        /// `flex-grow`.
        grow: f32,
        /// `flex-shrink`.
        shrink: f32,
        /// `flex-basis`.
        basis: FlexBasis,
    },
}

impl Flex {
    /// Expand to `(grow, shrink, basis)`.
    pub fn expand(self) -> (f32, f32, FlexBasis) {
        match self {
            Flex::None => (0.0, 0.0, FlexBasis::Auto),
            Flex::Auto => (1.0, 1.0, FlexBasis::Auto),
            Flex::Initial => (0.0, 1.0, FlexBasis::Auto),
            Flex::Number(n) => (
                n,
                1.0,
                FlexBasis::LengthPercentage(crate::data_type::Percentage(0.0).into()),
            ),
            Flex::Full {
                grow,
                shrink,
                basis,
            } => (grow, shrink, basis),
        }
    }
}

impl Css {
    /// Sets `flex` shorthand — expands to `flex-grow`, `flex-shrink`,
    /// `flex-basis`.
    /// <https://lynxjs.org/api/css/properties/flex>
    pub fn flex(self, v: Flex) -> Self {
        let (grow, shrink, basis) = v.expand();
        self.push_raw("flex-grow", number_to_string(grow))
            .push_raw("flex-shrink", number_to_string(shrink))
            .push("flex-basis", basis)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::ext::*;
    use crate::value::FlexBasis;

    use super::*;

    #[test]
    fn flex_none() {
        let s = Css::new().flex(Flex::None);
        assert_eq!(
            s.to_string(),
            "flex-grow: 0; flex-shrink: 0; flex-basis: auto;"
        );
    }

    #[test]
    fn flex_auto() {
        let s = Css::new().flex(Flex::Auto);
        assert_eq!(
            s.to_string(),
            "flex-grow: 1; flex-shrink: 1; flex-basis: auto;"
        );
    }

    #[test]
    fn flex_initial() {
        let s = Css::new().flex(Flex::Initial);
        assert_eq!(
            s.to_string(),
            "flex-grow: 0; flex-shrink: 1; flex-basis: auto;"
        );
    }

    #[test]
    fn flex_number() {
        let s = Css::new().flex(Flex::Number(2.0));
        assert_eq!(
            s.to_string(),
            "flex-grow: 2; flex-shrink: 1; flex-basis: 0%;"
        );
    }

    #[test]
    fn flex_full_triple() {
        let s = Css::new().flex(Flex::Full {
            grow: 1.5,
            shrink: 0.5,
            basis: FlexBasis::LengthPercentage(px(100).into()),
        });
        assert_eq!(
            s.to_string(),
            "flex-grow: 1.5; flex-shrink: 0.5; flex-basis: 100px;"
        );
    }

    #[test]
    fn flex_then_grow_override() {
        let s = Css::new().flex(Flex::Auto).flex_grow(3.0);
        assert_eq!(
            s.to_string(),
            "flex-shrink: 1; flex-basis: auto; flex-grow: 3;"
        );
    }
}
