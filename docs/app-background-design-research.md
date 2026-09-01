# Application Background Ownership

Research note for deciding how Whisker should represent the background behind
an application's pages. This separates two public values that are easy to
collapse into one:

1. the static application `background` owned by the generated Host;
2. an individual screen or element `background_color` painted by Whisker.

## React Native

React Native does not define one cross-platform, semantic "application
background" property in the JavaScript API. A rendered screen normally paints
its background with the ordinary [`View` `backgroundColor` style][rn-view-bg].
That is an element property, not an OS-window setting.

The native fallback is a separate Host concern and is not symmetric:

- On Android, `ReactRootView` is a `FrameLayout`. Its initialization assigns a
  root tag and clipping behavior but does not assign a background
  [in the official implementation][rn-android-root]. Ordinary React view
  background drawing defaults to `Color.TRANSPARENT`
  [in `BackgroundDrawable`][rn-android-drawable]. Consequently the Activity
  theme/window can be exposed until React content paints, or through a
  transparent root.
- On iOS, `RCTSurfaceHostingView` explicitly initializes its background to
  white. Its source calls this a backward-compatibility choice instead of the
  transparent OS default [in the official implementation][rn-ios-surface].
- The official community application template creates a full-size root `View`
  but gives it only `flex: 1`; it does not silently synthesize an application
  background in JavaScript [in `App.tsx`][rn-template].

The useful lesson is not RN's exact fallback color. It is that element paint
and native hosting fallback are different layers. RN leaves much of their
coordination to the application/template, which is also why native-to-React
startup flashes are possible when those colors disagree.

## Flutter

Flutter makes the separation more explicit:

- A Flutter Activity is opaque by default. Transparent embedding is an
  explicit background mode and has a documented non-trivial performance cost
  [in the Android embedder API][flutter-background-mode]. On iOS,
  `FlutterViewController.viewOpaque` similarly defaults to `YES`, and the docs
  warn that disabling it may hurt hardware-accelerated performance
  [in the iOS embedder API][flutter-ios-opaque].
