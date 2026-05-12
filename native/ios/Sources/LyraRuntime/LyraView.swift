import UIKit
import LyraMobile

/// Hosts the Lyra runtime on iOS.
///
/// **Phase 1**: shows a greeting fetched from the Rust runtime via the
/// `LyraMobile` C ABI. No Lynx yet — the parent class is still a plain
/// `UIView`. Phase 2 will switch to inheriting from `LynxView`.
public final class LyraView: UIView {

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .systemBackground
        installGreetingLabel()
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    private func installGreetingLabel() {
        let label = UILabel()
        label.text = greetingFromRust()
        label.font = .systemFont(ofSize: 32, weight: .semibold)
        label.textColor = .label
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)

        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    /// Calls into the Rust static library via the C ABI exposed in
    /// `lyra-mobile`. The pointer references static storage on the Rust
    /// side, so no free is required.
    private func greetingFromRust() -> String {
        guard let cstr = lyra_mobile_greeting() else {
            return "<null from Rust>"
        }
        return String(cString: cstr)
    }

    /// Stub. Will forward foreground transitions to the Rust runtime once wired.
    public func onEnterForeground() {}

    /// Stub. Will forward background transitions to the Rust runtime once wired.
    public func onEnterBackground() {}
}
