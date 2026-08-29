//! `whisker-paths` example app.
//!
//! On launch it resolves all four per-app directories, then does a
//! write → read round-trip against a file under the cache dir, so a
//! `whisker run` on a real device verifies the native module wiring and
//! that `std::fs` works against the resolved paths.

use whisker::css::{FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;

const BG: u32 = 0x101012;
const FG: u32 = 0xf0f0f3;

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
                out.push_str("fs round-trip: ok (wrote + read back match)\n")
            }
            Ok(s) => out.push_str(&format!("fs round-trip: MISMATCH {s}\n")),
            Err(e) => out.push_str(&format!("fs round-trip: ERROR {e}\n")),
        }

        let backup_dir = whisker_paths::document_dir().join("whisker-paths-example");
        match std::fs::create_dir_all(&backup_dir)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                whisker_paths::set_excluded_from_backup(&backup_dir, true)
                    .map_err(|e| e.to_string())
            }) {
            Ok(()) => out.push_str("backup-exclusion: ok"),
            Err(e) => out.push_str(&format!("backup-exclusion: ERROR {e}")),
        }
        log.set(out);
    });

    let page = Css::new()
        .background_color(Color::hex(BG))
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(72))
        .padding_left(px(20))
        .padding_right(px(20));
    let title = Css::new()
        .color(Color::hex(FG))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_bottom(px(20));
    let body = Css::new()
        .color(Color::hex(FG))
        .font_size(px(13))
        .line_height(px(22));

    render! {
        view(style: page) {
            text(style: title, value: "whisker-paths")
            text(style: body, value: computed(move || log.get()))
        }
    }
}
