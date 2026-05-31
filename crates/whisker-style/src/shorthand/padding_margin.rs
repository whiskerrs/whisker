//! `padding` / `margin` shorthand support.
//!
//! Both shorthands take 1–4 length-percentages and expand to the
//! four per-side longhands. `margin` additionally accepts `auto` for
//! centering.

use crate::data_type::LengthPercentage;
use crate::style::Style;

/// Argument to [`Style::padding`]. Built via `impl From` for common
/// shapes:
///
/// | Source                                  | CSS shorthand |
/// |-----------------------------------------|---------------|
/// | `Length` / `Percentage` / `LengthPercentage` | `<v>`         |
/// | `(t_b, l_r)`                            | `<t_b> <l_r>` |
/// | `(t, r_l, b)`                           | `<t> <r_l> <b>` |
/// | `(t, r, b, l)`                          | `<t> <r> <b> <l>` |
#[derive(Clone, Debug, PartialEq)]
pub struct Padding {
    /// Top, right, bottom, left — already expanded to all four sides.
    pub trbl: [LengthPercentage; 4],
}

impl<T: Into<LengthPercentage>> From<T> for Padding {
    fn from(v: T) -> Self {
        let v = v.into();
        Self {
            trbl: [v.clone(), v.clone(), v.clone(), v],
        }
    }
}

// Two-value form: `vertical, horizontal`.
impl<A, B> From<(A, B)> for Padding
where
    A: Into<LengthPercentage>,
    B: Into<LengthPercentage>,
{
    fn from((y, x): (A, B)) -> Self {
        let y = y.into();
        let x = x.into();
        Self {
            trbl: [y.clone(), x.clone(), y, x],
        }
    }
}

// Three-value form: `top, horizontal, bottom`.
impl<A, B, C> From<(A, B, C)> for Padding
where
    A: Into<LengthPercentage>,
    B: Into<LengthPercentage>,
    C: Into<LengthPercentage>,
{
    fn from((t, x, b): (A, B, C)) -> Self {
        let x = x.into();
        Self {
            trbl: [t.into(), x.clone(), b.into(), x],
        }
    }
}

// Four-value form: `top, right, bottom, left`.
impl<A, B, C, D> From<(A, B, C, D)> for Padding
where
    A: Into<LengthPercentage>,
    B: Into<LengthPercentage>,
    C: Into<LengthPercentage>,
    D: Into<LengthPercentage>,
{
    fn from((t, r, b, l): (A, B, C, D)) -> Self {
        Self {
            trbl: [t.into(), r.into(), b.into(), l.into()],
        }
    }
}

/// Per-side value for `margin`. Negative lengths and `auto` are
/// allowed (unlike padding).
#[derive(Clone, Debug, PartialEq)]
pub enum MarginValue {
    /// An explicit length or percentage. Negative values are valid.
    LengthPercentage(LengthPercentage),
    /// `auto` — distribute remaining space (used for centering).
    Auto,
}

impl crate::to_css::ToCss for MarginValue {
    fn to_css(&self, dest: &mut dyn core::fmt::Write) -> core::fmt::Result {
        match self {
            MarginValue::LengthPercentage(lp) => lp.to_css(dest),
            MarginValue::Auto => dest.write_str("auto"),
        }
    }
}

impl From<LengthPercentage> for MarginValue {
    fn from(v: LengthPercentage) -> Self {
        Self::LengthPercentage(v)
    }
}

impl From<crate::data_type::Length> for MarginValue {
    fn from(v: crate::data_type::Length) -> Self {
        Self::LengthPercentage(v.into())
    }
}

impl From<crate::data_type::Percentage> for MarginValue {
    fn from(v: crate::data_type::Percentage) -> Self {
        Self::LengthPercentage(v.into())
    }
}

/// Argument to [`Style::margin`]. Same shape as [`Padding`] but
/// values may be `MarginValue::Auto`.
#[derive(Clone, Debug, PartialEq)]
pub struct Margin {
    /// Top, right, bottom, left — already expanded to all four sides.
    pub trbl: [MarginValue; 4],
}

impl<T: Into<MarginValue>> From<T> for Margin {
    fn from(v: T) -> Self {
        let v = v.into();
        Self {
            trbl: [v.clone(), v.clone(), v.clone(), v],
        }
    }
}

impl<A, B> From<(A, B)> for Margin
where
    A: Into<MarginValue>,
    B: Into<MarginValue>,
{
    fn from((y, x): (A, B)) -> Self {
        let y = y.into();
        let x = x.into();
        Self {
            trbl: [y.clone(), x.clone(), y, x],
        }
    }
}

impl<A, B, C> From<(A, B, C)> for Margin
where
    A: Into<MarginValue>,
    B: Into<MarginValue>,
    C: Into<MarginValue>,
{
    fn from((t, x, b): (A, B, C)) -> Self {
        let x = x.into();
        Self {
            trbl: [t.into(), x.clone(), b.into(), x],
        }
    }
}

