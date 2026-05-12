import UIKit
import LyraRuntime

/// Phase 0 minimal iOS host.
///
/// `LyraAppDelegate` sets up a window with `LyraViewController`, which hosts
/// a `LyraView` that displays "Hello, Lyra". No Rust, no Lynx — this is purely
/// to verify the SPM distribution path works end-to-end.
@UIApplicationMain
class AppDelegate: LyraAppDelegate {
}
