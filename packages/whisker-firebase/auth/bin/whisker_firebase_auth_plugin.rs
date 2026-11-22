#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn main() -> anyhow::Result<()> {
    whisker_plugin::project::protocol::run_as_subprocess(whisker_firebase_auth::WhiskerFirebaseAuth)
}

// The plugin runs on the build host only.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn main() {}
