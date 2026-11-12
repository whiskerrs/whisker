//! Linux Host implementation for `whisker-input`: a `gtk::Text` field, or a `gtk::TextView` when multiline.

#![cfg(target_os = "linux")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use whisker::platform_module::WhiskerValue;
use whisker_linux::gtk4::{self as gtk, gdk, glib, pango, prelude::*};
use whisker_linux::{LinuxEventEmitter, LinuxModuleDefinition, LinuxView, LinuxViewDefinition};
use whisker_protocol::{
    AvailableSpace, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextAlignment,
    MeasureTextDirection, MeasuredSize, ModuleMeasureRequest, PaintColor, TextStyleSnapshot,
};

const NAME: &str = "whisker-input:Input";

#[derive(Clone)]
enum Editor {
    Line(gtk::Text),
    Area(gtk::TextView),
}

impl Editor {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Line(text) => text.clone().upcast(),
            Self::Area(view) => view.clone().upcast(),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Line(text) => text.text().into(),
            Self::Area(view) => {
                let buffer = view.buffer();
                let (start, end) = buffer.bounds();
                buffer.text(&start, &end, false).into()
            }
        }
    }

    fn set_text(&self, value: &str) {
        match self {
            Self::Line(text) => text.set_text(value),
            Self::Area(view) => view.buffer().set_text(value),
        }
    }
}

struct Settings {
    placeholder: String,
    placeholder_color: String,
    caret_color: String,
    selection_color: String,
    multiline: bool,
    secure: bool,
    editable: bool,
    auto_focus: bool,
    max_length: i64,
    keyboard_type: String,
    auto_capitalize: String,
    autocorrect: bool,
    spell_check: bool,
    style: Option<TextStyleSnapshot>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            placeholder: String::new(),
            placeholder_color: String::new(),
            caret_color: String::new(),
            selection_color: String::new(),
            multiline: false,
            secure: false,
            editable: true,
            auto_focus: false,
            max_length: 0,
            keyboard_type: "default".into(),
            auto_capitalize: "sentences".into(),
            autocorrect: true,
            spell_check: true,
            style: None,
        }
    }
}

struct Inner {
    root: gtk::Overlay,
    hint: gtk::Label,
    class: String,
    css: gtk::CssProvider,
    editor: RefCell<Editor>,
    settings: RefCell<Settings>,
    // Set while the Host writes the text, so the change is not echoed back as user input.
    silent: Cell<bool>,
    events: LinuxEventEmitter,
}

struct InputView(Rc<Inner>);

impl LinuxView for InputView {
    fn widget(&self) -> gtk::Widget {
        self.0.root.clone().upcast()
    }
}

impl Drop for InputView {
    fn drop(&mut self) {
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&display, &self.0.css);
        }
    }
}

fn detail(value: &str) -> WhiskerValue {
    WhiskerValue::map([("value", WhiskerValue::String(value.to_owned()))])
}

impl InputView {
    fn new(events: LinuxEventEmitter) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let class = format!("whisker-input-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let css = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let root = gtk::Overlay::new();
        let hint = gtk::Label::new(None);
        hint.set_xalign(0.0);
        hint.set_yalign(0.0);
        hint.set_can_target(false);
        hint.add_css_class(&format!("{class}-hint"));
        root.add_overlay(&hint);
        let inner = Rc::new(Inner {
            root,
            hint,
            class,
            css,
            editor: RefCell::new(Editor::Line(gtk::Text::new())),
            settings: RefCell::new(Settings::default()),
            silent: Cell::new(false),
            events,
        });
        let view = Self(inner);
        view.rebuild();
        let weak = Rc::downgrade(&view.0);
        view.0.root.connect_map(move |_| {
            if let Some(inner) = weak.upgrade()
                && inner.settings.borrow().auto_focus
            {
                inner.editor.borrow().widget().grab_focus();
            }
        });
        view
    }

