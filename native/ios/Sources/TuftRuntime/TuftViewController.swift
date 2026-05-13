import UIKit

/// Hosts a `TuftView` and forwards lifecycle events.
open class TuftViewController: UIViewController {

    public private(set) var tuftView: TuftView?

    public override func loadView() {
        let v = TuftView(frame: UIScreen.main.bounds)
        tuftView = v
        view = v
    }

    public override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        tuftView?.onEnterForeground()
    }

    public override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        tuftView?.onEnterBackground()
    }
}
