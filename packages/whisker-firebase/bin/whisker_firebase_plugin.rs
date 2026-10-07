fn main() -> anyhow::Result<()> {
    whisker_plugin::project::protocol::run_as_subprocess(whisker_firebase::WhiskerFirebase)
}
