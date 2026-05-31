//! `<length-percentage>` — accepts either a `<length>` or a
//! `<percentage>`, optionally wrapped in `calc()`.
//!
//! Lynx reference:
//! <https://lynxjs.org/api/css/data-type/length-percentage.html>
//!
//! Lynx's `calc()` follows the standard CSS arithmetic: `+`, `-`,
//! `*`, `/` with the operand-type constraints
//!
//! - `+` / `-`: both operands must be length-percentage,
//! - `*`: one operand must be a unit-less number,
//! - `/`: the right operand must be a unit-less number.
//!
//! Modeled here as a small expression tree so the same enum can
//! represent both flat values and deeply nested arithmetic. The tree
//! is hand-written rather than parsed because every constructor in
//! this crate is statically typed.

use core::fmt;

use crate::to_css::{write_number, ToCss};

use super::{Length, Percentage};

/// A CSS `<length-percentage>` value.
#[derive(Clone, Debug, PartialEq)]
pub enum LengthPercentage {
    /// A bare length such as `12px` or `1rem`.
    Length(Length),
    /// A bare percentage such as `50%`.
    Percentage(Percentage),
    /// A `calc()` expression.
    Calc(Box<CalcExpr>),
}

impl LengthPercentage {
    /// Wrap a `<length>`.
    pub const fn length(l: Length) -> Self {
        Self::Length(l)
    }

    /// Wrap a `<percentage>`.
    pub const fn percentage(p: Percentage) -> Self {
        Self::Percentage(p)
    }

    /// Wrap a [`CalcExpr`] in a `calc(...)` value.
    pub fn calc(expr: CalcExpr) -> Self {
        Self::Calc(Box::new(expr))
    }
}

impl From<Length> for LengthPercentage {
    fn from(l: Length) -> Self {
        Self::Length(l)
    }
}

impl From<Percentage> for LengthPercentage {
    fn from(p: Percentage) -> Self {
        Self::Percentage(p)
    }
}

impl ToCss for LengthPercentage {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            LengthPercentage::Length(l) => l.to_css(dest),
            LengthPercentage::Percentage(p) => p.to_css(dest),
            LengthPercentage::Calc(c) => {
                dest.write_str("calc(")?;
                c.to_css(dest)?;
                dest.write_char(')')
            }
        }
    }
}

/// A `calc()` expression node.
///
/// The tree is built bottom-up: leaves are length-percentage values
/// or unit-less numbers; inner nodes apply one of the four
/// arithmetic operators. Operator precedence and parenthesization
/// are handled by the serializer.
#[derive(Clone, Debug, PartialEq)]
pub enum CalcExpr {
    /// Length-percentage leaf.
    Value(LengthPercentage),
    /// Unit-less number leaf (used as a multiplier or divisor).
    Number(f32),
    /// `<lhs> + <rhs>`.
    Add(Box<CalcExpr>, Box<CalcExpr>),
    /// `<lhs> - <rhs>`.
    Sub(Box<CalcExpr>, Box<CalcExpr>),
    /// `<lhs> * <rhs>`.
    Mul(Box<CalcExpr>, Box<CalcExpr>),
    /// `<lhs> / <rhs>`.
    Div(Box<CalcExpr>, Box<CalcExpr>),
}

impl CalcExpr {
    /// Length-percentage leaf.
    pub fn value(v: impl Into<LengthPercentage>) -> Self {
        Self::Value(v.into())
    }

    /// Unit-less number leaf.
    pub fn number(v: f32) -> Self {
        Self::Number(v)
    }

    /// `self + rhs`.
    pub fn add(self, rhs: CalcExpr) -> Self {
        Self::Add(Box::new(self), Box::new(rhs))
    }

    /// `self - rhs`.
    pub fn sub(self, rhs: CalcExpr) -> Self {
        Self::Sub(Box::new(self), Box::new(rhs))
    }

    /// `self * rhs`.
    pub fn mul(self, rhs: CalcExpr) -> Self {
        Self::Mul(Box::new(self), Box::new(rhs))
    }

