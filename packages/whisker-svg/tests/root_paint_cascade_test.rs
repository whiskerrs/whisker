//! Regression test for the root-`<svg>` paint cascade.
//!
//! Lucide / Heroicons / etc. carry the icon-wide paint state
//! (`fill="none"`, `stroke="currentColor"`, `stroke-width="2"`)
//! on the root `<svg>` itself and leave each inner `<path>`
//! bare. The compiler used to ignore the root element's own
//! paint attributes and feed `PaintState::default()` (= solid
//! black fill) into the walk — every Lucide icon then rendered
//! as a black silhouette instead of an outline. See compile.rs's
//! `update_paint(&root, …)` call.

use whisker_svg::{TraceVisitor, compile, replay};

const LUCIDE_LIKE: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m9 18 6-6-6-6"/></svg>"#;

#[test]
fn root_svg_paint_cascades_to_child_shapes() {
    let compiled = compile(LUCIDE_LIKE).expect("compile");
    assert!(
        compiled.warnings.is_empty(),
        "compile produced warnings: {:?}",
        compiled.warnings
    );

    let mut trace = TraceVisitor::new();
    replay(&compiled.bytes, &mut trace).expect("replay");
    let txt = trace.into_string();

    // The cascade succeeded → stroke ops present, fill ops absent.
    assert!(
        txt.contains("STROKE_TINT"),
        "expected STROKE_TINT from inherited stroke=\"currentColor\":\n{txt}"
    );
    assert!(
        txt.contains("STROKE\n") || txt.ends_with("STROKE"),
        "expected PATH_STROKE execution:\n{txt}"
    );
    assert!(
        !txt.contains("FILL_COLOR"),
        "did not expect a literal FILL_COLOR — root fill=\"none\" should suppress it:\n{txt}"
    );
    assert!(
        !txt.contains("\nFILL\n") && !txt.contains("FILL_AND_STROKE"),
        "did not expect a fill execution op:\n{txt}"
    );
}

#[test]
fn child_can_still_override_root_paint() {
    // The cascade fix mustn't break shapes that pull their own
    // explicit colour over the inherited tint.
    let svg = r##"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M 0 0 L 10 10" stroke="#ff0000"/></svg>"##;
    let compiled = compile(svg).expect("compile");
    let mut trace = TraceVisitor::new();
    replay(&compiled.bytes, &mut trace).expect("replay");
    let txt = trace.into_string();
    assert!(
        txt.contains("STROKE_COLOR #FF0000FF"),
        "child stroke override didn't reach the trace:\n{txt}"
    );
}
