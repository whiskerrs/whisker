use super::*;
use whisker_protocol::{MeasureTextOverflow, TextByteRange, TextMeasureRun, TextRange};

fn payload() -> TextMeasurePayload {
    let text = "The astronaut 🧑‍🚀 reads 日本語 and then more text".to_owned();
    TextMeasurePayload {
        runs: vec![TextMeasureRun {
            alignment: Default::default(),
            range: TextByteRange {
                start: 0,
                end: text.len() as u32,
            },
            style: TextMeasureStyle {
                font_size: 22.0,
                ..Default::default()
            },
        }],
        text,
        attachments: Vec::new(),
        style: TextMeasureStyle::default(),
        locale: None,
        direction: MeasureTextDirection::Auto,
        alignment: MeasureTextAlignment::Start,
        indent: Default::default(),
        wrap: MeasureTextWrap::Wrap,
        word_break: MeasureTextWordBreak::Normal,
        max_lines: Some(1),
        overflow: MeasureTextOverflow::Ellipsis,
    }
}

#[test]
fn ellipsis_reserves_real_glyph_space_and_hides_only_complete_graphemes() {
    let payload = payload();
    let mut shaper = ParagraphShaper::default();
    for width in [60.0, 210.0, 320.0] {
        let paragraph = shaper.shape(&payload, Some(width));
        assert_eq!(paragraph.lines.len(), 1);
        assert!(paragraph.width <= width);
        assert!(
            payload
                .text
                .grapheme_indices(true)
                .any(|(offset, _)| offset == paragraph.visible_end)
        );
        assert!(paragraph.geometry.validate(&payload.text));
        let hidden =
            TextRange::from_utf8(&payload.text, paragraph.visible_end..payload.text.len()).unwrap();
        assert!(
            paragraph
                .selection_rects(&payload, hidden)
                .unwrap()
                .is_empty()
        );
        assert!(paragraph.geometry.lines[0].ellipsis_count > 0);
    }
}

#[test]
fn range_geometry_selects_a_substring_instead_of_its_complete_style_run() {
    let mut payload = payload();
    payload.max_lines = None;
    let paragraph = ParagraphShaper::default().shape(&payload, Some(600.0));
    let word = paragraph
        .selection_rects(
            &payload,
            TextRange::from_utf8(&payload.text, 4..13).unwrap(),
        )
        .unwrap();
    let entire = paragraph
        .selection_rects(
            &payload,
            TextRange::from_utf8(&payload.text, 0..payload.text.len()).unwrap(),
        )
        .unwrap();
    assert!(!word.is_empty());
    assert!(
        word.iter().map(|rect| rect.width).sum::<f32>()
            < entire.iter().map(|rect| rect.width).sum::<f32>() / 2.0
    );
    assert!(
        paragraph
            .selection_rects(
                &payload,
                TextRange {
                    start: 1000,
                    end: 1001
                }
            )
            .is_none()
    );
}

#[test]
fn custom_token_is_only_placed_on_overflow_and_never_enters_source_ranges() {
    use whisker_protocol::{InlineAttachment, MeasuredSize, NodeId};
    let mut payload = payload();
    let end = payload.text.len() as u32;
    payload.attachments.push(InlineAttachment {
        truncation: true,
        label: None,
        node: NodeId::new(22).unwrap(),
        range: TextByteRange { start: end, end },
        size: MeasuredSize::new(42.0, 20.0),
        baseline: 16.0,
        alignment: Default::default(),
    });
    let mut shaper = ParagraphShaper::default();
    assert!(shaper.shape(&payload, Some(1600.0)).attachments.is_empty());
    for width in [120.0, 20.0] {
        let paragraph = shaper.shape(&payload, Some(width));
        assert_eq!(paragraph.attachments.len(), 1);
        assert_eq!(paragraph.lines.len(), 1);
        assert!(paragraph.width <= width);
        assert!(paragraph.geometry.validate(&payload.text));
        let visible = payload.text[..paragraph.visible_end].encode_utf16().count() as u32;
        assert!(
            paragraph
                .geometry
                .fragments
                .iter()
                .all(|fragment| fragment.range.end <= visible)
        );
        assert_eq!(
            paragraph.geometry.lines[0].range.end,
            payload.text.encode_utf16().count() as u32
        );
    }
}
