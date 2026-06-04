//! Detail screen — minimal second route the shell can navigate to.
//! Body is plain text; the bug is reproduced from `HomeScreen`
//! alone (it crashes before navigation ever happens).

use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
pub fn detail_screen() -> Element {
    render! {
        view(style: "display: flex; flex-direction: column; padding: 40px; background-color: #dbeafe; width: 100%; height: 100%;") {
            text(style: "font-size: 24px;", value: "Detail (cross-crate)".to_string())
        }
    }
}
