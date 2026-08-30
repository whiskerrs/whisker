import Foundation
import WhiskerModule

struct PendingModuleEvent: Equatable {
    let epoch: UInt64
    let module: String
    let event: String
    let payload: WhiskerValue
}

/// Any-queue ingress buffer drained once per main-loop turn.
final class PendingModuleEvents {
    private let lock = NSLock()
    private var queue: [PendingModuleEvent] = []
    private var flushScheduled = false

    /// Returns true only when the caller must schedule the shared flush.
    func offer(_ event: PendingModuleEvent) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        queue.append(event)
        guard !flushScheduled else { return false }
        flushScheduled = true
        return true
    }

    func drain(epoch: UInt64) -> [PendingModuleEvent] {
        lock.lock()
        defer { lock.unlock() }
        flushScheduled = false
        let current = queue.filter { $0.epoch == epoch }
        queue.removeAll(keepingCapacity: true)
        return current
    }

    func clear() {
        lock.lock()
        queue.removeAll(keepingCapacity: false)
        flushScheduled = false
        lock.unlock()
    }
}
