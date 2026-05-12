import UIKit

/// Hosts a `FlintView` and forwards lifecycle events.
open class FlintViewController: UIViewController {

    public private(set) var flintView: FlintView?

    public override func loadView() {
        let v = FlintView(frame: UIScreen.main.bounds)
        flintView = v
        view = v
    }

    public override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        flintView?.onEnterForeground()
    }

    public override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        flintView?.onEnterBackground()
    }
}
