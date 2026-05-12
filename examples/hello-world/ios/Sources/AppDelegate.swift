import UIKit
import LyraRuntime

/// iOS host for the `hello-world` example. All of the app logic lives in
/// `examples/hello-world/src/lib.rs` (`#[lyra::main] fn app()`). This
/// AppDelegate is the bare minimum the iOS runtime needs: a UIWindow
/// hosting a `LyraViewController` (provided by the LyraRuntime SPM
/// package).
@UIApplicationMain
class AppDelegate: LyraAppDelegate {
}
