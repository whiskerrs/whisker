# whisker-status-bar

Whisker platform module for imperative status-bar control, mirroring
[`expo-status-bar`](https://docs.expo.dev/versions/latest/sdk/status-bar/).

```rust
use whisker_status_bar::{WhiskerStatusBar, StatusBarStyle};
let _ = WhiskerStatusBar::set_hidden(true);
let _ = WhiskerStatusBar::set_style(StatusBarStyle::Light);
```

> **Android-only** for now. On iOS the calls are a no-op: the only
> status-bar API reachable from a view-less module is the deprecated
> app-level `UIApplication.setStatusBarHidden`, which corrupts
> `whisker-router`'s transition animations. iOS support is a TODO — it
> needs a view-controller-based `prefersStatusBarHidden` implementation.
