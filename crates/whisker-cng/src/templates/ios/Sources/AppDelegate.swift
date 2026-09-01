import UIKit
import WhiskerModules

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        WhiskerModuleBehaviors.registerAll()
        let background = UIColor(named: "WhiskerBackground") ?? .white
        let root = UIViewController()
        root.view.backgroundColor = background
        let whiskerView = WhiskerView(frame: root.view.bounds)
        whiskerView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        root.view.addSubview(whiskerView)

        let window = UIWindow(frame: UIScreen.main.bounds)
        window.backgroundColor = background
        window.rootViewController = root
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}
