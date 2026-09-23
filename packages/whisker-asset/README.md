# whisker-asset

Bundle assets from an app crate and resolve their runtime paths on Android, iOS, macOS, Windows, Linux, and Web.

```rust,ignore
// whisker.rs
whisker_cng::run(|app| {
    app.project_plugin::<whisker_asset::WhiskerAsset>(|assets| {
        assets.dir("assets");
        assets.file("branding/logo.png");
    });
});
```

`dir("assets")` preserves paths beneath that directory: `assets/photos/cover.jpg` becomes `photos/cover.jpg`. `file("branding/logo.png")` uses the basename `logo.png`. Inputs must be relative to the app crate. Missing inputs, symlinks, non-regular files, and duplicate logical paths are errors. Empty directories contribute nothing.

In application code, `asset!("photos/cover.jpg")` checks for a file beneath the crate's `assets/` directory at compile time and calls `resolve`. Use `resolve("logo.png")` for explicitly bundled files outside that macro root. `asset_bytes!` and `asset_str!` embed file contents directly in Rust and do not require bundling.

| Platform | Bundled location | `resolve("photos/cover.jpg")` |
|---|---|---|
| Android | Main application module's `src/main/assets/whisker/` | `file:///android_asset/whisker/photos/cover.jpg` |
| iOS | Application bundle's `whisker_assets/` folder | Absolute bundle path ending in `whisker_assets/photos/cover.jpg` |
| macOS | `Contents/Resources/whisker_assets/` in the application bundle | Absolute path resolved from the bundled executable |
| Windows | `whisker_assets/` beside the application executable | Absolute path from the executable directory |
| Linux | `share/<executable>/whisker_assets/` below the installation prefix | Absolute path derived from `bin/<executable>` |
| Web | Distribution root, with logical subdirectories preserved | `/photos/cover.jpg`, or `/app/photos/cover.jpg` with `Config.web.base_path("/app/")` |

On Web, the plugin emits `whisker-asset-base` metadata using the current IR's deployment prefix. The resolver reads it independently of the current route and percent-encodes asset path segments, including spaces, Unicode, `#`, and `%`. Explicit `set_base` overrides this automatic base. Without a base or generated metadata, `resolve` returns the normalized relative path. Web Workers do not have a document; set `AssetBase::WebUrl` explicitly when resolving there. Serving the distribution beneath the configured prefix remains the hosting environment's responsibility.

The plugin uses `protocol = "project"`. Migrate previous `app.plugin::<WhiskerAsset>` calls to `app.project_plugin::<WhiskerAsset>`; the `dir` and `file` options are unchanged. It contributes `AppFile` declarations plus platform resource/source-set metadata through `Merge`. The renderer handles copying, file modes, fingerprints, and output conflicts. Native module initialization for Android/iOS is unchanged.

Web assets cannot claim outputs owned by the host, such as `index.html`, `whisker_app.js`, `whisker_app_bg.wasm`, or `snippets/`. Asset files are public distribution contents; only declare files intended for inclusion in the application.

Windows/Linux use the application executable destination in the final IR. Linux requires the application directly under `bin/`; its assets are isolated by executable filename under `share/`. Both runtime bases are derived from `current_exe`, independent of the working directory. Preserve the generated distribution layout when moving or installing the app. Raw Cargo output without its assets retains the relative fallback.

On macOS, the resolver discovers the asset folder beside the executable inside the `.app` once per process. Running the raw Cargo executable outside a bundle retains the relative fallback. An explicit `set_base` takes precedence.
