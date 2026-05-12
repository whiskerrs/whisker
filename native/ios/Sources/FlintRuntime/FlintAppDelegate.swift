import UIKit

/// Base AppDelegate for Flint apps.
///
/// The CNG-generated `AppDelegate` extends this. Initializes the Rust runtime
/// and sets a `FlintViewController` as the root.
///
/// Plugins that need AppDelegate-level hooks declare a `plugin-app-delegate-launch`
/// injection in their `flint_plugin` function.
open class FlintAppDelegate: UIResponder, UIApplicationDelegate {
    public var window: UIWindow?

    public func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        flint_runtime_init()

        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = FlintViewController()
        window.makeKeyAndVisible()
        self.window = window

        return true
    }
}

// Rust runtime exports (linked from the cdylib that ships alongside the framework).
@_silgen_name("flint_runtime_init")
func flint_runtime_init()
