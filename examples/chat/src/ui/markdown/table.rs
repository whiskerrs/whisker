use whisker::css::{FontWeight, TextAlign};
use whisker::prelude::*;

use super::{
    color, inline,
    model::{CellAlignment, Table},
    size, space, theme,
};

pub(super) fn render(table: Table) -> Element {
    ScrollView::builder()
        .axis(whisker::ScrollAxis::Horizontal)
        .style(Css::new().max_width(percent(100)).flex_shrink(0.0))
        .body(|body| {
            body.push(
                View::builder()
                    .style(theme::column())
                    .body(|body| {
                        for (index, row) in table.rows.into_iter().enumerate() {
                            body.push(
                                View::builder()
                                    .style(theme::row())
                                    .body(|body| {
                                        for (column, spans) in row.into_iter().enumerate() {
                                            let alignment = match table
                                                .alignments
                                                .get(column)
                                                .copied()
                                                .unwrap_or_default()
                                            {
                                                CellAlignment::Start => TextAlign::Left,
                                                CellAlignment::Center => TextAlign::Center,
                                                CellAlignment::End => TextAlign::Right,
                                            };
                                            let mut style = theme::text(size::BODY)
                                                .width(px(180))
                                                .flex_shrink(0.0)
                                                .padding(px(space::MD))
                                                .text_align(alignment)
                                                .border_bottom_width(px(1))
                                                .border_bottom_color(Color::hex(color::TINT));
                                            if index == 0 {
                                                style = style
                                                    .font_weight(FontWeight::Bold)
                                                    .background_color(Color::hex(color::TINT));
                                            }
                                            body.push(
                                                Text::builder()
                                                    .selectable(true)
                                                    .style(style)
                                                    .body(|body| {
                                                        for span in spans {
                                                            body.push(inline(span));
                                                        }
                                                    })
                                                    .build(),
                                            );
                                        }
                                    })
                                    .build(),
                            );
                        }
                    })
                    .build(),
            );
        })
        .build()
}
