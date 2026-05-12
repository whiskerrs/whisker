import UIKit

/// Hosts a `LyraView` and forwards lifecycle events.
open class LyraViewController: UIViewController {

    public private(set) var lyraView: LyraView?

    public override func loadView() {
        let v = LyraView(frame: UIScreen.main.bounds)
        lyraView = v
        view = v
    }

    public override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        lyraView?.onEnterForeground()
    }

    public override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        lyraView?.onEnterBackground()
    }
}
