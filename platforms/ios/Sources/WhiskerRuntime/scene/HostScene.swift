import UIKit
import WhiskerModule

/// Owns the transactional UIKit projection of one Whisker surface.
final class HostScene {
    private unowned let root: UIView
    private let resources: HostResourceStore
    private let logicalBounds: () -> CGRect
    private let emitElementEvent: (UInt64, String, WhiskerValue) -> Void
    private var nodes: [UInt64: WhiskerNodeView] = [:]
    private var nodeOrder: [UInt64] = []
    private var parents: [UInt64: UInt64] = [:]
    private var zOrders: [UInt64: Int32] = [:]
    private var sceneEpoch: UInt32 = 0
    private var revision: UInt64 = 0
    private var applyingFrame = false
    private var deferredEvents: [() -> Void] = []

    init(
        root: UIView,
        resources: HostResourceStore,
        logicalBounds: @escaping () -> CGRect,
        emitElementEvent: @escaping (UInt64, String, WhiskerValue) -> Void
    ) {
        self.root = root
        self.resources = resources
        self.logicalBounds = logicalBounds
        self.emitElementEvent = emitElementEvent
    }

    func applyFrame(
        _ frame: WhiskerMobileFrame,
        response: inout WhiskerMobileApplyResponse
    ) -> Bool {
        guard frame.abi_major == UInt16(WHISKER_MOBILE_ABI_MAJOR),
              frame.protocol_major == 1,
              let operations = frame.operations else {
            response.status = UInt8(WHISKER_APPLY_REJECTED)
            response.revision = revision
            return true
        }
        if frame.mode == UInt8(WHISKER_FRAME_DELTA) &&
            (frame.scene_epoch != sceneEpoch || frame.base_revision != revision) {
            response.status = UInt8(WHISKER_APPLY_NEED_SNAPSHOT)
            response.revision = revision
            return true
        }
        if frame.mode == UInt8(WHISKER_FRAME_SNAPSHOT) && frame.base_revision != 0 {
            response.status = UInt8(WHISKER_APPLY_REJECTED)
            response.revision = revision
            return true
        }

        let values = Array(UnsafeBufferPointer(start: operations, count: frame.operation_count))
        guard validate(values, snapshot: frame.mode == UInt8(WHISKER_FRAME_SNAPSHOT)) else {
            response.status = UInt8(WHISKER_APPLY_REJECTED)
            response.revision = revision
            return true
        }
        applyingFrame = true
        defer {
            applyingFrame = false
            let events = deferredEvents
            deferredEvents.removeAll()
            events.forEach { $0() }
        }
        if frame.mode == UInt8(WHISKER_FRAME_SNAPSHOT) { clear() }
        for operation in values where !apply(operation) {
            response.status = UInt8(WHISKER_APPLY_REJECTED)
            response.revision = revision
            return true
        }
        attachRoots()
        refreshZOrderProjection()
        sceneEpoch = frame.scene_epoch
        revision = frame.target_revision
        response.status = UInt8(WHISKER_APPLY_ACCEPTED)
        response.revision = revision
        return true
    }

    func dispatchOrDefer(_ event: @escaping () -> Void) {
        if applyingFrame { deferredEvents.append(event) } else { event() }
    }

    func clear() {
        nodes.values.forEach { $0.mountedElement?.dispose() }
        nodes.values.forEach { $0.removeFromSuperview() }
        nodes.removeAll()
        nodeOrder.removeAll()
        parents.removeAll()
        zOrders.removeAll()
    }

