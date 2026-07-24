# whisker-haptics

Whisker platform module for haptic feedback, mirroring
[`expo-haptics`](https://docs.expo.dev/versions/latest/sdk/haptics/).

Three stateless one-shot operations on `WhiskerHaptics` (API shape 5):

- `impact(ImpactStyle)` — a physical bump (Light / Medium / Heavy).
- `selection()` — a light tick for discrete value changes.
- `notification(NotificationType)` — success / warning / error pattern.

iOS uses `UIImpact/Selection/NotificationFeedbackGenerator`; Android uses
`Vibrator` / `VibratorManager` (needs `android.permission.VIBRATE`, added
automatically by this crate's plugin).

```rust
use whisker_haptics::{ImpactStyle, WhiskerHaptics};
let _ = WhiskerHaptics::impact(ImpactStyle::Light);
```
