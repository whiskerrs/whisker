import UIKit
import TuftRuntime

/// iOS host for the `hello-world` example. All of the app logic lives in
/// `examples/hello-world/src/lib.rs` (`#[tuft::main] fn app()`). This
/// AppDelegate is the bare minimum the iOS runtime needs: a UIWindow
/// hosting a `TuftViewController` (provided by the TuftRuntime SPM
/// package).
@UIApplicationMain
class AppDelegate: TuftAppDelegate {
}
