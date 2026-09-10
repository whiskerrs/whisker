use super::*;
use wasm_bindgen_test::wasm_bindgen_test;
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn unlimited_length_can_be_applied_and_restored_without_a_dom_exception() {
    let document = web_sys::window().unwrap().document().unwrap();
    let input: web_sys::HtmlInputElement =
        document.create_element("input").unwrap().unchecked_into();
    let textarea: web_sys::HtmlTextAreaElement = document
        .create_element("textarea")
        .unwrap()
        .unchecked_into();
    for limit in [0, 12, 0, -1, 24, 0] {
        set_max_length(&input, &textarea, limit).unwrap();
        if limit > 0 {
            assert_eq!(input.max_length(), limit as i32);
            assert_eq!(textarea.max_length(), limit as i32);
        } else {
            assert!(!input.has_attribute("maxlength"));
            assert!(!textarea.has_attribute("maxlength"));
        }
    }
}

#[wasm_bindgen_test]
fn both_controls_follow_host_padding_and_placeholder_colors() {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let root: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
    root.style().set_css_text("position:relative;width:240px;height:80px;padding:6px 10px 14px 18px;color:rgb(238,238,240)");
    configure_root(&document, &root).unwrap();
    document.body().unwrap().append_child(&root).unwrap();
    for tag in ["input", "textarea"] {
        let control: web_sys::HtmlElement = document.create_element(tag).unwrap().unchecked_into();
        configure_control(&control).unwrap();
        control.set_attribute("placeholder", "Placeholder").unwrap();
        root.append_child(&control).unwrap();
        let computed = window.get_computed_style(&control).unwrap().unwrap();
        assert_eq!(
            computed.get_property_value("padding").unwrap(),
            "6px 10px 14px 18px"
        );
        let placeholder = window
            .get_computed_style_with_pseudo_elt(&control, "::placeholder")
            .unwrap()
            .unwrap();
        assert_eq!(
            placeholder.get_property_value("color").unwrap(),
            "rgb(153, 153, 153)"
        );
        assert_eq!(placeholder.get_property_value("opacity").unwrap(), "1");
        root.style()
            .set_property("--whisker-placeholder-color", "#123456")
            .unwrap();
        assert_eq!(
            placeholder.get_property_value("color").unwrap(),
            "rgb(18, 52, 86)"
        );
        root.style().set_property("padding", "12px").unwrap();
        assert_eq!(computed.get_property_value("padding-left").unwrap(), "12px");
        root.style()
            .set_property("padding", "6px 10px 14px 18px")
            .unwrap();
        root.style()
            .remove_property("--whisker-placeholder-color")
            .unwrap();
        control.remove();
    }
    root.remove();
}

#[wasm_bindgen_test]
fn multiline_submit_requires_a_modifier_and_ignores_composition_and_repeat() {
    let event = |meta, ctrl, shift, alt, composing, repeat| {
        let init = web_sys::KeyboardEventInit::new();
        init.set_key("Enter");
        init.set_meta_key(meta);
        init.set_ctrl_key(ctrl);
        init.set_shift_key(shift);
        init.set_alt_key(alt);
        init.set_is_composing(composing);
        init.set_repeat(repeat);
        web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap()
    };
    assert!(is_submit_key(
        &event(true, false, false, false, false, false),
        true
    ));
    assert!(is_submit_key(
        &event(false, true, false, false, false, false),
        true
    ));
    for key in [
        event(false, false, false, false, false, false),
        event(true, false, true, false, false, false),
        event(true, false, false, true, false, false),
        event(true, false, false, false, true, false),
        event(true, false, false, false, false, true),
    ] {
        assert!(!is_submit_key(&key, true));
    }
    assert!(is_submit_key(
        &event(false, false, false, false, false, false),
        false
    ));
}