    /// `self / rhs`.
    pub fn div(self, rhs: CalcExpr) -> Self {
        Self::Div(Box::new(self), Box::new(rhs))
    }

    fn precedence(&self) -> u8 {
        match self {
            CalcExpr::Value(_) | CalcExpr::Number(_) => 3,
            CalcExpr::Mul(..) | CalcExpr::Div(..) => 2,
            CalcExpr::Add(..) | CalcExpr::Sub(..) => 1,
        }
    }

    fn write_child(&self, parent: u8, dest: &mut dyn fmt::Write) -> fmt::Result {
        if self.precedence() < parent {
            dest.write_char('(')?;
            self.to_css(dest)?;
            dest.write_char(')')
        } else {
            self.to_css(dest)
        }
    }
}

impl ToCss for CalcExpr {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            CalcExpr::Value(v) => v.to_css(dest),
            CalcExpr::Number(n) => write_number(dest, *n),
            CalcExpr::Add(a, b) => {
                a.write_child(1, dest)?;
                dest.write_str(" + ")?;
                b.write_child(1, dest)
            }
            CalcExpr::Sub(a, b) => {
                a.write_child(1, dest)?;
                dest.write_str(" - ")?;
                b.write_child(2, dest)
            }
            CalcExpr::Mul(a, b) => {
                a.write_child(2, dest)?;
                dest.write_str(" * ")?;
                b.write_child(2, dest)
            }
            CalcExpr::Div(a, b) => {
                a.write_child(2, dest)?;
                dest.write_str(" / ")?;
                b.write_child(3, dest)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_length_serializes() {
        let v = LengthPercentage::length(Length::Px(12.0));
        assert_eq!(v.to_css_string(), "12px");
    }

    #[test]
    fn flat_percentage_serializes() {
        let v = LengthPercentage::percentage(Percentage(50.0));
        assert_eq!(v.to_css_string(), "50%");
    }

    #[test]
    fn from_impls_convert() {
        let l: LengthPercentage = Length::Px(4.0).into();
        let p: LengthPercentage = Percentage(25.0).into();
        assert_eq!(l.to_css_string(), "4px");
        assert_eq!(p.to_css_string(), "25%");
    }

    #[test]
    fn calc_add_two_lengths() {
        let expr = CalcExpr::value(Length::Px(10.0)).add(CalcExpr::value(Percentage(50.0)));
        assert_eq!(LengthPercentage::calc(expr).to_css_string(), "calc(10px + 50%)");
    }

    #[test]
    fn calc_mixed_precedence_keeps_parentheses() {
        // (10px + 20px) * 2 — parens around the sum are required
        // because `+` has lower precedence than `*`.
        let expr = CalcExpr::value(Length::Px(10.0))
            .add(CalcExpr::value(Length::Px(20.0)))
            .mul(CalcExpr::number(2.0));
        assert_eq!(
            LengthPercentage::calc(expr).to_css_string(),
            "calc((10px + 20px) * 2)"
        );
    }

    #[test]
    fn calc_div_right_assoc_parens() {
        // 100% / (2 / 4) keeps the inner division grouped.
        let inner = CalcExpr::number(2.0).div(CalcExpr::number(4.0));
        let expr = CalcExpr::value(Percentage(100.0)).div(inner);
        assert_eq!(
            LengthPercentage::calc(expr).to_css_string(),
            "calc(100% / (2 / 4))"
        );
    }

    #[test]
    fn calc_sub_right_assoc_parens() {
        // 100% - (10px - 5px): subtraction requires parens on the
        // right operand to preserve associativity.
        let inner = CalcExpr::value(Length::Px(10.0)).sub(CalcExpr::value(Length::Px(5.0)));
        let expr = CalcExpr::value(Percentage(100.0)).sub(inner);
        assert_eq!(
            LengthPercentage::calc(expr).to_css_string(),
            "calc(100% - (10px - 5px))"
        );
    }
}
