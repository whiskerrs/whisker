//! Render untrusted Markdown as native text blocks; never execute embedded HTML.
use super::theme::{self, color, size, space};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use whisker::prelude::*;

#[derive(Clone, Debug, PartialEq)]
struct Block {
    id: usize,
    text: String,
    kind: Kind,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Body,
    Heading,
    Code,
    Quote,
}

fn blocks(source: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut kind = Kind::Body;
    let mut list_depth: usize = 0;
    let mut links = Vec::new();
    let flush = |result: &mut Vec<Block>, text: &mut String, kind| {
        if !text.trim().is_empty() {
            result.push(Block {
                id: result.len(),
                text: std::mem::take(text).trim_end().into(),
                kind,
            });
        } else {
            text.clear();
        }
    };
    for event in Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { .. } => {
                    flush(&mut result, &mut text, kind);
                    kind = Kind::Heading;
                }
                Tag::CodeBlock(_) => {
                    flush(&mut result, &mut text, kind);
                    kind = Kind::Code;
                }
                Tag::BlockQuote(_) => {
                    flush(&mut result, &mut text, kind);
                    kind = Kind::Quote;
                }
                Tag::Link { dest_url, .. } => links.push(dest_url.to_string()),
                Tag::List(_) => list_depth += 1,
                Tag::Item => {
                    flush(&mut result, &mut text, kind);
                    text.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                    text.push_str("• ");
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Item => flush(&mut result, &mut text, kind),
                TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::BlockQuote(_) => {
                    flush(&mut result, &mut text, kind);
                    kind = Kind::Body;
                }
                TagEnd::Link => {
                    if let Some(url) = links.pop() {
                        text.push_str(&format!(" ({url})"));
                    }
                }
                TagEnd::List(_) => list_depth = list_depth.saturating_sub(1),
                _ => {}
            },
            Event::Text(value) | Event::Code(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::TaskListMarker(checked) => text.push_str(if checked { "[x] " } else { "[ ] " }),
            Event::Rule => {
                flush(&mut result, &mut text, kind);
                text.push('—');
                flush(&mut result, &mut text, kind);
            }
            _ => {}
        }
    }
    flush(&mut result, &mut text, kind);
    result
}

#[component]
pub fn markdown(text: Signal<String>) -> Element {
    render! {
        View(style: theme::column().gap(px(space::MD))) {
            ForEach(
                each: move || blocks(&text.get()),
                key: |block: &Block| (block.id, block.text.clone()),
                children: |block: Block| render! {
                    Text(
                        value: block.text,
                        style: match block.kind {
                            Kind::Body => theme::text(size::BODY),
                            Kind::Heading => theme::title().margin_top(px(space::SM)),
                            Kind::Code => theme::text(14.0)
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
                        },
                    )
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_headings_lists_fences_and_untrusted_html() {
        let output = blocks(
            "# A title\n\nA **bold** idea.\n\n- One\n- Two\n\n```rust\nlet x = 1;\n```\n\n<script>alert(1)</script>\n",
        );
        assert_eq!(output[0].kind, Kind::Heading);
        assert_eq!(output[1].text, "A bold idea.");
        assert_eq!(output[2].text, "• One");
        assert_eq!(output[4].kind, Kind::Code);
        assert!(!output.iter().any(|b| b.text.contains("alert")));
    }
}
