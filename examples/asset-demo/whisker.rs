// `whisker.rs` for the `asset-demo` example.
//
// Registers the `whisker-asset` build plugin so the `assets/` tree is
// bundled into both generated native projects:
//   - iOS:     <bundle>/whisker_assets/images/logo.png  (folder ref)
//   - Android: file:///android_asset/whisker/images/logo.png
//
// At runtime, whisker-asset's native module half installs the matching
// resolver base (iOS bundle dir from Swift at launch; Android constant
// from a Rust `.init_array` ctor), so `asset!("images/logo.png")` in
// `lib.rs` resolves to the loadable path/URL above.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerAssetExample")
        .bundle_id("rs.whisker.assetexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.assetexample")
            .application_id("rs.whisker.assetexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.assetexample")
            .scheme("WhiskerAssetExample")
            .deployment_target("13.0");
    });

    // Bundle the example's `assets/` dir recursively. `c.dir("assets")`
    // is relative to the app crate root (this directory); the single
    // `assets/images/logo.png` lands under the `whisker(_assets)/`
    // namespace on both platforms.
    app.plugin::<whisker_asset::WhiskerAsset>(|c| {
        c.dir("assets");
    });
}
