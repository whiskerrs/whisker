import UIKit

/// Base AppDelegate for Whisker apps.
///
/// CNG-generated `AppDelegate` extends this. Sets up a window with a
/// `WhiskerViewController`, which attaches the Rust runtime.
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
