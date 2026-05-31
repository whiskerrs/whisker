//! Grid keyword enums.
//!
//! Reference:
//! - <https://lynxjs.org/api/css/properties/grid-auto-flow>

use core::fmt;

use crate::to_css::ToCss;

/// The `grid-auto-flow` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridAutoFlow {
    /// `row` — flow auto-placed items along rows. Default.
    Row,
    /// `column` — flow auto-placed items along columns.
    Column,
    /// `row dense` — flow along rows, back-filling holes.
    RowDense,
    /// `column dense` — flow along columns, back-filling holes.
    ColumnDense,
}

impl ToCss for GridAutoFlow {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            GridAutoFlow::Row => "row",
            GridAutoFlow::Column => "column",
            GridAutoFlow::RowDense => "row dense",
            GridAutoFlow::ColumnDense => "column dense",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants() {
        let cases = [
            (GridAutoFlow::Row, "row"),
            (GridAutoFlow::Column, "column"),
            (GridAutoFlow::RowDense, "row dense"),
            (GridAutoFlow::ColumnDense, "column dense"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }
}
