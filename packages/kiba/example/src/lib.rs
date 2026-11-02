use kiba::Viewport;
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[whisker::main]
pub fn app() -> Element {
    let style = Css::new().width(percent(100)).height(percent(100));
    render! {
        Viewport(style: style)
    }
}
