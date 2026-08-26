import UIKit

func logicalPointerPosition(_ location: CGPoint, viewport: CGRect) -> CGPoint {
    CGPoint(x: location.x - viewport.minX, y: location.y - viewport.minY)
}

enum HostPointerEvent: UInt32, Equatable {
    case down = 0
    case move = 1
    case up = 2
    case cancel = 3

    var buttons: UInt32 {
        switch self {
        case .down, .move: 1
        case .up, .cancel: 0
        }
    }
}

enum HostPointerKind: UInt32, Equatable {
    case mouse = 0
    case touch = 1
    case pen = 2
    case unknown = 3

    func changedButton(for event: HostPointerEvent) -> Int16 {
        guard self == .mouse || self == .pen else { return -1 }
        switch event {
        case .down, .up: return 0
        case .move, .cancel: return -1
        }
    }
}

func hostPointerKind(for type: UITouch.TouchType) -> HostPointerKind {
    if #available(iOS 13.4, *), type == .indirectPointer { return .mouse }
    switch type {
    case .direct: return .touch
    case .pencil: return .pen
    case .indirect: return .unknown
    default: return .unknown
    }
}

struct HostTouchIdentityMap {
    private var pointerIDs = [ObjectIdentifier: UInt64]()
    private var nextPointerID: UInt64 = 1

    mutating func begin(_ touch: ObjectIdentifier) -> UInt64 {
        if let existing = pointerIDs[touch] { return existing }
        let result = max(nextPointerID, 1)
        nextPointerID = result &+ 1
        if nextPointerID == 0 { nextPointerID = 1 }
        pointerIDs[touch] = result
        return result
    }

    func existing(_ touch: ObjectIdentifier) -> UInt64? {
        pointerIDs[touch]
    }

    mutating func end(_ touch: ObjectIdentifier) {
        pointerIDs.removeValue(forKey: touch)
    }

    mutating func clear() {
        pointerIDs.removeAll(keepingCapacity: true)
    }
}

/// Passive ancestor observer: it never recognizes, so descendant controls and
/// scroll recognizers continue to receive and arbitrate the original touches.
final class WhiskerTouchObserverGestureRecognizer: UIGestureRecognizer {
    var touchHandler: ((Set<UITouch>, HostPointerEvent) -> Void)?
    private var activeTouches = Set<ObjectIdentifier>()

    override init(target: Any?, action: Selector?) {
        super.init(target: target, action: action)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
    }

    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool {
        false
    }

    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool {
        false
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        activeTouches.formUnion(touches.map(ObjectIdentifier.init))
        touchHandler?(touches, .down)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        touchHandler?(touches, .move)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        touchHandler?(touches, .up)
        activeTouches.subtract(touches.map(ObjectIdentifier.init))
        if activeTouches.isEmpty { state = .failed }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        touchHandler?(touches, .cancel)
        activeTouches.subtract(touches.map(ObjectIdentifier.init))
        if activeTouches.isEmpty { state = .failed }
    }

    override func reset() {
        super.reset()
        activeTouches.removeAll(keepingCapacity: true)
    }
}