- The Android embedder tells applications to use a launch theme and then a
  normal theme whose `windowBackground` is an explicit neutral color behind
  Flutter content. It says that this color is normally unseen but is useful
  for transition edge cases [in `FlutterActivity`'s official docs][flutter-activity].
- The Flutter render root does not itself paint a background; `RenderView.paint`
  only paints its child [in the framework implementation][flutter-render-view].
- A Material page gets its visible background from `Scaffold`. Its
  `backgroundColor` paints the `Material` beneath the entire scaffold and
  defaults to `ThemeData.scaffoldBackgroundColor`
  [in the Material API][flutter-scaffold]. That theme value is described as the
  background for a typical app or page [in `ThemeData`][flutter-theme].
- `WidgetsApp.color` / `MaterialApp.color` is not the rendered page background.
  It is the color reported to the operating-system interface, for example the
  Android app switcher [in the Widgets API][flutter-widgets-app].

Flutter therefore has an opaque rendering surface and a native fallback, while
the actual visual page background remains ordinary framework-level UI paint.
The two colors may be coordinated, but they are not the same property.

## Behavior before this change

Whisker had no explicit persistent application-background contract:

- Android `WhiskerView` sets its Host view background to transparent
  ([source](../platforms/android/runtime/src/main/kotlin/rs/whisker/runtime/WhiskerView.kt)).
- iOS `WhiskerView` sets `backgroundColor = .clear`
  ([source](../platforms/ios/Sources/WhiskerRuntime/WhiskerView.swift)).
- Desktop configures an opaque swapchain but clears each frame to transparent
  ([source](../platforms/desktop/src/gpu/renderer.rs)). With an opaque surface,
  the platform determines what those cleared RGB channels visibly become.
- Web relies on the embedding document around its DOM root; it does not own a
  matching surface-level color.
- Element `background_color` is correctly represented as ordinary box paint in
  the shared frame protocol. It is not a surface fallback.

This is asymmetric and makes an app depend on whatever exists below the
Whisker tree. During a route transition, if both route wrappers are partially
transparent, that unspecified layer becomes visible.

## Decided Whisker model

### 1. Keep one static Host `background`

The application config in `whisker.rs` owns one static `background`. CNG writes
it into the generated Host so the OS can display it before Rust starts and so
unpainted Host pixels remain deterministic afterward. It is not a runtime CSS
property and does not introduce a third public surface-background API.

The same value is projected into each generated Host's native mechanism:

- Android window and splash-screen theme background;
- iOS launch-screen storyboard (including iOS 13), color asset, and
  window/root-view background;
- Web document and mount-node background;
- Desktop window/GPU clear color.

Whisker's rendered content remains transparent wherever no Element paints.
There is no runtime `SurfaceBackground`: transparent content simply reveals
the static Host `background`.

### 2. Keep screen backgrounds as ordinary CSS box paint

A route component should continue to use a viewport-filling `View` with
`background_color`. Different screens may have different backgrounds,
gradients, images, or intentional transparency. The router should not rewrite
that style and the Host should not infer the surface color from an arbitrary
root element.

Inferring it would be ambiguous for gradients, images, animated colors,
partially transparent roots, clipped roots, and multiple mounted roots. It
would also turn ordinary CSS changes into hidden Host configuration writes.

### 3. Do not use the fallback color to hide transition bugs

Route compositing should not accidentally expose the Host background. For the
common two-screen transition, the under screen can remain opaque while the top
screen fades or moves. A separate scrim can dim the under screen without
reducing its alpha. Applications that intentionally want a stable background
behind a router can wrap it in an ordinary background-painted `View`; the
router does not need a separate background API.

Changing Android's window background from white to black only changes a white
flash into a black flash. The surface background makes uncovered pixels
deterministic; the router must independently avoid accidentally uncovering it.

### 4. Test the two values and their boundary independently

Conformance should cover:

1. the generated Host exposes the configured static `background` before the
   first Rust frame and wherever the scene is transparent;
2. element `background_color` remains ordinary box paint;
3. every built-in route transition keeps the intended viewport coverage at
   `1.0` for the full animation;
4. cold start, first frame, resize, reload, and route transition never expose a
   Host-specific default color in a generated standalone app.

## Decision summary

The durable public split is:

| Layer | Owner | Meaning |
| --- | --- | --- |
| `background` | `whisker.rs` / generated Host | static color visible before first frame and behind transparent Whisker content |
| screen/element background | Rust UI via CSS box paint | visible design of a route or element |

RN demonstrates why relying on platform defaults is fragile. Flutter provides
the stronger precedent for a neutral native fallback while page backgrounds
remain normal UI paint. Whisker keeps that ownership split without exposing a
separate runtime surface-background concept.

[rn-view-bg]: https://reactnative.dev/docs/view-style-props#backgroundcolor
[rn-android-root]: https://github.com/facebook/react-native/blob/a344b4b2ceee94849bfe28e3da66aa994a7996d7/packages/react-native/ReactAndroid/src/main/java/com/facebook/react/ReactRootView.java#L120-L139
[rn-android-drawable]: https://github.com/facebook/react-native/blob/a344b4b2ceee94849bfe28e3da66aa994a7996d7/packages/react-native/ReactAndroid/src/main/java/com/facebook/react/uimanager/drawable/BackgroundDrawable.kt#L42-L49
[rn-ios-surface]: https://github.com/facebook/react-native/blob/a344b4b2ceee94849bfe28e3da66aa994a7996d7/packages/react-native/React/Base/Surface/SurfaceHostingView/RCTSurfaceHostingView.mm#L45-L50
[rn-template]: https://github.com/react-native-community/template/blob/main/template/App.tsx#L24-L43
[flutter-background-mode]: https://api.flutter.dev/javadoc/io/flutter/embedding/android/FlutterActivity.CachedEngineIntentBuilder.html#backgroundMode(io.flutter.embedding.android.FlutterActivityLaunchConfigs.BackgroundMode)
[flutter-ios-opaque]: https://api.flutter.dev/ios-embedder/interface_flutter_view_controller.html#a31ee6902e716208e895fc2f564f8844a
[flutter-activity]: https://api.flutter.dev/javadoc/io/flutter/embedding/android/FlutterActivity.html
[flutter-render-view]: https://api.flutter.dev/flutter/rendering/RenderView/paint.html
[flutter-scaffold]: https://api.flutter.dev/flutter/material/Scaffold/backgroundColor.html
[flutter-theme]: https://api.flutter.dev/flutter/material/ThemeData/scaffoldBackgroundColor.html
[flutter-widgets-app]: https://api.flutter.dev/flutter/widgets/WidgetsApp/color.html
