use super::theme::{self, color, size, space};
use whisker::css::{FontStyle, FontWeight, TextDecorationLine, TextDecorationStyle};
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
        .style(match block.kind {
            Kind::Body => theme::text(size::BODY),
            Kind::Heading => theme::title().margin_top(px(space::SM)),
            Kind::Code => theme::text(14.0)
                .white_space(whisker::css::WhiteSpace::PreWrap)
                .font_family("monospace")
                .background_color(Color::hex(color::CODE))
                .color(Color::hex(color::ON_CODE))
                .padding(px(space::LG))
                .border_radius(px(12)),
            Kind::Quote => theme::text(size::BODY)
                .color(Color::hex(color::MUTED))
                .border_left_width(px(3))
                .border_left_color(Color::hex(color::ACCENT))
                .padding_left(px(space::LG)),
        })
        .body(|body| {
            for span in block.spans {
                body.push(inline(span));
            }
        })
        .build()
}

fn inline(span: Span) -> Element {
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
            .background_color(Color::hex(color::TINT))
            .color(Color::hex(color::INK))
            .border_radius(px(3));
    }
    if span.style.strike {
        style = style.text_decoration(
            TextDecorationLine::LineThrough,
            TextDecorationStyle::Solid,
            Color::hex(color::MUTED),
        );
    }
    if let Some(color) = span.style.foreground {
        style = style.color(Color::hex(color));
    }
    let mut text = Text::builder().value(span.text);
    if let Some(url) = span.style.link {
        style = style.color(Color::hex(color::ACCENT)).text_decoration(
            TextDecorationLine::Underline,
            TextDecorationStyle::Solid,
            Color::hex(color::ACCENT),
        );
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
