# whisker-splash-screen

Whisker **config plugin** that generates the native splash screen from a
build-time config, mirroring
[`expo-splash-screen`](https://docs.expo.dev/versions/latest/sdk/splash-screen/).

```rust
// whisker.rs
use whisker_splash_screen::{WhiskerSplashScreen, ResizeMode};

app.plugin::<WhiskerSplashScreen>(|c| {
    c.image("assets/splash.png")
        .resize_mode(ResizeMode::Contain)
        .background_color("#ffffff");
});
```

- **Android** (Android 12 `SplashScreen`): generates a splash theme
  (`windowSplashScreenBackground` / `windowSplashScreenAnimatedIcon`),
  points the app theme at it, injects `installSplashScreen()` into
  `MainActivity`, and adds the `androidx.core:core-splashscreen` backport.
- **iOS** (`UILaunchScreen`, iOS 14+): sets the `UILaunchScreen` plist
  dict + emits `.colorset` / `.imageset` under `Assets.xcassets`.

Static splash (auto-hides on first frame). Imperative
`preventAutoHide`/`hide` is a future increment. `image_width` is not yet
honored exactly on iOS (the image is emitted `@3x`).