    private func validate(_ operations: [WhiskerMobileOperation], snapshot: Bool) -> Bool {
        var existing = snapshot ? Set<UInt64>() : Set(nodes.keys)
        var stagedParents = snapshot ? [:] : parents
        var elementTypes: [UInt64: Int] = snapshot ? [:] : Dictionary(
            uniqueKeysWithValues: nodes.compactMap { id, node in
                node.mountedElement.map { (id, $0.registration.elementType) }
            }
        )
        for operation in operations {
            switch operation.tag {
            case UInt32(WHISKER_OP_CREATE):
                guard operation.node != 0, !existing.contains(operation.node),
                      WhiskerElementRegistry.registration(Int(operation.member)) != nil
                else { return false }
                existing.insert(operation.node)
                elementTypes[operation.node] = Int(operation.member)
            case UInt32(WHISKER_OP_DELETE):
                guard existing.remove(operation.node) != nil else { return false }
                elementTypes.removeValue(forKey: operation.node)
                stagedParents = stagedParents.filter {
                    $0.key != operation.node && $0.value != operation.node
                }
            case UInt32(WHISKER_OP_INSERT):
                guard existing.contains(operation.parent), existing.contains(operation.child),
                      stagedParents[operation.child] == nil,
                      elementTypes[operation.parent]
                        .flatMap({ WhiskerElementRegistry.registration($0) })?
                        .childPolicy.acceptsElements == true
                else { return false }
                stagedParents[operation.child] = operation.parent
            case UInt32(WHISKER_OP_REMOVE):
                guard stagedParents[operation.child] == operation.parent else { return false }
                stagedParents.removeValue(forKey: operation.child)
            case UInt32(WHISKER_OP_MOVE):
                guard stagedParents[operation.child] == operation.parent else { return false }
            case UInt32(WHISKER_OP_LAYOUT), UInt32(WHISKER_OP_PAINT),
                 UInt32(WHISKER_OP_TEXT), UInt32(WHISKER_OP_PROPERTY):
                guard existing.contains(operation.node), operation.payload != nil else { return false }
            case UInt32(WHISKER_OP_TRANSFORM):
                guard existing.contains(operation.node), operation.payload != nil,
                      operation.payload_count == 16 else { return false }
                let values = UnsafeBufferPointer(
                    start: operation.payload?.assumingMemoryBound(to: Float.self),
                    count: 16
                )
                guard values.allSatisfy(\.isFinite) else { return false }
            case UInt32(WHISKER_OP_BACKGROUND_LAYERS):
                guard existing.contains(operation.node) else { return false }
                if operation.payload_count == 0 {
                    guard operation.payload == nil else { return false }
                    continue
                }
                guard (1...4_096).contains(operation.payload_count),
                      let pointer = operation.payload?.assumingMemoryBound(
                          to: WhiskerMobileBackgroundLayer.self
                      ) else {
                    return false
                }
                let layers = UnsafeBufferPointer(
                    start: pointer,
                    count: operation.payload_count
                )
                guard layers.allSatisfy({ validBackgroundLayer($0, resources: resources) }) else {
                    return false
                }
            case UInt32(WHISKER_OP_OPACITY):
                guard existing.contains(operation.node), operation.scalar.isFinite,
                      (0...1).contains(operation.scalar) else { return false }
            case UInt32(WHISKER_OP_VISIBILITY):
                guard existing.contains(operation.node),
                      operation.integer == 0 || operation.integer == 1 else { return false }
            case UInt32(WHISKER_OP_CLIP), UInt32(WHISKER_OP_Z_ORDER),
                 UInt32(WHISKER_OP_CLEAR_PROPERTY), UInt32(WHISKER_OP_EVENT_MASK):
                guard existing.contains(operation.node) else { return false }
            default:
                return false
            }
        }
        return true
    }

