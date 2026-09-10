use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Block {
    pub id: usize,
    pub kind: Kind,
    pub spans: Vec<Span>,
    pub language: Option<String>,
    pub table: Option<Table>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Table {
    pub alignments: Vec<CellAlignment>,
    pub rows: Vec<Vec<Vec<Span>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum CellAlignment {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    #[default]
    Body,
    Heading,
    Code,
    Quote,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Span {
    pub text: String,
    pub style: SpanStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
    pub foreground: Option<u32>,
}

#[derive(Default)]
struct Blocks {
    result: Vec<Block>,
    spans: Vec<Span>,
    kinds: Vec<Kind>,
    styles: Vec<SpanStyle>,
    lists: Vec<Option<u64>>,
    language: Option<String>,
    table: Option<Table>,
    row: Vec<Vec<Span>>,
}

impl Blocks {
    fn append(&mut self, text: &str, code: bool) {
        let mut style = self.styles.last().cloned().unwrap_or_default();
        style.code |= code;
        if let Some(last) = self.spans.last_mut().filter(|span| span.style == style) {
            last.text.push_str(text);
        } else if !text.is_empty() {
            self.spans.push(Span {
                text: text.into(),
                style,
            });
        }
    }

    fn flush(&mut self) {
        if self.spans.iter().any(|span| !span.text.trim().is_empty()) {
            self.result.push(Block {
                id: self.result.len(),
                kind: self.kinds.last().copied().unwrap_or_default(),
                spans: std::mem::take(&mut self.spans),
                language: self.language.clone(),
                table: None,
            });
        } else {
            self.spans.clear();
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Table(alignments) => {
                self.flush();
                self.table = Some(Table {
                    alignments: alignments
                        .into_iter()
                        .map(|alignment| match alignment {
                            pulldown_cmark::Alignment::Center => CellAlignment::Center,
                            pulldown_cmark::Alignment::Right => CellAlignment::End,
                            _ => CellAlignment::Start,
                        })
                        .collect(),
                    rows: Vec::new(),
                });
            }
            Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::BlockQuote(_) => {
                self.flush();
                if let Tag::CodeBlock(CodeBlockKind::Fenced(info)) = &tag {
                    self.language = info.split_whitespace().next().map(str::to_owned);
                }
                self.kinds.push(match tag {
                    Tag::Heading { .. } => Kind::Heading,
                    Tag::CodeBlock(_) => Kind::Code,
                    _ => Kind::Quote,
                });
            }
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } => {
                let mut style = self.styles.last().cloned().unwrap_or_default();
                match tag {
                    Tag::Emphasis => style.italic = true,
                    Tag::Strong => style.bold = true,
                    Tag::Strikethrough => style.strike = true,
                    Tag::Link { dest_url, .. } => style.link = browser_url(&dest_url),
                    _ => unreachable!(),
                }
                self.styles.push(style);
            }
            Tag::List(start) => self.lists.push(start),
            Tag::Item => {
                self.flush();
                let indentation = "  ".repeat(self.lists.len().saturating_sub(1));
                let marker = match self.lists.last_mut() {
                    Some(Some(next)) => {
                        let marker = format!("{next}. ");
                        *next = next.saturating_add(1);
                        marker
                    }
                    _ => "• ".into(),
                };
                self.append(&format!("{indentation}{marker}"), false);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::TableCell => self.row.push(std::mem::take(&mut self.spans)),
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(std::mem::take(&mut self.row));
                }
            }
            TagEnd::Table => {
                self.result.push(Block {
                    id: self.result.len(),
                    kind: Kind::Body,
                    spans: Vec::new(),
                    language: None,
                    table: self.table.take(),
                });
            }
            TagEnd::Paragraph | TagEnd::Item => self.flush(),
            TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::BlockQuote(_) => {
                self.flush();
                self.kinds.pop();
                if tag == TagEnd::CodeBlock {
                    self.language = None;
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            _ => {}
        }
    }
}

pub(super) fn blocks(source: &str) -> Vec<Block> {
    let mut blocks = Blocks::default();
    for event in Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES,
    ) {
        match event {
            Event::Start(tag) => blocks.start(tag),
            Event::End(tag) => blocks.end(tag),
            Event::Text(text) => blocks.append(&text, false),
            Event::Code(text) => blocks.append(&text, true),
            Event::SoftBreak => blocks.append(" ", false),
            Event::HardBreak => blocks.append("\n", false),
            Event::TaskListMarker(checked) => {
                blocks.append(if checked { "[x] " } else { "[ ] " }, false)
            }
            Event::Rule => {
                blocks.flush();
                blocks.append("—", false);
                blocks.flush();
            }
            _ => {}
        }
    }
    blocks.flush();
    blocks.result
}

fn browser_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    matches!(url.scheme(), "https" | "http").then(|| url.into())
}

#[cfg(test)]
mod tests;
