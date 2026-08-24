//! CSS Grid properties.

use crate::css::Css;
use crate::keyword::{AlignItems, AlignSelf, GridAutoFlow};
use crate::style_value::grid_auto_tracks;
use crate::to_css::ToCss;
use crate::value::{GridLine, GridTemplate, GridTemplateAreas, GridTrack};

impl Css {
    /// Sets `grid-template-rows` — track-sizing along the block axis.
    /// <https://lynxjs.org/api/css/properties/grid-template-rows>
    pub fn grid_template_rows(self, v: GridTemplate) -> Self {
        self.push_typed(crate::StyleProperty::GridTemplateRows, v)
    }

    /// Sets `grid-template-columns` — track-sizing along the inline axis.
    /// <https://lynxjs.org/api/css/properties/grid-template-columns>
    pub fn grid_template_columns(self, v: GridTemplate) -> Self {
        self.push_typed(crate::StyleProperty::GridTemplateColumns, v)
    }

    /// Sets named rectangular Grid regions.
    pub fn grid_template_areas(self, v: GridTemplateAreas) -> Self {
        self.push_typed(crate::StyleProperty::GridTemplateAreas, v)
    }

    /// Sets `grid-auto-rows`.
    /// <https://lynxjs.org/api/css/properties/grid-auto-rows>
    pub fn grid_auto_rows<T: Into<GridTrack>>(self, v: impl IntoIterator<Item = T>) -> Self {
        let template = GridTemplate::tracks(v);
        let css = template.to_css_string();
        self.push_semantic(
            crate::StyleProperty::GridAutoRows,
            grid_auto_tracks(&template),
            css,
        )
    }

    /// Sets `grid-auto-columns`.
    /// <https://lynxjs.org/api/css/properties/grid-auto-columns>
    pub fn grid_auto_columns<T: Into<GridTrack>>(self, v: impl IntoIterator<Item = T>) -> Self {
        let template = GridTemplate::tracks(v);
        let css = template.to_css_string();
        self.push_semantic(
            crate::StyleProperty::GridAutoColumns,
            grid_auto_tracks(&template),
            css,
        )
    }

    /// Sets `grid-auto-flow`.
    /// <https://lynxjs.org/api/css/properties/grid-auto-flow>
    pub fn grid_auto_flow(self, v: GridAutoFlow) -> Self {
        self.push_typed(crate::StyleProperty::GridAutoFlow, v)
    }

    /// Sets `grid-row-start`.
    /// <https://lynxjs.org/api/css/properties/grid-row-start>
    pub fn grid_row_start(self, v: GridLine) -> Self {
        self.push_typed(crate::StyleProperty::GridRowStart, v)
    }

    /// Sets `grid-row-end`.
    /// <https://lynxjs.org/api/css/properties/grid-row-end>
    pub fn grid_row_end(self, v: GridLine) -> Self {
        self.push_typed(crate::StyleProperty::GridRowEnd, v)
    }

    /// Sets `grid-column-start`.
    /// <https://lynxjs.org/api/css/properties/grid-column-start>
    pub fn grid_column_start(self, v: GridLine) -> Self {
        self.push_typed(crate::StyleProperty::GridColumnStart, v)
    }

    /// Sets `grid-column-end`.
    /// <https://lynxjs.org/api/css/properties/grid-column-end>
    pub fn grid_column_end(self, v: GridLine) -> Self {
        self.push_typed(crate::StyleProperty::GridColumnEnd, v)
    }

    /// Sets both `grid-row-start` and `grid-row-end`.
    pub fn grid_row(self, start: GridLine, end: GridLine) -> Self {
        self.grid_row_start(start).grid_row_end(end)
    }

    /// Sets both `grid-column-start` and `grid-column-end`.
    pub fn grid_column(self, start: GridLine, end: GridLine) -> Self {
        self.grid_column_start(start).grid_column_end(end)
    }

    /// Sets Grid inline-axis alignment for all children.
    pub fn justify_items(self, v: AlignItems) -> Self {
        self.push_typed(crate::StyleProperty::JustifyItems, v)
    }

    /// Sets Grid inline-axis alignment for this item.
    pub fn justify_self(self, v: AlignSelf) -> Self {
        self.push_typed(crate::StyleProperty::JustifySelf, v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::ext::*;
    use crate::keyword::GridAutoFlow;
    use crate::value::{
        GridArea, GridLine, GridRepeatCount, GridTemplate, GridTemplateAreas, GridTrack,
        GridTrackMax, GridTrackMin,
    };

    #[test]
    fn template_rows_and_columns() {
        let s = Css::new()
            .grid_template_rows(GridTemplate::tracks([
                GridTrack::auto(),
                GridTrack::fraction(1.0),
            ]))
            .grid_template_columns(GridTemplate::tracks([
                GridTrack::fraction(1.0),
                GridTrack::fraction(2.0),
            ]));
        assert_eq!(
            s.to_string(),
            "grid-template-rows: auto 1fr; grid-template-columns: 1fr 2fr;"
        );
    }

    #[test]
    fn auto_rows_columns_flow() {
        let s = Css::new()
            .grid_auto_rows([GridTrack::minmax(px(100).into(), GridTrackMax::Auto)])
            .grid_auto_columns([GridTrack::fixed(px(50))])
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
    fn semantic_grid_values_are_available_without_css_parsing() {
        let style = Css::new()
            .grid_template_columns(GridTemplate::tracks([
                GridTrack::fraction(1.0),
                GridTrack::minmax(GridTrackMin::MinContent, GridTrackMax::Fraction(2.0)),
            ]))
            .grid_row(GridLine::Number(1), GridLine::Span(2))
            .to_specified_style()
            .expect("all grid declarations should be typed");
        let value = |property| {
            style
                .declarations()
                .find(|declaration| declaration.property() == property)
                .map(|declaration| declaration.value())
        };
        assert!(matches!(
            value(crate::StyleProperty::GridTemplateColumns),
            Some(whisker_style::StyleValue::GridTemplate(_))
        ));
        assert!(matches!(
            value(crate::StyleProperty::GridRowEnd),
            Some(whisker_style::StyleValue::GridPlacement(_))
        ));
    }

    #[test]
    fn named_areas_are_typed_and_serialize_as_css_rows() {
        let areas = GridTemplateAreas::new(2, 2)
            .area(GridArea::new("header", 0, 1, 0, 2))
            .area(GridArea::new("main", 1, 2, 1, 2));
        let css = Css::new().grid_template_areas(areas);
        assert_eq!(
            css.to_string(),
            "grid-template-areas: \"header header\" \". main\";"
        );
        css.to_specified_style()
            .expect("grid-template-areas should not require CSS parsing");
    }

    #[test]
    fn repeat_and_named_lines_remain_semantic() {
        let template = GridTemplate::repeat(
            GridRepeatCount::AutoFit,
            [GridTrack::minmax(
                GridTrackMin::Fixed(px(100).into()),
                GridTrackMax::Fraction(1.0),
            )],
        )
        .line_names([["content-start"], ["content-end"]]);
        let css = Css::new().grid_template_columns(template);
        assert_eq!(
            css.to_string(),
            "grid-template-columns: [content-start] repeat(auto-fit, minmax(100px, 1fr)) [content-end];"
        );
        let specified = css
            .to_specified_style()
            .expect("repeat() should not require CSS parsing");
        assert!(matches!(
            specified.declarations().next().map(|value| value.value()),
            Some(whisker_style::StyleValue::GridTemplate(template))
                if matches!(template.components.as_slice(), [whisker_style::GridTemplateComponentValue::Repeat(_)])
        ));
    }
}
