use std::sync::OnceLock;

use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use super::model::{Span, SpanStyle};

pub(super) fn spans(source: &[Span], language: Option<&str>) -> Vec<Span> {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);
    let themes = THEMES.get_or_init(ThemeSet::load_defaults);
    let Some(syntax) = language.and_then(|token| syntaxes.find_syntax_by_token(token)) else {
        return source.to_vec();
    };
    let Some(theme) = themes.themes.get("base16-ocean.dark") else {
        return source.to_vec();
    };
    let text: String = source.iter().map(|span| span.text.as_str()).collect();
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut spans: Vec<Span> = Vec::new();
    for line in LinesWithEndings::from(text.as_str()) {
        let Ok(tokens) = highlighter.highlight_line(line, syntaxes) else {
            return source.to_vec();
        };
        for (style, text) in tokens {
            let style = SpanStyle {
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
                foreground: Some(
                    (u32::from(style.foreground.r) << 16)
                        | (u32::from(style.foreground.g) << 8)
                        | u32::from(style.foreground.b),
                ),
                ..SpanStyle::default()
            };
            if let Some(last) = spans.last_mut().filter(|last| last.style == style) {
                last.text.push_str(text);
            } else {
                spans.push(Span {
                    text: text.into(),
                    style,
                });
            }
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlighted_code_preserves_whitespace_and_uses_multiple_colors() {
        let text = "fn main() {\n    println!(\"Hello 🦀\");\n}\n";
        let source = [Span {
            text: text.into(),
            style: SpanStyle::default(),
        }];
        let result = spans(&source, Some("rust"));
        assert_eq!(
            result
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(
            result
                .iter()
                .filter_map(|span| span.style.foreground)
                .collect::<std::collections::HashSet<_>>()
                .len()
                >= 3
        );
        assert_eq!(spans(&source, Some("unknown-language")), source);
    }
}
