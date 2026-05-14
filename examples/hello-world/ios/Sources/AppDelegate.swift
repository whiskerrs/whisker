import UIKit
import WhiskerRuntime

/// iOS host for the `hello-world` example. All of the app logic lives in
/// `examples/hello-world/src/lib.rs` (`#[whisker::main] fn app()`). This
/// AppDelegate is the bare minimum the iOS runtime needs: a UIWindow
/// hosting a `WhiskerViewController` (provided by the WhiskerRuntime SPM
/// package).
@UIApplicationMain
class AppDelegate: WhiskerAppDelegate {
}