    private func apply(_ operation: WhiskerMobileOperation) -> Bool {
        let id = operation.node
        switch operation.tag {
        case UInt32(WHISKER_OP_CREATE):
            guard let registration = WhiskerElementRegistry.registration(Int(operation.member)),
                  let mounted = WhiskerElementRegistry.mount(
                      Int(operation.member),
                      eventSink: { [weak self] event, detail in
                          self?.emitElementEvent(id, event.name, detail)
                      }
                  )
            else { return false }
            let node = WhiskerNodeView(element: registration.name)
            node.mountedElement = mounted
            mounted.view.frame = node.bounds
            mounted.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            node.addSubview(mounted.view)
            node.mountedContentDidInstall()
            nodes[id] = node
            nodeOrder.append(id)
        case UInt32(WHISKER_OP_DELETE):
            deleteNode(id)
        case UInt32(WHISKER_OP_INSERT), UInt32(WHISKER_OP_MOVE):
            insertChild(parent: operation.parent, child: operation.child, index: Int(operation.index))
        case UInt32(WHISKER_OP_REMOVE):
            detachChild(parent: operation.parent, child: operation.child)
        case UInt32(WHISKER_OP_LAYOUT):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerMobileLayoutGeometry.self).pointee
            else { return false }
            applyLayout(id, nodes[id], payload)
        case UInt32(WHISKER_OP_PAINT):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerMobileBoxPaint.self).pointee
            else { return false }
            applyPaint(nodes[id], payload)
        case UInt32(WHISKER_OP_CLIP):
            nodes[id]?.setOverflowClip(
                horizontal: operation.flags & 1 != 0,
                vertical: operation.flags & 2 != 0
            )
        case UInt32(WHISKER_OP_TRANSFORM):
            guard let payload = operation.payload else { return false }
            nodes[id]?.setPresentationTransform(UnsafeBufferPointer(
                start: payload.assumingMemoryBound(to: Float.self),
                count: 16
            ))
        case UInt32(WHISKER_OP_BACKGROUND_LAYERS):
            guard let node = nodes[id] else { return false }
            if operation.payload_count == 0 {
                node.boxPainter.updateBackgroundLayers([])
                node.setNeedsDisplay()
                return true
            }
            guard let pointer = operation.payload?.assumingMemoryBound(
                to: WhiskerMobileBackgroundLayer.self
            ) else { return false }
            let rawLayers = UnsafeBufferPointer(start: pointer, count: operation.payload_count)
            let layers = rawLayers.compactMap { hostBackgroundLayer($0, resources: resources) }
            guard layers.count == rawLayers.count else { return false }
            node.boxPainter.updateBackgroundLayers(layers)
            node.setNeedsDisplay()
        case UInt32(WHISKER_OP_OPACITY):
            nodes[id]?.alpha = CGFloat(operation.scalar)
        case UInt32(WHISKER_OP_VISIBILITY):
            nodes[id]?.isHidden = operation.integer == 0
        case UInt32(WHISKER_OP_Z_ORDER):
            zOrders[id] = operation.integer
        case UInt32(WHISKER_OP_TEXT):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerMobileText.self).pointee
            else { return false }
            applyText(nodes[id], payload)
        case UInt32(WHISKER_OP_PROPERTY):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerValueRaw.self).pointee
            else { return false }
            nodes[id]?.mountedElement?.setProperty(
                Int(operation.member),
                value: WhiskerValue.from(raw: payload)
            )
        case UInt32(WHISKER_OP_CLEAR_PROPERTY):
            nodes[id]?.mountedElement?.clearProperty(Int(operation.member))
        case UInt32(WHISKER_OP_EVENT_MASK):
            nodes[id]?.mountedElement?.setEventMask(operation.wide)
        default:
            return false
        }
        return true
    }

    private func attachRoots() {
        for id in nodeOrder where parents[id] == nil {
            guard let node = nodes[id], node.superview !== root else { continue }
            node.removeFromSuperview()
            root.addSubview(node)
        }
    }

    private func insertChild(parent parentID: UInt64, child childID: UInt64, index: Int) {
        guard let parent = nodes[parentID], let child = nodes[childID] else { return }
        guard let mounted = parent.mountedElement else {
            preconditionFailure("parent element is not mounted")
        }
        precondition(
            mounted.registration.childPolicy.acceptsElements,
            "\(mounted.registration.name) does not accept element children"
        )
        child.removeFromSuperview()
        parents[childID] = parentID
        let childrenHost = parent.sceneChildrenHost()
        childrenHost.insertSubview(child, at: min(max(index, 0), childrenHost.subviews.count))
    }

    private func detachChild(parent parentID: UInt64, child childID: UInt64) {
        guard nodes[parentID] != nil, let child = nodes[childID] else { return }
        child.removeFromSuperview()
        parents.removeValue(forKey: childID)
    }

    private func deleteNode(_ id: UInt64) {
        guard let node = nodes.removeValue(forKey: id) else { return }
        let descendants = nodes.keys.filter { isDescendant($0, of: id) }
        descendants.forEach {
            nodes.removeValue(forKey: $0)?.mountedElement?.dispose()
            parents.removeValue(forKey: $0)
        }
        let removed = Set(descendants).union([id])
        nodeOrder.removeAll { removed.contains($0) }
        removed.forEach { zOrders.removeValue(forKey: $0) }
        parents.removeValue(forKey: id)
        node.mountedElement?.dispose()
        node.removeFromSuperview()
    }

    private func normalizeZOrder(parent parentID: UInt64?) {
        let creationOrder = Dictionary(uniqueKeysWithValues: nodeOrder.enumerated().map { ($1, $0) })
        let host: UIView?
        if let parentID, let parent = nodes[parentID] {
            host = parent.sceneChildrenHost()
        } else {
            host = root
        }
        let idsByView = Dictionary(uniqueKeysWithValues: nodes.map { (ObjectIdentifier($1), $0) })
        let siblingPairs: [(UInt64, Int)] = (host?.subviews ?? []).enumerated()
            .compactMap { index, view in
                guard let id = idsByView[ObjectIdentifier(view)] else { return nil }
                return (id, index)
            }
        let siblingOrder = Dictionary(uniqueKeysWithValues: siblingPairs)
        let siblings = nodeOrder.filter { id in
            nodes[id] != nil && parents[id] == parentID
        }.sorted { left, right in
            let leftZ = zOrders[left] ?? 0
            let rightZ = zOrders[right] ?? 0
            if leftZ != rightZ { return leftZ < rightZ }
            return siblingOrder[left, default: creationOrder[left, default: 0]] <
                siblingOrder[right, default: creationOrder[right, default: 0]]
        }
        for id in siblings {
            guard let node = nodes[id] else { continue }
            // `CALayer.render(in:)`, used by deterministic Host capture as
            // well as some UIKit snapshot paths, preserves sublayer order but
            // does not reliably sort extreme signed zPosition values. Project
            // protocol z-order into stable UIView sibling order instead.
            node.layer.zPosition = 0
            if node.superview === host { host?.bringSubviewToFront(node) }
        }
    }

    private func refreshZOrderProjection() {
        normalizeZOrder(parent: nil)
        for parentID in Set(parents.values).sorted() where nodes[parentID] != nil {
            normalizeZOrder(parent: parentID)
        }
    }

    private func isDescendant(_ candidate: UInt64, of ancestor: UInt64) -> Bool {
        var current = parents[candidate]
        while let value = current {
            if value == ancestor { return true }
            current = parents[value]
        }
        return false
    }

    private func applyLayout(
        _ id: UInt64,
        _ node: WhiskerNodeView?,
        _ geometry: WhiskerMobileLayoutGeometry
    ) {
        guard let node else { return }
        var frame = hostRect(geometry.border)
        if let parentID = parents[id],
           let parent = nodes[parentID],
           parent.mountedElement?.childrenHost() != nil {
            frame.origin.x -= parent.contentFrame.origin.x
            frame.origin.y -= parent.contentFrame.origin.y
        } else if parents[id] == nil {
            frame.origin.x += logicalBounds().origin.x
            frame.origin.y += logicalBounds().origin.y
        }
        node.setLayoutFrame(frame)
        node.contentFrame = hostRect(geometry.content)
        node.superview?.setNeedsLayout()
        node.superview?.superview?.setNeedsLayout()
        if let paint = node.paint { applyPaint(node, paint) }
        node.setNeedsLayout()
        node.setNeedsDisplay()
    }

    private func applyText(_ node: WhiskerNodeView?, _ content: WhiskerMobileText) {
        guard let node, let mounted = node.mountedElement else { return }
        guard mounted.setText(WhiskerTextContent(
            value: hostString(content.text),
            fontSize: CGFloat(content.font_size),
            fontWeight: Int(content.font_weight),
            color: parsePaintColor(content.color)
        )) else {
            preconditionFailure(
                "text operation sent to element \(mounted.registration.name) without a text implementation"
            )
        }
    }

    private func applyPaint(_ node: WhiskerNodeView?, _ raw: WhiskerMobileBoxPaint) {
        guard let node else { return }
        applyPaint(node, HostBoxPaint(raw))
    }

    private func applyPaint(_ node: WhiskerNodeView, _ paint: HostBoxPaint) {
        node.paint = paint
        node.boxPainter.update(paint, bounds: node.bounds)
        node.boxPaintDidChange()
        node.setNeedsLayout()
    }
}