    fn rebuild(&self) {
        let inner = &self.0;
        let value = inner.editor.borrow().text();
        let editor = if inner.settings.borrow().multiline {
            let view = gtk::TextView::new();
            view.set_wrap_mode(gtk::WrapMode::WordChar);
            view.set_accepts_tab(false);
            Editor::Area(view)
        } else {
            Editor::Line(gtk::Text::new())
        };
        let widget = editor.widget();
        widget.add_css_class(&inner.class);
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        inner.root.set_child(Some(&widget));
        *inner.editor.borrow_mut() = editor.clone();
        self.connect(&editor);
        inner.silent.set(true);
        editor.set_text(&value);
        inner.silent.set(false);
        self.apply();
    }

    fn connect(&self, editor: &Editor) {
        let weak = Rc::downgrade(&self.0);
        let changed = move || {
            let Some(inner) = weak.upgrade() else { return };
            inner.refresh_hint();
            if inner.silent.get() {
                return;
            }
            let mut value = inner.editor.borrow().text();
            let limit = inner.settings.borrow().max_length;
            if limit > 0 && value.chars().count() > limit as usize {
                value = value.chars().take(limit as usize).collect();
                inner.silent.set(true);
                inner.editor.borrow().set_text(&value);
                inner.silent.set(false);
            }
            inner.events.emit("input", detail(&value));
        };
        match editor {
            Editor::Line(text) => {
                text.connect_changed(move |_| changed());
                let weak = Rc::downgrade(&self.0);
                text.connect_activate(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        inner.submit();
                    }
                });
            }
            Editor::Area(view) => {
                view.buffer().connect_changed(move |_| changed());
                let keys = gtk::EventControllerKey::new();
                let weak = Rc::downgrade(&self.0);
                keys.connect_key_pressed(move |_, key, _, state| {
                    let submit = matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
                        && state.contains(gdk::ModifierType::CONTROL_MASK);
                    match weak.upgrade() {
                        Some(inner) if submit => {
                            inner.submit();
                            glib::Propagation::Stop
                        }
                        _ => glib::Propagation::Proceed,
                    }
                });
                view.add_controller(keys);
            }
        }
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&self.0);
        focus.connect_enter(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner
                    .events
                    .emit("focus", WhiskerValue::Map(Default::default()));
            }
        });
        let weak = Rc::downgrade(&self.0);
        focus.connect_leave(move |_| {
            if let Some(inner) = weak.upgrade() {
                let value = inner.editor.borrow().text();
                inner
                    .events
                    .emit("blur", WhiskerValue::Map(Default::default()));
                inner.events.emit("change", detail(&value));
            }
        });
        editor.widget().add_controller(focus);
    }

    fn update(&self, change: impl FnOnce(&mut Settings)) {
        let multiline = self.0.settings.borrow().multiline;
        change(&mut self.0.settings.borrow_mut());
        if multiline != self.0.settings.borrow().multiline {
            self.rebuild();
        } else {
            self.apply();
        }
    }

    fn apply(&self) {
        let inner = &self.0;
        let settings = inner.settings.borrow();
        let editor = inner.editor.borrow();
        let purpose = match settings.keyboard_type.as_str() {
            "number" | "numeric" => gtk::InputPurpose::Number,
            "decimal" => gtk::InputPurpose::Number,
            "phone" => gtk::InputPurpose::Phone,
            "email" => gtk::InputPurpose::Email,
            "url" => gtk::InputPurpose::Url,
            _ if settings.secure => gtk::InputPurpose::Password,
            _ => gtk::InputPurpose::FreeForm,
        };
        let mut hints = match settings.auto_capitalize.as_str() {
            "characters" => gtk::InputHints::UPPERCASE_CHARS,
            "words" => gtk::InputHints::UPPERCASE_WORDS,
            "none" => gtk::InputHints::LOWERCASE,
            _ => gtk::InputHints::UPPERCASE_SENTENCES,
        };
        hints |= if settings.spell_check {
            gtk::InputHints::SPELLCHECK
        } else {
            gtk::InputHints::NO_SPELLCHECK
        };
        if settings.autocorrect {
            hints |= gtk::InputHints::WORD_COMPLETION;
        }
        let alignment = settings.style.as_ref().map(|style| style.alignment);
        let rtl = settings
            .style
            .as_ref()
            .is_some_and(|style| style.direction == MeasureTextDirection::RightToLeft);
        let xalign = match alignment {
            Some(MeasureTextAlignment::Center) => 0.5,
            Some(MeasureTextAlignment::End) => (!rtl) as u8 as f32,
            Some(MeasureTextAlignment::Right) => 1.0,
            Some(MeasureTextAlignment::Start) => rtl as u8 as f32,
            _ => 0.0,
        };
        match &*editor {
            Editor::Line(text) => {
                text.set_placeholder_text(Some(&settings.placeholder));
                text.set_visibility(!settings.secure);
                text.set_editable(settings.editable);
                text.set_max_length(settings.max_length.clamp(0, i32::MAX as i64) as i32);
                text.set_input_purpose(purpose);
                text.set_input_hints(hints);
                text.set_alignment(xalign);
            }
            Editor::Area(view) => {
                view.set_editable(settings.editable);
                view.set_cursor_visible(settings.editable);
                view.set_input_purpose(purpose);
                view.set_input_hints(hints);
                view.set_justification(match xalign {
                    0.5 => gtk::Justification::Center,
                    1.0 => gtk::Justification::Right,
                    _ => gtk::Justification::Left,
                });
            }
        }
        let direction = match settings.style.as_ref().map(|style| style.direction) {
            Some(MeasureTextDirection::LeftToRight) => gtk::TextDirection::Ltr,
            Some(MeasureTextDirection::RightToLeft) => gtk::TextDirection::Rtl,
            _ => gtk::TextDirection::None,
        };
        editor.widget().set_direction(direction);
        inner.hint.set_text(&settings.placeholder);
        inner.hint.set_xalign(xalign);
        inner
            .css
            .load_from_string(&stylesheet(&inner.class, &settings));
        drop(settings);
        drop(editor);
        inner.refresh_hint();
    }

    fn set_value(&self, value: &str) {
        let inner = &self.0;
        if inner.editor.borrow().text() == value {
            return;
        }
        inner.silent.set(true);
        inner.editor.borrow().set_text(value);
        inner.silent.set(false);
        inner.refresh_hint();
    }

    fn blur(&self) {
        let widget = self.0.editor.borrow().widget();
        if widget.has_focus()
            && let Some(window) = widget.root().and_downcast::<gtk::Window>()
        {
            GtkWindowExt::set_focus(&window, None::<&gtk::Widget>);
        }
    }
}

