import UIKit
import WhiskerModule

/// Owns the transactional UIKit projection of one Whisker surface.
final class HostScene {
    private unowned let root: UIView
    private let resources: HostResourceStore
    private let logicalBounds: () -> CGRect
    private let emitElementEvent: (UInt64, String, WhiskerValue) -> Void
    private let updateScrollOffset: (UInt64, CGPoint) -> Void
    private let removeScrollOffset: (UInt64) -> Void
    private var nodes: [UInt64: WhiskerNodeView] = [:]
    private var nodeOrder: [UInt64] = []
    private var parents: [UInt64: UInt64] = [:]
    private var presentationPool: [Int: [WhiskerMountedElement]] = [:]
    private var zOrders: [UInt64: Int32] = [:]
    private var sceneEpoch: UInt32 = 0
    private var revision: UInt64 = 0
    private let eventGate = HostEventGate { event in
        DispatchQueue.main.async(execute: event)
    }

    init(
        root: UIView,
        resources: HostResourceStore,
        logicalBounds: @escaping () -> CGRect,
        emitElementEvent: @escaping (UInt64, String, WhiskerValue) -> Void,
        updateScrollOffset: @escaping (UInt64, CGPoint) -> Void,
        removeScrollOffset: @escaping (UInt64) -> Void
    ) {
        self.root = root
        self.resources = resources
        self.logicalBounds = logicalBounds
        self.emitElementEvent = emitElementEvent
        self.updateScrollOffset = updateScrollOffset
        self.removeScrollOffset = removeScrollOffset
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
        eventGate.beginFrame()
        defer {
            eventGate.endFrame()
        }
        let snapshot = frame.mode == UInt8(WHISKER_FRAME_SNAPSHOT)
        if snapshot { clear() }
        var zOrderParents = Set<UInt64>()
        for operation in values {
            recordZOrderImpact(operation, into: &zOrderParents)
            if !apply(operation) {
                response.status = UInt8(WHISKER_APPLY_REJECTED)
                response.revision = revision
                return true
            }
        }
        attachRoots()
        refreshZOrderProjection(parentKeys: snapshot ? nil : zOrderParents)
        sceneEpoch = frame.scene_epoch
        revision = frame.target_revision
        response.status = UInt8(WHISKER_APPLY_ACCEPTED)
        response.revision = revision
        return true
    }

    func dispatchOrDefer(_ event: @escaping () -> Void) {
        eventGate.dispatch(event)
    }