private func hostRect(_ value: WhiskerMobileRect) -> CGRect {
    CGRect(
        x: CGFloat(value.x),
        y: CGFloat(value.y),
        width: CGFloat(value.width),
        height: CGFloat(value.height)
    )
}

private func validGradientStops(
    _ payload: UnsafeRawPointer,
    count: Int,
    requiresFractionOnly: Bool = false
) -> Bool {
    guard (2...4_096).contains(count) else { return false }
    let stops = UnsafeBufferPointer(
        start: payload.assumingMemoryBound(to: WhiskerMobileGradientStop.self),
        count: count
    )
    return stops.allSatisfy { stop in
        stop.position.isFinite && (!requiresFractionOnly || stop.position.length == 0) &&
            stop.color.kind <= 1 && stop.color.alpha.isFinite &&
            (0...1).contains(stop.color.alpha)
    }
}

private func validBackgroundLayer(
    _ layer: WhiskerMobileBackgroundLayer,
    resources: HostResourceStore
) -> Bool {
    guard validBackgroundGeometry(layer), layer.image.scalar.isFinite else { return false }
    switch layer.image.kind {
    case UInt32(WHISKER_BACKGROUND_RESOURCE):
        guard layer.image.payload_count == 1,
              let resource = layer.image.payload?.assumingMemoryBound(to: UInt64.self).pointee,
              resource != 0 else { return false }
        return resources.rasterImage(id: resource) != nil
    case UInt32(WHISKER_BACKGROUND_LINEAR):
        return layer.image.payload.map {
            validGradientStops($0, count: layer.image.payload_count)
        } ?? false
    case UInt32(WHISKER_BACKGROUND_RADIAL):
        guard layer.image.payload_count == 1,
              let radial = layer.image.payload?.assumingMemoryBound(
                  to: WhiskerMobileRadialGradient.self
              ).pointee,
              radial.center_x.isFinite, radial.center_y.isFinite,
              radial.radius_x.isFinite, radial.radius_y.isFinite,
              (2...4_096).contains(radial.stop_count),
              let stops = radial.stops else { return false }
        return validGradientStops(UnsafeRawPointer(stops), count: radial.stop_count)
    case UInt32(WHISKER_BACKGROUND_CONIC):
        guard layer.image.payload_count == 1,
              let conic = layer.image.payload?.assumingMemoryBound(
                  to: WhiskerMobileConicGradient.self
              ).pointee,
              conic.center_x.isFinite, conic.center_y.isFinite,
              (2...4_096).contains(conic.stop_count),
              let stops = conic.stops else { return false }
        return validGradientStops(
            UnsafeRawPointer(stops),
            count: conic.stop_count,
            requiresFractionOnly: true
        )
    default:
        return false
    }
}

