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
    let count = RwSignal::new(0);
    render! {
        page(style: "padding: 24px; flex-direction: column; gap: 12px;") {
            text(value: computed(move || format!("Count: {}", count.get())))
            view(on_tap: move |_| count.update(|n| *n += 1)) {
                text(value: "+1")
            }
        }
    }
}
```

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

Pre-alpha (`0.2.x`). The core runtime, `render!`, routing, hot reload, and
the iOS/Android build pipelines work end-to-end. APIs may still change
before `1.0`.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option.
