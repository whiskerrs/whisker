import UIKit

/// Base AppDelegate for Whisker apps.
///
/// CNG-generated `AppDelegate` extends this. In Phase 0 we just set up a
/// window with a `WhiskerViewController`. Later phases will:
/// - Initialize the Rust runtime via FFI
/// - Provide injection points for plugins (camera, push notifications, …)
open class WhiskerAppDelegate: UIResponder, UIApplicationDelegate {
    public var window: UIWindow?

    open func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = WhiskerViewController()
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}
