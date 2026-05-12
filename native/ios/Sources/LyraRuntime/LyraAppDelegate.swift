import UIKit

/// Base AppDelegate for Lyra apps.
///
/// The CNG-generated `AppDelegate` extends this. Initializes the Rust runtime
/// and sets a `LyraViewController` as the root.
///
/// Plugins that need AppDelegate-level hooks declare a `plugin-app-delegate-launch`
/// injection in their `lyra_plugin` function.
open class LyraAppDelegate: UIResponder, UIApplicationDelegate {
    public var window: UIWindow?

    public func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        lyra_runtime_init()

        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = LyraViewController()
        window.makeKeyAndVisible()
        self.window = window

        return true
    }
}

// Rust runtime exports (linked from the cdylib that ships alongside the framework).
@_silgen_name("lyra_runtime_init")
func lyra_runtime_init()