impl Inner {
    fn refresh_hint(&self) {
        let area = matches!(&*self.editor.borrow(), Editor::Area(_));
        let empty = self.editor.borrow().text().is_empty();
        self.hint.set_visible(area && empty);
    }

    fn submit(&self) {
        let value = self.editor.borrow().text();
        self.events.emit("submit", detail(&value));
        self.events.emit("change", detail(&value));
    }
}

fn css_color(color: &PaintColor) -> String {
    match color {
        PaintColor::Named(name) => name.clone(),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => format!("rgba({red},{green},{blue},{alpha})"),
        #[allow(unreachable_patterns)]
        _ => "currentColor".into(),
    }
}

fn css_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn stylesheet(class: &str, settings: &Settings) -> String {
    let mut text = String::from(
        "background: transparent; border: none; box-shadow: none; outline: none; \
         padding: 0; margin: 0; min-height: 0;",
    );
    if let Some(style) = &settings.style {
        let font = &style.style;
        let family = font
            .font_families
            .iter()
            .map(|family| match family {
                MeasureFontFamily::System => "sans-serif".to_owned(),
                MeasureFontFamily::Named(name) => css_string(name),
            })
            .collect::<Vec<_>>()
            .join(", ");
        if !family.is_empty() {
            text.push_str(&format!(" font-family: {family};"));
        }
        text.push_str(&format!(
            " font-size: {}px; font-weight: {}; font-style: {}; letter-spacing: {}px; color: {};",
            font.font_size,
            font.font_weight,
            match font.font_style {
                MeasureFontStyle::Normal => "normal",
                MeasureFontStyle::Italic => "italic",
                MeasureFontStyle::Oblique => "oblique",
            },
            font.letter_spacing,
            css_color(&style.paint.foreground),
        ));
        if let MeasureLineHeight::LogicalPixels(height) = font.line_height {
            text.push_str(&format!(" line-height: {height}px;"));
        }
    }
    if !settings.caret_color.is_empty() {
        text.push_str(&format!(" caret-color: {};", settings.caret_color));
    }
    let mut css = format!(
        ".{class}, .{class} > text {{ {text} }}\n.{class}-hint {{ {text} opacity: 0.5; }}\n"
    );
    if !settings.selection_color.is_empty() {
        css.push_str(&format!(
            ".{class} selection, .{class} > text selection {{ background-color: {}; }}\n",
            settings.selection_color
        ));
    }
    if !settings.placeholder_color.is_empty() {
        css.push_str(&format!(
            ".{class} > placeholder, .{class}-hint {{ color: {}; opacity: 1; }}\n",
            settings.placeholder_color
        ));
    }
    css
}

