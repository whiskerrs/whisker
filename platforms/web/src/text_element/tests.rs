use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen_test::wasm_bindgen_test;
use whisker_protocol::*;

#[wasm_bindgen_test]
fn native_selection_queries_use_utf16_and_reject_stale_presentations() {
    let document = web_sys::window().unwrap().document().unwrap();
    let element = document.create_element("div").unwrap();
    let content = document.create_element("span").unwrap();
    content.set_attribute("data-whisker-text", "").unwrap();
    element.append_child(&content).unwrap();
    document.body().unwrap().append_child(&element).unwrap();
    let received = Rc::new(RefCell::new(Vec::new()));
    let events = received.clone();
    let mut text = TextElement {
        element: element.clone(),
        events: WebEventEmitter(Rc::new(move |event, _| events.borrow_mut().push(event))),
        selection_listener: None,
        copy_listener: None,
        action_listener: None,
    };
    let mut model = content_fixture();
    crate::paint::text::apply(&content, &model).unwrap();
    text.set_selectable(true).unwrap();
    text.set_selection(
        &WhiskerValue::map([
            ("revision", WhiskerValue::Int(7)),
            ("start", WhiskerValue::Int(6)),
            ("end", WhiskerValue::Int(8)),
        ]),
        false,
    )
    .unwrap();
    let request = |revision, kind: &str| {
        WhiskerValue::map([
            ("id", WhiskerValue::Int(3)),
            ("revision", WhiskerValue::Int(revision)),
            ("kind", WhiskerValue::String(kind.into())),
            ("start", WhiskerValue::Int(6)),
            ("end", WhiskerValue::Int(8)),
        ])
    };
    text.query(&request(7, "selectedText")).unwrap();
    assert_eq!(
        string(&received.borrow().last().unwrap().detail, "text"),
        Some("🦀")
    );
    text.query(&request(7, "boundingRects")).unwrap();
    let WhiskerValue::Map(detail) = &received.borrow().last().unwrap().detail.clone() else {
        panic!()
    };
    let WhiskerValue::Array(rects) = &detail["rects"] else {
        panic!()
    };
    assert_eq!(rects.len(), 1);
    text.query(&request(8, "selectedText")).unwrap();
    assert_eq!(
        string(&received.borrow().last().unwrap().detail, "error"),
        Some("stale-layout")
    );
    model.paint.foreground = PaintColor::Srgba {
        red: 200,
        green: 0,
        blue: 0,
        alpha: 1.0,
    };
    crate::paint::text::apply(&content, &model).unwrap();
    assert_eq!(
        ranges::selection(&content).unwrap(),
        Some((6, 8, "forward"))
    );
    model.payload.text = "Changed".into();
    model.prepared_content = PreparedContentId::new(8);
    crate::paint::text::apply(&content, &model).unwrap();
    assert_eq!(ranges::selection(&content).unwrap(), None);
    element.remove();
}

fn content_fixture() -> TextContent {
    TextContent {
        paragraph: None,
        runs: vec![],
        paint: TextPaint::default(),
        prepared_content: PreparedContentId::new(7),
        payload: TextMeasurePayload {
            text: "Hello 🦀 world".into(),
            style: TextMeasureStyle::default(),
            runs: vec![],
            attachments: vec![],
            locale: None,
            direction: MeasureTextDirection::Auto,
            alignment: MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::Wrap,
            word_break: Default::default(),
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
        },
    }
}

