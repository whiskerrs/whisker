import UIKit
import WhiskerModule

/// Minimal native control used as the first RFC0004 iOS element.
public final class ToggleView: WhiskerUI<UISwitch> {
    private var applyingProperty = false

    @objc public override func createView() -> UISwitch {
        let control = UISwitch()
        control.addTarget(self, action: #selector(valueChanged(_:)), for: .valueChanged)
        return control
    }

    public func setChecked(_ value: Bool) {
        applyingProperty = true
        self.view().setOn(value, animated: false)
        applyingProperty = false
    }

    public func setDisabled(_ value: Bool) {
        self.view().isEnabled = !value
    }

    @objc private func valueChanged(_ sender: UISwitch) {
        guard !applyingProperty else { return }
        WhiskerCustomEvent.dispatch(
            from: self,
            name: "change",
            params: ["checked": sender.isOn]
        )
    }
}
