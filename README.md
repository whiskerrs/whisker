<p align="center">
  <a href="https://whisker.rs">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/whiskerrs/whisker/main/.github/assets/banner-dark.png" />
      <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/whiskerrs/whisker/main/.github/assets/banner-light.png" />
      <img alt="Whisker — Build native mobile apps in Rust" src="https://raw.githubusercontent.com/whiskerrs/whisker/main/.github/assets/banner-dark.png" width="100%" />
    </picture>
  </a>
</p>

<p align="center">
  Build native iOS and Android apps in Rust — a Leptos-style fine-grained
  reactive API on the <a href="https://github.com/lynx-family/lynx">Lynx</a> engine.
  No virtual DOM, no JavaScript runtime.
</p>

<p align="center">
  <a href="https://crates.io/crates/whisker-cli"><img src="https://img.shields.io/crates/v/whisker-cli?logo=rust&label=whisker-cli&color=fdba74" alt="crates.io" /></a>
  <a href="https://whisker.rs/docs"><img src="https://img.shields.io/badge/docs-whisker.rs-22d3ee" alt="Documentation" /></a>
  <a href="#license"><img src="https://img.shields.io/crates/l/whisker-cli" alt="License" /></a>
</p>

---

```rust
use whisker::prelude::*;

#[whisker::main]
fn app() -> Element {
    render! {
        Counter
    }
}

#[component]
fn counter() -> Element {
    let count = signal(0);
    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            gap: px(12),
            background_color: Color::hex(0x0B0B0F),
        )) {
            text(
                value: computed(move || format!("Count: {}", count.get())),
                style: css!(color: Color::hex(0xFFFFFF), font_size: px(28)),
            )
            view(
                style: css!(
                    padding: (px(10), px(20)),
                    border_radius: px(10),
                    background_color: Color::hex(0x7C5CFF),
                ),
                on_tap: move |_| count.set(count.get() + 1),
            ) {
                text(value: "+1", style: css!(color: Color::hex(0xFFFFFF)))
            }
        }
    }
}
```

<p align="center">
  <img alt="Whisker hot reload — edit the code, save, and the running app updates in under a second with state intact" src="https://raw.githubusercontent.com/whiskerrs/whisker/main/.github/assets/hot-reload.gif" width="100%" />
</p>

<p align="center"><sub>Edit, save — the running app hot-reloads in under a second, with state intact.</sub></p>

## Get started

```sh
cargo install whisker-cli
whisker new my-app && cd my-app
whisker run ios          # or: whisker run android
```

`whisker run` watches your source and hot-patches the running app in under
a second — no restart, no state loss.

## Documentation

Everything — installation, core concepts, guides, and the full API
reference — lives at **[whisker.rs/docs](https://whisker.rs/docs)**.

## Status

Pre-alpha. The core runtime, `render!`, routing, hot reload, and the
iOS/Android build pipelines work end-to-end. APIs may still change
before `1.0`.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option.
