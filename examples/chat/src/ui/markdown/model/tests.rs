use super::*;

#[test]
fn nested_emphasis_and_code_keep_independent_inline_styles() {
    let blocks = blocks("Plain **bold *both*** and `code`.");
    let spans = &blocks[0].spans;
    assert_eq!(
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "Plain bold both and code."
    );
    assert!(
        spans
            .iter()
            .any(|span| span.text == "bold " && span.style.bold && !span.style.italic)
    );
    assert!(
        spans
            .iter()
            .any(|span| span.text == "both" && span.style.bold && span.style.italic)
    );
    assert!(
        spans
            .iter()
            .any(|span| span.text == "code" && span.style.code && !span.style.bold)
    );
}

#[test]
fn links_allow_only_browser_navigation_and_html_is_never_executed() {
    let blocks = blocks(
        "[Good](https://example.com) [Bad](javascript:alert)\n\n<script>alert(1)</script>\n",
    );
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].spans[0].style.link.as_deref(),
        Some("https://example.com/")
    );
    assert!(
        blocks[0]
            .spans
            .iter()
            .filter(|span| span.text.contains("Bad"))
            .all(|span| span.style.link.is_none())
    );
    assert!(
        !blocks[0]
            .spans
            .iter()
            .any(|span| span.text.contains("alert"))
    );
}

#[test]
fn ordered_lists_and_fences_preserve_source_content() {
    let blocks = blocks("# Title\n\n3. Three\n4. Four\n\n```rust\nlet x = 1;\n```\n");
    assert_eq!(blocks[0].kind, Kind::Heading);
    assert_eq!(blocks[1].spans[0].text, "3. Three");
    assert_eq!(blocks[2].spans[0].text, "4. Four");
    assert_eq!(blocks[3].kind, Kind::Code);
    assert_eq!(blocks[3].spans[0].text, "let x = 1;\n");
}

#[test]
fn tables_keep_cell_boundaries_alignment_and_inline_content() {
    let parsed = blocks("| Name | Value |\n| :--- | ---: |\n| **alpha** | `42` |\n\nAfter");
    let table = parsed[0].table.as_ref().unwrap();
    assert_eq!(table.alignments, [CellAlignment::Start, CellAlignment::End]);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0][0].text, "Name");
    assert!(table.rows[1][0][0].style.bold);
    assert!(table.rows[1][1][0].style.code);
    assert_eq!(parsed[1].spans[0].text, "After");
}