thread_local! {
    static MEASURER: gtk::Label = gtk::Label::new(None);
}

fn weight(value: i64) -> pango::Weight {
    match value {
        ..=150 => pango::Weight::Thin,
        151..=250 => pango::Weight::Ultralight,
        251..=350 => pango::Weight::Light,
        351..=450 => pango::Weight::Normal,
        451..=550 => pango::Weight::Medium,
        551..=650 => pango::Weight::Semibold,
        651..=750 => pango::Weight::Bold,
        751..=850 => pango::Weight::Ultrabold,
        _ => pango::Weight::Heavy,
    }
}

fn measure(request: &ModuleMeasureRequest) -> Option<MeasuredSize> {
    if request.payload_version != 1 {
        return None;
    }
    let field = |name: &str| match &request.payload {
        WhiskerValue::Map(entries) => entries.get(name),
        _ => None,
    };
    let number = |name: &str| match field(name) {
        Some(WhiskerValue::Float(value)) => Some(*value),
        Some(WhiskerValue::Int(value)) => Some(*value as f64),
        _ => None,
    };
    let Some(WhiskerValue::String(text)) = field("text") else {
        return None;
    };
    let multiline = matches!(field("multiline"), Some(WhiskerValue::Bool(true)));
    let italic = matches!(field("italic"), Some(WhiskerValue::Bool(true)));
    let font_size = number("font_size")?;
    let letter_spacing = number("letter_spacing").unwrap_or(0.0);
    let line_height = number("line_height");
    let text = if multiline {
        text.clone()
    } else {
        text.replace('\n', " ")
    };
    MEASURER.with(|label| {
        let layout = label.create_pango_layout(Some(&text));
        let mut font = layout
            .context()
            .font_description()
            .unwrap_or_else(pango::FontDescription::new);
        if let Some(WhiskerValue::String(family)) = field("font_family") {
            font.set_family(family);
        }
        font.set_absolute_size(font_size * f64::from(pango::SCALE));
        font.set_weight(weight(number("font_weight").unwrap_or(400.0) as i64));
        font.set_style(if italic {
            pango::Style::Italic
        } else {
            pango::Style::Normal
        });
        layout.set_font_description(Some(&font));
        let attributes = pango::AttrList::new();
        attributes.insert(pango::AttrInt::new_letter_spacing(
            (letter_spacing * f64::from(pango::SCALE)) as i32,
        ));
        layout.set_attributes(Some(&attributes));
        let natural = layout.pixel_size().0 as f32;
        let available = match request.available_space[0] {
            AvailableSpace::Definite(width) => natural.min(width),
            AvailableSpace::MinContent if multiline => 1.0,
            AvailableSpace::MinContent | AvailableSpace::MaxContent => natural,
        };
        let width = request.known_dimensions[0].unwrap_or(available).max(1.0);
        if multiline {
            layout.set_wrap(pango::WrapMode::WordChar);
            layout.set_width((width * pango::SCALE as f32) as i32);
        }
        let (used_width, used_height) = layout.pixel_size();
        let height = match line_height {
            Some(line) => line as f32 * layout.line_count().max(1) as f32,
            None => used_height as f32,
        };
        Some(MeasuredSize::new(
            request.known_dimensions[0].unwrap_or(used_width as f32),
            request.known_dimensions[1].unwrap_or(height),
        ))
    })
}

