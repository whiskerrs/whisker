import UIKit
import WhiskerRuntime

// iOS host. All app logic lives in `src/lib.rs` (`#[whisker::main] fn app()`).
// This AppDelegate is the bare minimum: a UIWindow hosting a
// `WhiskerViewController` (provided by the WhiskerRuntime SPM package).
@UIApplicationMain
class AppDelegate: WhiskerAppDelegate {
}