private func validBackgroundGeometry(_ layer: WhiskerMobileBackgroundLayer) -> Bool {
    guard layer.position_x.isFinite, layer.position_y.isFinite,
          layer.attachment == UInt32(WHISKER_BACKGROUND_ATTACHMENT_SCROLL),
          layer.blend_mode == UInt32(WHISKER_BACKGROUND_BLEND_NORMAL) else {
        return false
    }
    if layer.size_kind == UInt32(WHISKER_BACKGROUND_SIZE_AUTO) {
        return layer.position_x.isZero && layer.position_y.isZero &&
            layer.size_width.isZero && layer.size_height.isZero &&
            layer.repeat_x == UInt32(WHISKER_BACKGROUND_REPEAT) &&
            layer.repeat_y == UInt32(WHISKER_BACKGROUND_REPEAT) &&
            layer.origin == UInt32(WHISKER_BACKGROUND_BOX_PADDING) &&
            layer.clip == UInt32(WHISKER_BACKGROUND_BOX_BORDER)
    }
    let supportedRepeats = [
        UInt32(WHISKER_BACKGROUND_REPEAT),
        UInt32(WHISKER_BACKGROUND_NO_REPEAT),
        UInt32(WHISKER_BACKGROUND_SPACE),
        UInt32(WHISKER_BACKGROUND_ROUND)
    ]
    return layer.size_kind == UInt32(WHISKER_BACKGROUND_SIZE_EXPLICIT) &&
        layer.size_width.isNonNegativeFinite && layer.size_height.isNonNegativeFinite &&
        supportedRepeats.contains(layer.repeat_x) && supportedRepeats.contains(layer.repeat_y) &&
        [UInt32(WHISKER_BACKGROUND_BOX_BORDER), UInt32(WHISKER_BACKGROUND_BOX_PADDING),
         UInt32(WHISKER_BACKGROUND_BOX_CONTENT)]
            .contains(layer.origin) &&
        [UInt32(WHISKER_BACKGROUND_BOX_BORDER), UInt32(WHISKER_BACKGROUND_BOX_PADDING),
         UInt32(WHISKER_BACKGROUND_BOX_CONTENT)]
            .contains(layer.clip)
}