fn string(value: &WhiskerValue) -> String {
    match value {
        WhiskerValue::String(value) => value.clone(),
        _ => String::new(),
    }
}

fn boolean(value: &WhiskerValue, default: bool) -> bool {
    match value {
        WhiskerValue::Bool(value) => *value,
        _ => default,
    }
}

fn int(value: &WhiskerValue) -> i64 {
    match value {
        WhiskerValue::Int(value) => *value,
        _ => 0,
    }
}

struct InputModule;

#[whisker::WhiskerModule]
impl whisker::WhiskerModule for InputModule {
    type Definition = LinuxModuleDefinition;

    fn definition() -> Self::Definition {
        macro_rules! setting {
            ($definition:expr, $prop:literal, |$settings:ident, $value:ident| $set:expr, $default:expr) => {
                $definition.prop(
                    $prop,
                    |view: &InputView, $value| view.update(|$settings| $set),
                    |view: &InputView| {
                        let $value = &$default;
                        view.update(|$settings| $set)
                    },
                )
            };
        }
        let view = LinuxViewDefinition::new(NAME, InputView::new).prop(
            "value",
            |view, value| view.set_value(&string(value)),
            |view| view.set_value(""),
        );
        let view = setting!(
            view,
            "placeholder",
            |s, v| s.placeholder = string(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "placeholder-color",
            |s, v| s.placeholder_color = string(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "caret-color",
            |s, v| s.caret_color = string(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "selection-color",
            |s, v| s.selection_color = string(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "multiline",
            |s, v| s.multiline = boolean(v, false),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "secure",
            |s, v| s.secure = boolean(v, false),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "editable",
            |s, v| s.editable = boolean(v, true),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "auto-focus",
            |s, v| s.auto_focus = boolean(v, false),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "max-length",
            |s, v| s.max_length = int(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "keyboard-type",
            |s, v| s.keyboard_type = string(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "auto-capitalize",
            |s, v| s.auto_capitalize = string(v),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "autocorrect",
            |s, v| s.autocorrect = boolean(v, true),
            WhiskerValue::Null
        );
        let view = setting!(
            view,
            "spell-check",
            |s, v| s.spell_check = boolean(v, true),
            WhiskerValue::Null
        );
        let view = view
            .prop("lines", |_, _| {}, |_| {})
            .prop("return-key", |_, _| {}, |_| {})
            .prop("auto-size", |_, _| {}, |_| {})
            .text_style(|view, style| view.update(|settings| settings.style = Some(style.clone())))
            .measure(measure)
            .event("input")
            .event("change")
            .event("focus")
            .event("blur")
            .event("submit")
            .command("focus", |view, _| {
                view.0.editor.borrow().widget().grab_focus();
            })
            .command("blur", |view, _| view.blur())
            .command("clear", |view, _| {
                view.set_value("");
                view.0.events.emit("input", detail(""));
            })
            .command("setValue", |view, value| {
                if let WhiskerValue::Map(entries) = value
                    && let Some(WhiskerValue::String(value)) = entries.get("value")
                {
                    view.set_value(value);
                }
            });
        LinuxModuleDefinition::new().name(NAME).view(view)
    }
}
