import Foundation

/// Prevents Host element callbacks from re-entering Rust while a frame is being presented.
final class HostEventGate {
    private let schedule: (@escaping () -> Void) -> Void
    private var isApplyingFrame = false
    private var deferred: [() -> Void] = []

    init(schedule: @escaping (@escaping () -> Void) -> Void) {
        self.schedule = schedule
    }

    func beginFrame() {
        precondition(!isApplyingFrame)
        isApplyingFrame = true
    }

    func endFrame() {
        precondition(isApplyingFrame)
        isApplyingFrame = false
        guard !deferred.isEmpty else { return }
        let events = deferred
        deferred.removeAll(keepingCapacity: true)
        schedule {
            events.forEach { $0() }
        }
    }

    func dispatch(_ event: @escaping () -> Void) {
        if isApplyingFrame {
            deferred.append(event)
        } else {
            event()
        }
    }
}
