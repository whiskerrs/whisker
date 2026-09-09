use serde::{Deserialize, Serialize};
use whisker::prelude::*;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: u32,
    pub paper: u32,
    pub ink: u32,
    pub muted: u32,
    pub border: u32,
    pub tint: u32,
    pub accent: u32,
    pub accent_soft: u32,
    pub on_accent: u32,
    pub code: u32,
    pub on_code: u32,
}

impl Appearance {
    pub fn palette(self) -> Palette {
        match self {
            Self::Light => Palette {
                canvas: 0xfafafa,
                paper: 0xffffff,
                ink: 0x202124,
                muted: 0x676b73,
                border: 0xdcdfe3,
                tint: 0xefeff1,
                accent: 0x24262b,
                accent_soft: 0xebedf0,
                on_accent: 0xffffff,
                code: 0x181b20,
                on_code: 0xe5e7eb,
            },
            Self::Dark => Palette {
                canvas: 0x18191c,
                paper: 0x222428,
                ink: 0xeeeef0,
                muted: 0xa1a5ae,
                border: 0x393c43,
                tint: 0x2b2d32,
                accent: 0xe8eaed,
                accent_soft: 0x2c2f35,
                on_accent: 0x202124,
                code: 0x111318,
                on_code: 0xe5e7eb,
            },
        }
    }
}

pub fn provide_appearance() {
    let appearance = signal(Appearance::default());
    provide_context(appearance);
    on_mount(move || match crate::storage::load_appearance() {
        Ok(Some(saved)) => appearance.set(saved),
        Ok(None) => {}
        Err(error) => eprintln!("Unable to restore appearance: {error}"),
    });
}

pub fn use_appearance() -> RwSignal<Appearance> {
    use_context().expect("Appearance context")
}

/// Capture the app's theme once; update styles without remounting their views.
pub fn style(build: impl Fn(Palette) -> Css + 'static) -> ReadSignal<Css> {
    let appearance = use_appearance();
    computed(move || build(appearance.get().palette()))
}
