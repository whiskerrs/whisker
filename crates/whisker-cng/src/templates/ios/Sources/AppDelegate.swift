import UIKit

// Minimal UIKit Host shell. The retained Rust renderer and its frame protocol
// are connected in the next mobile slice; app startup has no Lynx/SPM runtime.
@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        let root = UIViewController()
        root.view.backgroundColor = UIColor(
            red: 32.0 / 255.0,
            green: 36.0 / 255.0,
            blue: 42.0 / 255.0,
            alpha: 1.0
        )

        let label = UILabel()
        label.translatesAutoresizingMaskIntoConstraints = false
        label.text = "{{app_name}}"
        label.textColor = UIColor(
            red: 245.0 / 255.0,
            green: 247.0 / 255.0,
            blue: 250.0 / 255.0,
            alpha: 1.0
        )
        label.font = .systemFont(ofSize: 18.0)
        root.view.addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: root.view.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: root.view.centerYAnchor),
        ])

        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = root
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}
