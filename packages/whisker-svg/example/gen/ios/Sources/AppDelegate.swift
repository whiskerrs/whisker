import UIKit
import WhiskerRuntime
import WhiskerModules

// iOS host. All app logic lives in `src/lib.rs` (`#[whisker::main] fn app()`).
// This AppDelegate is the bare minimum: a UIWindow hosting a
// `WhiskerViewController` (provided by the WhiskerRuntime SPM package).
//
// `WhiskerModuleBehaviors.registerAll()` walks every `[ios].behaviors`
// entry from each Whisker module crate (see `gen/ios/whisker_modules/
// Sources/WhiskerModules/WhiskerModuleBehaviors.swift`) and registers
// each `LynxUI` subclass with Lynx's behaviour registry before the
// first `WhiskerView` is constructed. Mirrors the Android path's
// `WhiskerModuleBehaviors.registerAll()` invocation from
// `<App>Application.onCreate`.
@UIApplicationMain
class AppDelegate: WhiskerAppDelegate {
    override func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        WhiskerModuleBehaviors.registerAll()
        return super.application(application, didFinishLaunchingWithOptions: launchOptions)
    }
}