private func hostBackgroundGeometry(
    _ layer: WhiskerMobileBackgroundLayer
) -> HostBackgroundGeometry? {
    guard validBackgroundGeometry(layer) else { return nil }
    let explicit = layer.size_kind == UInt32(WHISKER_BACKGROUND_SIZE_EXPLICIT)
    return HostBackgroundGeometry(
        positionX: layer.position_x,
        positionY: layer.position_y,
        sizeWidth: explicit ? layer.size_width : nil,
        sizeHeight: explicit ? layer.size_height : nil,
        repeatX: hostBackgroundRepeat(layer.repeat_x),
        repeatY: hostBackgroundRepeat(layer.repeat_y),
        origin: hostBackgroundBox(layer.origin),
        clip: hostBackgroundBox(layer.clip)
    )
}

private func hostBackgroundLayer(
    _ layer: WhiskerMobileBackgroundLayer,
    resources: HostResourceStore
) -> HostBackgroundLayer? {
    guard let geometry = hostBackgroundGeometry(layer) else { return nil }
    let image: HostBackgroundImage
    switch layer.image.kind {
    case UInt32(WHISKER_BACKGROUND_RESOURCE):
        guard layer.image.payload_count == 1,
              let resource = layer.image.payload?.assumingMemoryBound(to: UInt64.self).pointee,
              let raster = resources.rasterImage(id: resource) else { return nil }
        image = .raster(raster)
    case UInt32(WHISKER_BACKGROUND_LINEAR):
        guard let payload = layer.image.payload else { return nil }
        image = .linear(HostLinearGradient(
            angleDegrees: CGFloat(layer.image.scalar),
            stops: UnsafeBufferPointer(
                start: payload.assumingMemoryBound(to: WhiskerMobileGradientStop.self),
                count: layer.image.payload_count
            ).map(HostLinearGradientStop.init)
        ))
    case UInt32(WHISKER_BACKGROUND_RADIAL):
        guard let radial = layer.image.payload?.assumingMemoryBound(
            to: WhiskerMobileRadialGradient.self
        ).pointee, let stopPointer = radial.stops else { return nil }
        image = .radial(HostRadialGradient(
            centerX: radial.center_x,
            centerY: radial.center_y,
            radiusX: radial.radius_x,
            radiusY: radial.radius_y,
            stops: UnsafeBufferPointer(
                start: stopPointer,
                count: radial.stop_count
            ).map(HostLinearGradientStop.init)
        ))
    case UInt32(WHISKER_BACKGROUND_CONIC):
        guard let conic = layer.image.payload?.assumingMemoryBound(
            to: WhiskerMobileConicGradient.self
        ).pointee, let stopPointer = conic.stops else { return nil }
        image = .conic(HostConicGradient(
            fromDegrees: CGFloat(layer.image.scalar),
            centerX: conic.center_x,
            centerY: conic.center_y,
            stops: UnsafeBufferPointer(
                start: stopPointer,
                count: conic.stop_count
            ).map(HostLinearGradientStop.init)
        ))
    default:
        return nil
    }
    return HostBackgroundLayer(image: image, geometry: geometry)
}

private func hostBackgroundRepeat(_ value: UInt32) -> HostBackgroundRepeat {
    switch value {
    case UInt32(WHISKER_BACKGROUND_NO_REPEAT): .noRepeat
    case UInt32(WHISKER_BACKGROUND_SPACE): .space
    case UInt32(WHISKER_BACKGROUND_ROUND): .round
    default: .repeat
    }
}

private func hostBackgroundBox(_ value: UInt32) -> HostBackgroundBox {
    switch value {
    case UInt32(WHISKER_BACKGROUND_BOX_BORDER): .border
    case UInt32(WHISKER_BACKGROUND_BOX_CONTENT): .content
    default: .padding
    }
}

private extension WhiskerMobileLengthPercentage {
    var isFinite: Bool { length.isFinite && fraction.isFinite }
    var isZero: Bool { length == 0 && fraction == 0 }
    var isNonNegativeFinite: Bool {
        isFinite && length >= 0 && fraction >= 0
    }
}
