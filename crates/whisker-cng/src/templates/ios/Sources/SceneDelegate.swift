import UIKit
import WhiskerModules

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let background = UIColor(named: "WhiskerBackground") ?? .white
        let root = UIViewController()
        root.view.backgroundColor = background
        let whiskerView = WhiskerView(frame: root.view.bounds)
        whiskerView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        root.view.addSubview(whiskerView)

        let window = UIWindow(windowScene: windowScene)
        window.backgroundColor = background
        window.rootViewController = root
        window.makeKeyAndVisible()
        self.window = window
    }
}
