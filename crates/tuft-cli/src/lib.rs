//! Tuft CLI implementation.
//!
//! Subcommands:
//! - `init`     — scaffold a new Tuft app
//! - `prebuild` — run CNG, regenerate `ios/` and `android/`
//! - `dev`      — start dev server + hot reload + log stream
//! - `build`    — production build (debug / profile / release)
//! - `add`      — add a plugin (cargo add + prebuild trigger)
//! - `clean`    — clean build artifacts

pub fn run(args: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
    // First arg is program name; skip it.
    let mut iter = args.into_iter();
    let _program = iter.next();
    let sub = iter.next();

    match sub.as_deref() {
        Some("init") => todo!("tuft init"),
        Some("prebuild") => todo!("tuft prebuild"),
        Some("dev") => todo!("tuft dev"),
        Some("build") => todo!("tuft build"),
        Some("add") => todo!("tuft add"),
        Some("clean") => todo!("tuft clean"),
        Some(other) => {
            anyhow::bail!("unknown subcommand: {other}");
        }
        None => {
            print_help();
            Ok(())
        }
    }
}

fn print_help() {
    println!(
        "tuft — cross-platform mobile UI framework

Usage: tuft <SUBCOMMAND>

Subcommands:
  init      Scaffold a new Tuft app
  prebuild  Regenerate ios/ and android/ from tuft.rs + plugins
  dev       Start dev server with hot reload
  build     Production build
  add       Add a plugin
  clean     Clean build artifacts
"
    );
}
