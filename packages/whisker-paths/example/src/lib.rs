//! `whisker-paths` example app.
//!
//! On launch it resolves all four per-app directories, then does a
//! write → read round-trip against a file under the cache dir, so a
//! `whisker run` on a real device verifies the native module wiring and
//! that `std::fs` works against the resolved paths.

use whisker::prelude::*;
use whisker::runtime::view::Element;

const BG: &str = "#101012";
const FG: &str = "#f0f0f3";

#[whisker::main]
pub fn app() -> Element {
    let log = RwSignal::new("resolving paths…".to_string());

    on_mount(move || {
        let mut out = String::new();
        out.push_str(&format!(
            "cache:    {}\n",
            whisker_paths::cache_dir().display()
        ));
        out.push_str(&format!(
            "document: {}\n",
            whisker_paths::document_dir().display()
        ));
        out.push_str(&format!(
            "support:  {}\n",
            whisker_paths::support_dir().display()
        ));
        out.push_str(&format!(
            "temp:     {}\n\n",
            whisker_paths::temp_dir().display()
        ));

        let dir = whisker_paths::cache_dir().join("whisker-paths-example");
        let file = dir.join("roundtrip.txt");
        match std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(&file, b"hello from whisker-paths"))
            .and_then(|_| std::fs::read_to_string(&file))
        {
            Ok(s) if s == "hello from whisker-paths" => {
                out.push_str("fs round-trip: ok (wrote + read back match)")
            }
            Ok(s) => out.push_str(&format!("fs round-trip: MISMATCH {s}")),
            Err(e) => out.push_str(&format!("fs round-trip: ERROR {e}")),
        }
        log.set(out);
    });

    let page = format!(
        "background-color: {BG}; flex-grow: 1; display: flex; flex-direction: column; \
         padding-top: 72px; padding-left: 20px; padding-right: 20px;"
    );
    let title = format!("color: {FG}; font-size: 22px; font-weight: 700; margin-bottom: 20px;");
    let body = format!("color: {FG}; font-size: 13px; line-height: 22px;");

    render! {
        view(style: page) {
            text(style: title, value: "whisker-paths")
            text(style: body, value: computed(move || log.get()))
        }
    }
}