    func clear() {
        nodes.keys.forEach(removeScrollOffset)
        nodes.values.forEach(releasePresentation)
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
                guard existing.contains(operation.node) else { return false }
                let removed = existing.filter {
                    $0 == operation.node || isStagedDescendant(
                        $0,
                        of: operation.node,
                        parents: stagedParents
                    )
                }
                existing.subtract(removed)
                removed.forEach { elementTypes.removeValue(forKey: $0) }
                let removedSet = Set(removed)
                stagedParents = stagedParents.filter {
                    !removedSet.contains($0.key) && !removedSet.contains($0.value)
                }
            case UInt32(WHISKER_OP_INSERT):
                guard existing.contains(operation.parent), existing.contains(operation.child),
                      stagedParents[operation.child] == nil,
                      !isStagedDescendant(
                          operation.parent,
                          of: operation.child,
                          parents: stagedParents
                      ),
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
            case UInt32(WHISKER_OP_LAYOUT):
                guard existing.contains(operation.node),
                      let geometry = operation.payload?
                        .assumingMemoryBound(to: WhiskerMobileLayoutGeometry.self).pointee,
                      validLayoutGeometry(geometry) else { return false }
            case UInt32(WHISKER_OP_PAINT):
                guard existing.contains(operation.node), operation.payload != nil else { return false }
            case UInt32(WHISKER_OP_PROPERTY):
                guard existing.contains(operation.node),
                      let registration = elementTypes[operation.node]
                        .flatMap({ WhiskerElementRegistry.registration($0) }),
                      let property = registration.property(Int(operation.member)),
                      let payload = operation.payload?
                        .assumingMemoryBound(to: WhiskerValueRaw.self).pointee,
                      property.value.accepts(WhiskerValue.from(raw: payload))
                else { return false }
            case UInt32(WHISKER_OP_COMMAND):
                guard existing.contains(operation.node),
                      let registration = elementTypes[operation.node]
                        .flatMap({ WhiskerElementRegistry.registration($0) }),
                      let command = registration.command(Int(operation.member)),
                      let payload = operation.payload?
                        .assumingMemoryBound(to: WhiskerValueRaw.self).pointee,
                      command.arguments.accepts(WhiskerValue.from(raw: payload))
                else { return false }
            case UInt32(WHISKER_OP_ACCESSIBILITY):
                guard existing.contains(operation.node),
                      let payload = operation.payload?
                        .assumingMemoryBound(to: WhiskerValueRaw.self).pointee,
                      case .map = WhiskerValue.from(raw: payload)
                else { return false }
            case UInt32(WHISKER_OP_TEXT), UInt32(WHISKER_OP_TEXT_STYLE):
                guard existing.contains(operation.node),
                      let registration = elementTypes[operation.node]
                        .flatMap({ WhiskerElementRegistry.registration($0) }),
                      let text = operation.payload?
                        .assumingMemoryBound(to: WhiskerMobileText.self).pointee,
                      text.decoration_flags <= 2,
                      text.decoration_style <= 4,
                      text.alignment <= 4,
                      text.direction <= 2,
                      text.wrap <= 1,
                      text.word_break <= 2,
                      text.overflow <= 1,
                      text.font_style <= 2,
                      text.font_optical_sizing <= 1,
                      text.font_size.isFinite,
                      text.font_size > 0,
                      (1...1_000).contains(Int(text.font_weight)),
                      text.line_height.isFinite,
                      text.line_height >= 0,
                      text.letter_spacing.isFinite,
                      validBorrowedArray(text.font_families, text.font_family_count),
                      validBorrowedArray(text.font_features, text.font_feature_count),
                      validBorrowedArray(text.font_variations, text.font_variation_count),
                      text.font_family_count > 0,
                      text.font_family_count <= 4_096,
                      validBorrowedStrings(text.font_families, text.font_family_count),
                      text.font_feature_count <= 4_096,
                      text.font_variation_count <= 4_096,
                      text.font_family_count + text.font_feature_count
                        + text.font_variation_count <= 4_096,
                      text.indent_logical_pixels.isFinite,
                      text.indent_percentage.isFinite else { return false }
                if operation.tag == UInt32(WHISKER_OP_TEXT),
                   !registration.childPolicy.acceptsPlainText { return false }
                if operation.tag == UInt32(WHISKER_OP_TEXT_STYLE),
                   !registration.textStyle { return false }
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
            case UInt32(WHISKER_OP_BOX_SHADOWS):
                guard existing.contains(operation.node) else { return false }
                if operation.payload_count == 0 {
                    guard operation.payload == nil else { return false }
                    continue
                }
                guard (1...4_096).contains(operation.payload_count),
                      let pointer = operation.payload?.assumingMemoryBound(
                          to: WhiskerMobileBoxShadow.self
                      ) else { return false }
                let shadows = UnsafeBufferPointer(start: pointer, count: operation.payload_count)
                guard shadows.allSatisfy(validHardBoxShadow) else { return false }
            case UInt32(WHISKER_OP_CLIP_PATH):
                guard existing.contains(operation.node) else { return false }
                if operation.payload_count == 0 {
                    guard operation.payload == nil else { return false }
                    continue
                }
                guard operation.payload_count == 1,
                      let clip = operation.payload?.assumingMemoryBound(
                          to: WhiskerMobileClipPath.self
                      ).pointee,
                      validClipPath(clip) else { return false }
            case UInt32(WHISKER_OP_BACKDROP_BLUR):
                guard existing.contains(operation.node), operation.scalar.isFinite,
                      operation.scalar >= 0 else { return false }
            case UInt32(WHISKER_OP_IMAGE_RENDERING):
                guard existing.contains(operation.node),
                      operation.integer == Int32(WHISKER_IMAGE_RENDERING_AUTO) ||
                      operation.integer == Int32(WHISKER_IMAGE_RENDERING_PIXELATED) ||
                      operation.integer == Int32(WHISKER_IMAGE_RENDERING_CRISP_EDGES)
                else { return false }
            case UInt32(WHISKER_OP_HIT_TEST):
                guard existing.contains(operation.node), (0...3).contains(operation.integer)
                else { return false }
            case UInt32(WHISKER_OP_CURSOR):
                guard existing.contains(operation.node), (0...34).contains(operation.integer)
                else { return false }
            case UInt32(WHISKER_OP_CAPTURE), UInt32(WHISKER_OP_RELEASE_CAPTURE):
                guard existing.contains(operation.node), operation.wide != 0 else { return false }
            case UInt32(WHISKER_OP_OPACITY):
                guard existing.contains(operation.node), operation.scalar.isFinite,
                      (0...1).contains(operation.scalar) else { return false }
            case UInt32(WHISKER_OP_VISIBILITY):
                guard existing.contains(operation.node),
                      operation.integer == 0 || operation.integer == 1 else { return false }
            case UInt32(WHISKER_OP_CLEAR_PROPERTY):
                guard existing.contains(operation.node),
                      let registration = elementTypes[operation.node]
                        .flatMap({ WhiskerElementRegistry.registration($0) }),
                      registration.property(Int(operation.member)) != nil
                else { return false }
            case UInt32(WHISKER_OP_CLIP), UInt32(WHISKER_OP_Z_ORDER),
                 UInt32(WHISKER_OP_EVENT_MASK):
                guard existing.contains(operation.node) else { return false }
            default:
                return false
            }
        }
        return true
    }

    private func isStagedDescendant(
        _ candidate: UInt64,
        of ancestor: UInt64,
        parents: [UInt64: UInt64]
    ) -> Bool {
        var current: UInt64? = candidate
        while let node = current {
            if node == ancestor { return true }
            current = parents[node]
        }
        return false
    }

    private func validLayoutGeometry(_ geometry: WhiskerMobileLayoutGeometry) -> Bool {
        validLayoutRect(geometry.border) && validLayoutRect(geometry.content)
    }

    private func validLayoutRect(_ rect: WhiskerMobileRect) -> Bool {
        rect.x.isFinite && rect.y.isFinite && rect.width.isFinite && rect.height.isFinite
            && rect.width >= 0 && rect.height >= 0
    }

    private func apply(_ operation: WhiskerMobileOperation) -> Bool {
        let id = operation.node
        switch operation.tag {
        case UInt32(WHISKER_OP_CREATE):
            let elementType = Int(operation.member)
            guard let registration = WhiskerElementRegistry.registration(elementType) else {
                return false
            }
            let eventSink: WhiskerElementEventSink = { [weak self] event, detail in
                self?.emitElementEvent(id, event.name, detail)
            }
            let mounted: WhiskerMountedElement
            if var pool = presentationPool[elementType], let reused = pool.popLast() {
                presentationPool[elementType] = pool
                reused.prepareForReuse(eventSink: eventSink)
                mounted = reused
            } else if let created = WhiskerElementRegistry.mount(
                elementType,
                eventSink: eventSink
            ) {
                mounted = created
            } else {
                return false
            }
            let node = WhiskerNodeView(element: registration.name)
            node.mountedElement = mounted
            (mounted.view as? WhiskerScrollContainerView)?.installWhiskerPresentationSink {
                [weak self] offset in self?.updateScrollOffset(id, offset)
            }
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
                node.boxPaintDidChange()
                return true
            }
            guard let pointer = operation.payload?.assumingMemoryBound(
                to: WhiskerMobileBackgroundLayer.self
            ) else { return false }
            let rawLayers = UnsafeBufferPointer(start: pointer, count: operation.payload_count)
            let layers = rawLayers.compactMap { hostBackgroundLayer($0, resources: resources) }
            guard layers.count == rawLayers.count else { return false }
            node.boxPainter.updateBackgroundLayers(layers)
            node.boxPaintDidChange()
        case UInt32(WHISKER_OP_BOX_SHADOWS):
            guard let node = nodes[id] else { return false }
            if operation.payload_count == 0 {
                node.setBoxShadows([])
                return true
            }
            guard let pointer = operation.payload?.assumingMemoryBound(
                to: WhiskerMobileBoxShadow.self
            ) else { return false }
            let shadows = UnsafeBufferPointer(start: pointer, count: operation.payload_count).map {
                HostBoxShadow(
                    offset: CGSize(width: CGFloat($0.offset_x), height: CGFloat($0.offset_y)),
                    blurRadius: CGFloat($0.blur_radius),
                    spreadRadius: CGFloat($0.spread_radius),
                    color: parsePaintColor($0.color),
                    inset: $0.inset != 0
                )
            }
            node.setBoxShadows(shadows)
        case UInt32(WHISKER_OP_CLIP_PATH):
            guard let node = nodes[id] else { return false }
            if operation.payload_count == 0 {
                node.setClipPath(nil)
                return true
            }
            guard let raw = operation.payload?.assumingMemoryBound(
                to: WhiskerMobileClipPath.self
            ).pointee else { return false }
            let referenceBox: HostClipReferenceBox = switch raw.reference_box {
            case UInt32(WHISKER_BACKGROUND_BOX_PADDING): .padding
            case UInt32(WHISKER_BACKGROUND_BOX_CONTENT): .content
            default: .border
            }
            switch raw.shape_kind {
            case UInt32(WHISKER_CLIP_SHAPE_INSET):
                guard let inset = raw.payload?.assumingMemoryBound(
                    to: WhiskerMobileClipInset.self
                ).pointee else { return false }
                node.setClipPath(.inset(HostInsetClipPath(
                    referenceBox: referenceBox, edges: tupleArray(inset.edges),
                    radiiHorizontal: tupleArray(inset.radii_horizontal),
                    radiiVertical: tupleArray(inset.radii_vertical)
                )))
            case UInt32(WHISKER_CLIP_SHAPE_CIRCLE):
                guard let circle = raw.payload?.assumingMemoryBound(
                    to: WhiskerMobileClipCircle.self
                ).pointee else { return false }
                node.setClipPath(.circle(HostCircleClipPath(
                    referenceBox: referenceBox, radius: circle.radius,
                    centerX: circle.center_x, centerY: circle.center_y
                )))
            case UInt32(WHISKER_CLIP_SHAPE_ELLIPSE):
                guard let ellipse = raw.payload?.assumingMemoryBound(
                    to: WhiskerMobileClipEllipse.self
                ).pointee else { return false }
                node.setClipPath(.ellipse(HostEllipseClipPath(
                    referenceBox: referenceBox, radiusX: ellipse.radius_x,
                    radiusY: ellipse.radius_y, centerX: ellipse.center_x,
                    centerY: ellipse.center_y
                )))
            case UInt32(WHISKER_CLIP_SHAPE_PATH):
                guard let path = raw.payload?.assumingMemoryBound(
                    to: WhiskerMobileClipPathCommands.self
                ).pointee, let commands = path.commands else { return false }
                let copied = UnsafeBufferPointer(start: commands, count: path.command_count).map {
                    HostPathCommand(kind: $0.kind, points: tupleArray($0.points))
                }
                node.setClipPath(.path(HostPathClipPath(
                    referenceBox: referenceBox,
                    evenOdd: path.fill_rule == UInt32(WHISKER_FILL_RULE_EVEN_ODD),
                    commands: copied
                )))
            default: return false
            }
        case UInt32(WHISKER_OP_BACKDROP_BLUR):
            guard let node = nodes[id] else { return false }
            node.setBackdropBlur(CGFloat(operation.scalar))
        case UInt32(WHISKER_OP_IMAGE_RENDERING):
            guard let node = nodes[id],
                  let imageRendering = HostImageRendering(rawValue: operation.integer)
            else { return false }
            node.setImageRendering(imageRendering)
        case UInt32(WHISKER_OP_HIT_TEST):
            nodes[id]?.setHitTestBehavior(operation.integer)
        case UInt32(WHISKER_OP_CURSOR):
            nodes[id]?.setCursorKeyword(operation.integer)
        // Capture targeting is owned by SurfaceRuntime. UIKit already keeps a
        // UITouch stream attached to its recognizer, so no Host mirror is needed.
        case UInt32(WHISKER_OP_CAPTURE), UInt32(WHISKER_OP_RELEASE_CAPTURE): break
        case UInt32(WHISKER_OP_OPACITY):
            nodes[id]?.alpha = CGFloat(operation.scalar)
        case UInt32(WHISKER_OP_VISIBILITY):
            nodes[id]?.setWhiskerVisibility(operation.integer != 0)
        case UInt32(WHISKER_OP_Z_ORDER):
            zOrders[id] = operation.integer
        case UInt32(WHISKER_OP_TEXT):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerMobileText.self).pointee
            else { return false }
            applyText(nodes[id], payload)
        case UInt32(WHISKER_OP_TEXT_STYLE):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerMobileText.self).pointee
            else { return false }
            applyText(nodes[id], payload, styleOnly: true)
        case UInt32(WHISKER_OP_ACCESSIBILITY):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerValueRaw.self).pointee,
                  let node = nodes[id]
            else { return false }
            node.setAccessibility(WhiskerValue.from(raw: payload))
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
        case UInt32(WHISKER_OP_COMMAND):
            guard let payload = operation.payload?
                .assumingMemoryBound(to: WhiskerValueRaw.self).pointee
            else { return false }
            nodes[id]?.mountedElement?.invokeCommand(
                Int(operation.member),
                parameters: WhiskerValue.from(raw: payload)
            )
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
            removeScrollOffset($0)
            if let removed = nodes.removeValue(forKey: $0) {
                releasePresentation(removed)
            }
            parents.removeValue(forKey: $0)
        }
        let removed = Set(descendants).union([id])
        nodeOrder.removeAll { removed.contains($0) }
        removed.forEach { zOrders.removeValue(forKey: $0) }
        parents.removeValue(forKey: id)
        removeScrollOffset(id)
        releasePresentation(node)
        node.removeFromSuperview()
    }

    private func releasePresentation(_ node: WhiskerNodeView) {
        guard let mounted = node.mountedElement else { return }
        mounted.view.removeFromSuperview()
        node.mountedElement = nil
        mounted.dispose()
        guard mounted.registration.name == "whisker.ui/View"
                || mounted.registration.name == "whisker.ui/Text" else { return }
        var pool = presentationPool[mounted.registration.elementType] ?? []
        if pool.count < 128 {
            pool.append(mounted)
            presentationPool[mounted.registration.elementType] = pool
        }
    }

    private func recordZOrderImpact(
        _ operation: WhiskerMobileOperation,
        into parentKeys: inout Set<UInt64>
    ) {
        let rootKey: UInt64 = 0
        switch operation.tag {
        case UInt32(WHISKER_OP_CREATE):
            parentKeys.insert(rootKey)
        case UInt32(WHISKER_OP_DELETE), UInt32(WHISKER_OP_Z_ORDER):
            parentKeys.insert(parents[operation.node] ?? rootKey)
        case UInt32(WHISKER_OP_INSERT):
            parentKeys.insert(rootKey)
            parentKeys.insert(operation.parent)
        case UInt32(WHISKER_OP_REMOVE):
            parentKeys.insert(operation.parent)
            parentKeys.insert(rootKey)
        case UInt32(WHISKER_OP_MOVE):
            parentKeys.insert(operation.parent)
        default:
            break
        }
    }

    private func normalizeZOrder(
        parent parentID: UInt64?,
        siblings: [UInt64],
        idsByView: [ObjectIdentifier: UInt64]
    ) {
        let host: UIView?
        if let parentID, let parent = nodes[parentID] {
            host = parent.sceneChildrenHost()
        } else {
            host = root
        }
        let siblingPairs: [(UInt64, Int)] = (host?.subviews ?? []).enumerated()
            .compactMap { index, view in
                guard let id = idsByView[ObjectIdentifier(view)] else { return nil }
                return (id, index)
            }
        let siblingOrder = Dictionary(uniqueKeysWithValues: siblingPairs)
        let creationOrder = Dictionary(uniqueKeysWithValues: siblings.enumerated().map { ($1, $0) })
        let ordered = siblings.sorted { left, right in
            let leftZ = zOrders[left] ?? 0
            let rightZ = zOrders[right] ?? 0
            if leftZ != rightZ { return leftZ < rightZ }
            return siblingOrder[left, default: creationOrder[left, default: 0]] <
                siblingOrder[right, default: creationOrder[right, default: 0]]
        }
        for id in ordered {
            guard let node = nodes[id] else { continue }
            // `CALayer.render(in:)`, used by deterministic Host capture as
            // well as some UIKit snapshot paths, preserves sublayer order but
            // does not reliably sort extreme signed zPosition values. Project
            // protocol z-order into stable UIView sibling order instead.
            node.layer.zPosition = 0
            if node.superview === host { host?.bringSubviewToFront(node) }
        }
    }

    private func refreshZOrderProjection(parentKeys requested: Set<UInt64>?) {
        let rootKey: UInt64 = 0
        var parentKeys: Set<UInt64>
        if let requested {
            parentKeys = requested
        } else {
            parentKeys = Set(parents.values)
            parentKeys.insert(rootKey)
        }
        guard !parentKeys.isEmpty else { return }

        var siblingsByParent: [UInt64: [UInt64]] = [:]
        var idsByView: [ObjectIdentifier: UInt64] = [:]
        for id in nodeOrder where nodes[id] != nil {
            let parentKey = parents[id] ?? rootKey
            guard parentKeys.contains(parentKey), let node = nodes[id] else { continue }
            siblingsByParent[parentKey, default: []].append(id)
            idsByView[ObjectIdentifier(node)] = id
        }
        for parentKey in parentKeys.sorted() {
            normalizeZOrder(
                parent: parentKey == rootKey ? nil : parentKey,
                siblings: siblingsByParent[parentKey] ?? [],
                idsByView: idsByView
            )
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

    private func applyText(
        _ node: WhiskerNodeView?,
        _ content: WhiskerMobileText,
        styleOnly: Bool = false
    ) {
        guard let node, let mounted = node.mountedElement else { return }
        let decoded = WhiskerTextContent(
            value: hostString(content.text),
            fontFamilies: hostFontFamilies(content),
            fontSize: CGFloat(content.font_size),
            fontWeight: Int(content.font_weight),
            fontStyle: {
                switch content.font_style {
                case 0: return .normal
                case 1: return .italic
                case 2: return .oblique
                default: preconditionFailure("invalid font style")
                }
            }(),
            lineHeight: content.line_height > 0 ? CGFloat(content.line_height) : nil,
            letterSpacing: CGFloat(content.letter_spacing),
            fontFeatures: hostFontFeatures(content),
            fontVariations: hostFontVariations(content),
            fontOpticalSizing: content.font_optical_sizing == 0 ? .auto : .none,
            color: parsePaintColor(content.color),
            direction: {
                switch content.direction {
                case 0: return .auto
                case 1: return .leftToRight
                case 2: return .rightToLeft
                default: preconditionFailure("invalid text direction")
                }
            }(),
            alignment: {
                switch content.alignment {
                case 0: return .start
                case 1: return .end
                case 2: return .left
                case 3: return .right
                case 4: return .center
                default: preconditionFailure("invalid text alignment")
                }
            }(),
            indent: WhiskerTextIndent(
                logicalPixels: CGFloat(content.indent_logical_pixels),
                percentage: CGFloat(content.indent_percentage)
            ),
            wrap: content.wrap != 0,
            wordBreak: {
                switch content.word_break {
                case 0: return .normal
                case 1: return .breakAll
                case 2: return .keepAll
                default: preconditionFailure("invalid word-break")
                }
            }(),
            maxLines: Int(content.max_lines),
            overflow: content.overflow == 0 ? .clip : .ellipsis,
            decoration: content.decoration_flags == 0 ? nil : WhiskerTextDecoration(
                line: content.decoration_flags & 1 != 0 ? .underline : .lineThrough,
                style: {
                    switch content.decoration_style {
                    case 0: return .solid
                    case 1: return .double
                    case 2: return .dotted
                    case 3: return .dashed
                    case 4: return .wavy
                    default: preconditionFailure("invalid text decoration style")
                    }
                }(),
                color: parsePaintColor(content.decoration_color)
            ),
            shadow: content.shadow_flags == 0 ? nil : WhiskerTextShadow(
                offset: CGSize(
                    width: CGFloat(content.shadow_offset_x),
                    height: CGFloat(content.shadow_offset_y)
                ),
                blurRadius: CGFloat(content.shadow_blur_radius),
                color: parsePaintColor(content.shadow_color)
            )
        )
        let accepted = styleOnly
            ? mounted.setTextStyle(WhiskerTextStyle(content: decoded))
            : mounted.setText(decoded)
        guard accepted else {
            preconditionFailure(
                "text operation sent to element \(mounted.registration.name) without the declared text implementation"
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
