import UIKit

/// Hosts the Lyra runtime on iOS.
///
/// **Phase 0**: Pure UIKit placeholder that displays "Hello, Lyra" centered.
/// No Lynx, no Rust. Validates the SPM distribution path end-to-end.
///
/// In later phases this will:
/// - Inherit from `LynxView` (when Lynx binary is wired in)
/// - Hand its underlying engine shell to the Rust runtime via FFI
/// - Receive and forward lifecycle events
public final class LyraView: UIView {

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .systemBackground
        installPlaceholderLabel()
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    private func installPlaceholderLabel() {
        let label = UILabel()
        label.text = "Hello, Lyra"
        label.font = .systemFont(ofSize: 32, weight: .semibold)
        label.textColor = .label
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)

        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    /// Stub. Will forward foreground transitions to the Rust runtime once wired.
    public func onEnterForeground() {}

    /// Stub. Will forward background transitions to the Rust runtime once wired.
    public func onEnterBackground() {}
}
