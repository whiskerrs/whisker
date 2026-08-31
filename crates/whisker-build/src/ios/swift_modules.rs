use std::path::Path;

/// Inputs for invoking xcodebuild on the already-generated iOS Host.
pub struct XcodebuildArgs<'a> {
    pub gen_ios: &'a Path,
    pub scheme: &'a str,
    pub sdk: &'a str,
    pub configuration: &'a str,
    pub xcodeproj_name: &'a str,
    pub derived_data: &'a Path,
    pub whisker_runtime_path: Option<&'a Path>,
    pub whisker_ios_macros_path: Option<&'a Path>,
}