impl<A, B, C, D> From<(A, B, C, D)> for Margin
where
    A: Into<MarginValue>,
    B: Into<MarginValue>,
    C: Into<MarginValue>,
    D: Into<MarginValue>,
{
    fn from((t, r, b, l): (A, B, C, D)) -> Self {
        Self {
            trbl: [t.into(), r.into(), b.into(), l.into()],
        }
    }
}

impl Style {
    /// Sets `padding` shorthand. Expands to the four per-side
    /// longhands so later overrides win cleanly. The argument
    /// accepts a single length, a `(y, x)` tuple, a `(t, x, b)`
    /// tuple, or a `(t, r, b, l)` tuple.
    /// <https://lynxjs.org/api/css/properties/padding>
    pub fn padding(self, v: impl Into<Padding>) -> Self {
        let Padding { trbl: [t, r, b, l] } = v.into();
        self.padding_top(t)
            .padding_right(r)
            .padding_bottom(b)
            .padding_left(l)
    }

    /// Sets `margin` shorthand. Same shape rules as
    /// [`Style::padding`], with `auto` allowed per side.
    /// <https://lynxjs.org/api/css/properties/margin>
    pub fn margin(self, v: impl Into<Margin>) -> Self {
        let Margin { trbl: [t, r, b, l] } = v.into();
        self.push("margin-top", t)
            .push("margin-right", r)
            .push("margin-bottom", b)
            .push("margin-left", l)
    }
}

#[cfg(test)]
mod tests {
    use crate::ext::*;
    use crate::shorthand::padding_margin::MarginValue;
    use crate::Style;

    #[test]
    fn padding_single_value_expands_to_four() {
        let s = Style::new().padding(px(8));
        assert_eq!(
            s.to_string(),
            "padding-top: 8px; padding-right: 8px; padding-bottom: 8px; padding-left: 8px;"
        );
    }

    #[test]
    fn padding_two_value_y_x() {
        let s = Style::new().padding((px(8), px(16)));
        assert_eq!(
            s.to_string(),
            "padding-top: 8px; padding-right: 16px; padding-bottom: 8px; padding-left: 16px;"
        );
    }

    #[test]
    fn padding_three_value_t_x_b() {
        let s = Style::new().padding((px(8), px(16), px(4)));
        assert_eq!(
            s.to_string(),
            "padding-top: 8px; padding-right: 16px; padding-bottom: 4px; padding-left: 16px;"
        );
    }

    #[test]
    fn padding_four_value_trbl() {
        let s = Style::new().padding((px(2), px(4), px(6), px(8)));
        assert_eq!(
            s.to_string(),
            "padding-top: 2px; padding-right: 4px; padding-bottom: 6px; padding-left: 8px;"
        );
    }

    #[test]
    fn padding_then_single_side_override() {
        let s = Style::new().padding(px(8)).padding_top(px(0));
        assert_eq!(
            s.to_string(),
            "padding-right: 8px; padding-bottom: 8px; padding-left: 8px; padding-top: 0px;"
        );
    }

    #[test]
    fn padding_percentages() {
        let s = Style::new().padding(50.percent());
        assert_eq!(
            s.to_string(),
            "padding-top: 50%; padding-right: 50%; padding-bottom: 50%; padding-left: 50%;"
        );
    }

    #[test]
    fn margin_single_value() {
        let s = Style::new().margin(px(8));
        assert_eq!(
            s.to_string(),
            "margin-top: 8px; margin-right: 8px; margin-bottom: 8px; margin-left: 8px;"
        );
    }

    #[test]
    fn margin_auto_centers() {
        let s = Style::new().margin((px(0), MarginValue::Auto));
        assert_eq!(
            s.to_string(),
            "margin-top: 0px; margin-right: auto; margin-bottom: 0px; margin-left: auto;"
        );
    }

    #[test]
    fn margin_four_value_with_negative() {
        let s = Style::new().margin((px(-4), px(0), px(4), MarginValue::Auto));
        assert_eq!(
            s.to_string(),
            "margin-top: -4px; margin-right: 0px; margin-bottom: 4px; margin-left: auto;"
        );
    }

    #[test]
    fn margin_three_value_t_x_b() {
        let s = Style::new().margin((px(2), px(4), px(6)));
        assert_eq!(
            s.to_string(),
            "margin-top: 2px; margin-right: 4px; margin-bottom: 6px; margin-left: 4px;"
        );
    }

    #[test]
    fn margin_value_from_length_percentage() {
        let v: MarginValue = px(4).into();
        let v2: MarginValue = 25.percent().into();
        let v3: MarginValue = crate::data_type::LengthPercentage::Length(crate::data_type::Length::Px(8.0)).into();
        assert_eq!(matches!(v, MarginValue::LengthPercentage(_)), true);
        assert_eq!(matches!(v2, MarginValue::LengthPercentage(_)), true);
        assert_eq!(matches!(v3, MarginValue::LengthPercentage(_)), true);
    }
}