#[wasm_bindgen_test]
fn measured_dom_survives_paint_updates_and_inline_actions_are_scoped() {
    use whisker_engine::MeasurementProvider;
    let document = web_sys::window().unwrap().document().unwrap();
    let root = document.create_element("div").unwrap();
    let display = document.create_element("span").unwrap();
    display.set_attribute("data-whisker-text", "").unwrap();
    root.append_child(&display).unwrap();
    document.body().unwrap().append_child(&root).unwrap();
    let mut content = content_fixture();
    let range = TextByteRange {
        start: 0,
        end: content.payload.text.len() as u32,
    };
    content.payload.runs.push(TextMeasureRun {
        range,
        style: content.payload.style.clone(),
        alignment: Default::default(),
    });
    content.runs.push(TextPaintRun {
        range,
        span: TextSpanId::new(2).unwrap(),
        action: TextSpanId::new(2),
        paint: TextPaint::default(),
        background: None,
        background_radii: PaintCorners {
            top_left: Default::default(),
            top_right: Default::default(),
            bottom_right: Default::default(),
            bottom_left: Default::default(),
        },
    });
    let mut provider = crate::measure::text::DomMeasurementProvider::new(document.clone());
    let mut responses = Vec::new();
    provider
        .measure_batch(
            SurfaceId::new(1).unwrap(),
            &[MeasurementRequest {
                key: MeasurementKey::new(7).unwrap(),
                node: NodeId::new(1).unwrap(),
                element_type: ElementTypeId::new(1).unwrap(),
                environment_epoch: 1,
                constraints: MeasureConstraints {
                    known_dimensions: [None, None],
                    available_space: [AvailableSpace::Definite(300.0), AvailableSpace::MaxContent],
                },
                payload: MeasurementPayload::Text(content.payload.clone()),
            }],
            &mut responses,
        )
        .unwrap();
    let MeasurementResponse::Ready { metrics, .. } = responses.remove(0) else {
        panic!("measurement")
    };
    content.paragraph = metrics.paragraph;
    let prepared = provider.prepared.borrow()[&content.prepared_content.unwrap()].clone();
    let span = prepared
        .first_element_child()
        .unwrap()
        .first_element_child()
        .unwrap();
    crate::paint::text::apply_prepared(&display, &content, Some(&prepared)).unwrap();
    content.runs[0].paint.foreground = PaintColor::Srgba {
        red: 255,
        green: 0,
        blue: 0,
        alpha: 1.0,
    };
    crate::paint::text::apply_prepared(&display, &content, Some(&prepared)).unwrap();
    assert!(
        span.is_same_node(
            display
                .query_selector("[role=button]")
                .unwrap()
                .as_ref()
                .map(|element| element.as_ref())
        )
    );
    assert_eq!(span.get_attribute("tabindex").as_deref(), Some("0"));
    let received = Rc::new(RefCell::new(Vec::new()));
    let events = received.clone();
    let mut text = TextElement {
        element: root.clone(),
        events: WebEventEmitter(Rc::new(move |event, _| events.borrow_mut().push(event))),
        selection_listener: None,
        copy_listener: None,
        action_listener: None,
    };
    text.install_actions().unwrap();
    let options = web_sys::MouseEventInit::new();
    options.set_bubbles(true);
    span.dispatch_event(
        &web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &options).unwrap(),
    )
    .unwrap();
    assert_eq!(received.borrow().last().unwrap().event, "textactivate");
    drop(text);
    span.dispatch_event(
        &web_sys::MouseEvent::new_with_mouse_event_init_dict("click", &options).unwrap(),
    )
    .unwrap();
    assert_eq!(received.borrow().len(), 1);
    provider.retain_prepared_content(SurfaceId::new(1).unwrap(), 1, &mut std::iter::empty());
    assert!(provider.prepared.borrow().is_empty());
    assert!(display.contains(Some(&span)));
    root.remove();
}

#[wasm_bindgen_test]
fn selection_expands_combining_sequences_and_rejects_split_surrogates() {
    let document = web_sys::window().unwrap().document().unwrap();
    let content = document.create_element("span").unwrap();
    content.set_text_content(Some("e\u{301}🧑‍🚀"));
    document.body().unwrap().append_child(&content).unwrap();
    ranges::select(&content, 1, 2).unwrap();
    assert_eq!(
        ranges::selection(&content).unwrap(),
        Some((0, 2, "forward"))
    );
    assert!(ranges::select(&content, 3, 4).is_err());
    ranges::select(&content, 2, 4).unwrap();
    assert_eq!(
        ranges::selection(&content).unwrap(),
        Some((2, 7, "forward"))
    );
    content.remove();
}
