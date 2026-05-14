import UIKit

/// Hosts a `WhiskerView` and forwards lifecycle events.
open class WhiskerViewController: UIViewController {

    public private(set) var whiskerView: WhiskerView?

    public override func loadView() {
        let v = WhiskerView(frame: UIScreen.main.bounds)
        whiskerView = v
        view = v
    }

    public override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        whiskerView?.onEnterForeground()
    }

    public override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        whiskerView?.onEnterBackground()
    }
}
