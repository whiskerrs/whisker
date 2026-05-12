//! Build automation for Flint (cargo xtask pattern).
//!
//! Run via `cargo xtask <task>`. Use this for cross-language tasks that don't
//! fit Cargo natively, e.g. building the C++ bridge, packaging the Android AAR,
//! producing the iOS xcframework, end-to-end CI checks.

fn main() -> anyhow::Result<()> {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("build-bridge") => todo!("build native/bridge via CMake"),
        Some("build-aar") => todo!("assemble native/android AAR"),
        Some("build-xcframework") => todo!("assemble native/ios xcframework"),
        Some("ci") => todo!("run full CI suite"),
        Some(other) => anyhow::bail!("unknown xtask: {other}"),
        None => {
            println!(
                "xtask — Flint build automation

Tasks:
  build-bridge      Build native/bridge (C++) via CMake
  build-aar         Assemble Android AAR from native/android
  build-xcframework Assemble iOS xcframework from native/ios
  ci                Run full CI suite"
            );
            Ok(())
        }
    }
}
