//! Flint CLI implementation.
//!
//! Subcommands:
//! - `init`     — scaffold a new Flint app
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
        Some("init") => todo!("flint init"),
        Some("prebuild") => todo!("flint prebuild"),
        Some("dev") => todo!("flint dev"),
        Some("build") => todo!("flint build"),
        Some("add") => todo!("flint add"),
        Some("clean") => todo!("flint clean"),
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
        "flint — cross-platform mobile UI framework

Usage: flint <SUBCOMMAND>

Subcommands:
  init      Scaffold a new Flint app
  prebuild  Regenerate ios/ and android/ from flint.rs + plugins
  dev       Start dev server with hot reload
  build     Production build
  add       Add a plugin
  clean     Clean build artifacts
"
    );
}
