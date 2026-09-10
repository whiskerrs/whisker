use super::theme::{self, size, space};
use whisker::css::{Cursor, FontStyle, FontWeight, TextDecorationLine, TextDecorationStyle};
use whisker::prelude::*;

mod highlight;
mod model;
mod table;
use model::{Block, Kind, Span, blocks};

#[component]
pub fn markdown(text: Signal<String>) -> Element {
    render! {
        View(style: theme::column().gap(px(space::MD))) {
            ForEach(
                each: move || blocks(&text.get()),
                key: |block: &Block| block.clone(),
                children: paragraph,
            )
        }
    }
}

fn paragraph(mut block: Block) -> Element {
    if let Some(table) = block.table {
        return table::render(table);
    }
    if block.kind == Kind::Code {
        block.spans = highlight::spans(&block.spans, block.language.as_deref());
    }
    Text::builder()
        .selectable(true)
        .style(theme::style(move |palette| match block.kind {
            Kind::Body => palette.text(size::BODY),
            Kind::Heading => palette.title().margin_top(px(space::SM)),
            Kind::Code => palette
                .text(14.0)
                .white_space(whisker::css::WhiteSpace::PreWrap)
                .font_family("monospace")
                .background_color(Color::hex(palette.code))
                .color(Color::hex(palette.on_code))
                .padding(px(space::LG))
                .border_radius(px(12)),
            Kind::Quote => palette
                .text(size::BODY)
                .color(Color::hex(palette.muted))
                .border_left_width(px(3))
                .border_left_color(Color::hex(palette.accent))
                .padding_left(px(space::LG)),
        }))
        .body(|body| {
            for span in block.spans {
                body.push(inline(span));
            }
        })
        .build()
}

fn inline(span: Span) -> Element {
    let is_link = span.style.link.is_some();
    let style = theme::style(move |palette| {
        let mut style = Css::new();
        if span.style.bold {
            style = style.font_weight(FontWeight::Bold);
        }
        if span.style.italic {
            style = style.font_style(FontStyle::Italic);
        }
        if span.style.code {
            style = style
                .font_family("monospace")
                .background_color(Color::hex(palette.tint))
                .color(Color::hex(palette.ink))
                .border_radius(px(3));
        }
        if span.style.strike {
            style = style.text_decoration(
                TextDecorationLine::LineThrough,
                TextDecorationStyle::Solid,
                Color::hex(palette.muted),
            );
        }
        if let Some(color) = span.style.foreground {
            style = style.color(Color::hex(color));
        }
        if is_link {
            style = style
                .cursor(Cursor::Pointer)
                .color(Color::hex(palette.accent))
                .text_decoration(
                    TextDecorationLine::Underline,
                    TextDecorationStyle::Solid,
                    Color::hex(palette.accent),
                );
        }
        style
    });
    let mut text = Text::builder().value(span.text);
    if let Some(url) = span.style.link {
        text = text.on_tap(move |_| {
            let url = url.clone();
            spawn_local(async move {
                if let whisker_web_browser::BrowserResult::Error(error) =
                    whisker_web_browser::open_browser_async(&url).await
                {
                    eprintln!("Unable to open link: {error}");
                }
            });
        });
    }
    text.style(style).build()
}
