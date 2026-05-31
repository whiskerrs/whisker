//! CSS Grid properties.

use crate::css::Css;
use crate::data_type::LengthPercentage;
use crate::keyword::GridAutoFlow;
use crate::value::{GridLine, GridTemplate};

impl Css {
    /// Sets `grid-template-rows` — track-sizing along the block axis.
    /// <https://lynxjs.org/api/css/properties/grid-template-rows>
    pub fn grid_template_rows(self, v: GridTemplate) -> Self {
        self.push("grid-template-rows", v)
    }

    /// Sets `grid-template-columns` — track-sizing along the inline axis.
    /// <https://lynxjs.org/api/css/properties/grid-template-columns>
    pub fn grid_template_columns(self, v: GridTemplate) -> Self {
        self.push("grid-template-columns", v)
    }

    /// Sets `grid-auto-rows`.
    /// <https://lynxjs.org/api/css/properties/grid-auto-rows>
    pub fn grid_auto_rows(self, v: GridTemplate) -> Self {
        self.push("grid-auto-rows", v)
    }

    /// Sets `grid-auto-columns`.
    /// <https://lynxjs.org/api/css/properties/grid-auto-columns>
    pub fn grid_auto_columns(self, v: GridTemplate) -> Self {
        self.push("grid-auto-columns", v)
    }

    /// Sets `grid-auto-flow`.
    /// <https://lynxjs.org/api/css/properties/grid-auto-flow>
    pub fn grid_auto_flow(self, v: GridAutoFlow) -> Self {
        self.push("grid-auto-flow", v)
    }

    /// Sets `grid-row-start`.
    /// <https://lynxjs.org/api/css/properties/grid-row-start>
    pub fn grid_row_start(self, v: GridLine) -> Self {
        self.push("grid-row-start", v)
    }

    /// Sets `grid-row-end`.
    /// <https://lynxjs.org/api/css/properties/grid-row-end>
    pub fn grid_row_end(self, v: GridLine) -> Self {
        self.push("grid-row-end", v)
    }

    /// Sets `grid-column-start`.
    /// <https://lynxjs.org/api/css/properties/grid-column-start>
    pub fn grid_column_start(self, v: GridLine) -> Self {
        self.push("grid-column-start", v)
    }

    /// Sets `grid-column-end`.
    /// <https://lynxjs.org/api/css/properties/grid-column-end>
    pub fn grid_column_end(self, v: GridLine) -> Self {
        self.push("grid-column-end", v)
    }

    /// Sets `grid-row-gap` (legacy alias for `row-gap`).
    /// <https://lynxjs.org/api/css/properties/grid-row-gap>
    pub fn grid_row_gap(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("grid-row-gap", v.into())
    }

    /// Sets `grid-column-gap` (legacy alias for `column-gap`).
    /// <https://lynxjs.org/api/css/properties/grid-column-gap>
    pub fn grid_column_gap(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("grid-column-gap", v.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ext::*;
    use crate::keyword::GridAutoFlow;
    use crate::value::{GridLine, GridTemplate};
    use crate::Css;

    #[test]
    fn template_rows_and_columns() {
        let s = Css::new()
            .grid_template_rows(GridTemplate::tracks(["auto", "1fr"]))
            .grid_template_columns(GridTemplate::tracks(["1fr", "2fr"]));
        assert_eq!(
            s.to_string(),
            "grid-template-rows: auto 1fr; grid-template-columns: 1fr 2fr;"
        );
    }

    #[test]
    fn auto_rows_columns_flow() {
        let s = Css::new()
            .grid_auto_rows(GridTemplate::tracks(["minmax(100px, auto)"]))
            .grid_auto_columns(GridTemplate::tracks(["50px"]))
            .grid_auto_flow(GridAutoFlow::ColumnDense);
        assert_eq!(
            s.to_string(),
            "grid-auto-rows: minmax(100px, auto); grid-auto-columns: 50px; grid-auto-flow: column dense;"
        );
    }

    #[test]
    fn grid_lines_for_item() {
        let s = Css::new()
            .grid_row_start(GridLine::Number(1))
            .grid_row_end(GridLine::Span(2))
            .grid_column_start(GridLine::Auto)
            .grid_column_end(GridLine::Number(-1));
        assert_eq!(
            s.to_string(),
            "grid-row-start: 1; grid-row-end: span 2; grid-column-start: auto; grid-column-end: -1;"
        );
    }

    #[test]
    fn grid_gaps_legacy() {
        let s = Css::new().grid_row_gap(px(8)).grid_column_gap(px(12));
        assert_eq!(s.to_string(), "grid-row-gap: 8px; grid-column-gap: 12px;");
    }
}
