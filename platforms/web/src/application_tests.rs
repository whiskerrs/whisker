use super::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use whisker::prelude::*;

#[wasm_bindgen(inline_js = "
export function mockViewport() {
    const descriptor = Object.getOwnPropertyDescriptor(window, 'visualViewport');
    const viewport = new EventTarget();
    Object.assign(viewport, { width: innerWidth, height: 600, scale: 1 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: viewport });
    return () => descriptor
        ? Object.defineProperty(window, 'visualViewport', descriptor)
        : delete window.visualViewport;
}
export function resizeViewport(height, scale) {
    Object.assign(window.visualViewport, { width: innerWidth / scale, height, scale });
    window.visualViewport.dispatchEvent(new Event('resize'));
}
export function withoutVisualViewport() {
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: null });
    window.dispatchEvent(new Event('resize'));
}
export function afterFrames() {
    return new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
}
")]
extern "C" {
    #[wasm_bindgen(js_name = mockViewport)]
    fn mock_viewport() -> js_sys::Function;
    #[wasm_bindgen(js_name = resizeViewport)]
    fn resize_viewport(height: f64, scale: f64);
    #[wasm_bindgen(js_name = withoutVisualViewport)]
    fn without_visual_viewport();
    #[wasm_bindgen(js_name = afterFrames)]
    fn after_frames() -> js_sys::Promise;
}

struct MountedViewport {
    root: web_sys::Element,
    restore: js_sys::Function,
}

impl Drop for MountedViewport {
    fn drop(&mut self) {
        APPLICATION.with(|slot| *slot.borrow_mut() = None);
        self.root.remove();
        self.restore.call0(&JsValue::NULL).unwrap();
    }
}

fn assert_height(root: &web_sys::Element, expected: f32) {
    assert_eq!(root.get_bounding_client_rect().height() as f32, expected);
    APPLICATION.with(|slot| {
        assert_eq!(slot.borrow().as_ref().unwrap().viewport.1, expected);
    });
    let content = root
        .shadow_root()
        .unwrap()
        .query_selector("[data-whisker-node]")
        .unwrap()
        .expect("the mounted View must have DOM content");
    assert_eq!(content.get_bounding_client_rect().height() as f32, expected);
}

#[wasm_bindgen_test]
async fn visual_viewport_resize_updates_surface_and_dom_without_window_resize() {
    let restore = mock_viewport();
    let document = browser_window().unwrap().document().unwrap();
    let root = document.create_element("div").unwrap();
    root.set_id("viewport-test-root");
    document.body().unwrap().append_child(&root).unwrap();
    let mounted = MountedViewport { root, restore };
    let mut config = WebAppConfig::new("Viewport test");
    config.root_id = "viewport-test-root".into();
    run(
        config,
        || render! { View(style: Css::new().height(percent(100))) },
    )
    .unwrap();
    JsFuture::from(after_frames()).await.unwrap();
    assert_height(&mounted.root, 600.0);

    for height in [720.0, 560.0, 720.0] {
        resize_viewport(height, 1.0);
        JsFuture::from(after_frames()).await.unwrap();
        assert_height(&mounted.root, height as f32);
    }

    resize_viewport(360.0, 2.0);
    JsFuture::from(after_frames()).await.unwrap();
    assert_height(&mounted.root, 720.0);

    without_visual_viewport();
    JsFuture::from(after_frames()).await.unwrap();
    let height = browser_window()
        .unwrap()
        .inner_height()
        .unwrap()
        .as_f64()
        .unwrap();
    assert_height(&mounted.root, height as f32);
}
