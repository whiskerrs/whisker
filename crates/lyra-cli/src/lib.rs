//! Lyra CLI implementation.
//!
//! Subcommands:
//! - `init`     — scaffold a new Lyra app
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
        Some("init") => todo!("lyra init"),
        Some("prebuild") => todo!("lyra prebuild"),
        Some("dev") => todo!("lyra dev"),
        Some("build") => todo!("lyra build"),
        Some("add") => todo!("lyra add"),
        Some("clean") => todo!("lyra clean"),
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
        "lyra — cross-platform mobile UI framework

Usage: lyra <SUBCOMMAND>

Subcommands:
  init      Scaffold a new Lyra app
  prebuild  Regenerate ios/ and android/ from lyra.rs + plugins
  dev       Start dev server with hot reload
  build     Production build
  add       Add a plugin
  clean     Clean build artifacts
"
    );
}
