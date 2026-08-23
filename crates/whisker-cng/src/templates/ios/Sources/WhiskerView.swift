import UIKit
#if canImport(WhiskerModules)
import WhiskerModules
#else
import WhiskerModule
#endif

private typealias WhiskerRequestFrame = @convention(c) (UnsafeMutableRawPointer?) -> Void
private typealias WhiskerBootstrapHost = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileBootstrap>?
) -> Bool
private typealias WhiskerMeasureHost = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileMeasureRequest>?, Int,
    UnsafeMutablePointer<WhiskerMobileMeasureResponse>?
) -> Bool
private typealias WhiskerPresentFrame = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileFrame>?,
    UnsafeMutablePointer<WhiskerMobileApplyResponse>?
) -> Bool
private typealias WhiskerModuleResult = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerValueRaw>?
) -> Void
private typealias WhiskerInvokeModule = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?, Int,
    UnsafePointer<UInt8>?, Int,
    UnsafePointer<WhiskerValueRaw>?, Int,
    Bool,
    WhiskerModuleResult,
    UnsafeMutableRawPointer?
) -> Bool
private typealias WhiskerObserveModule = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?, Int,
    UnsafePointer<UInt8>?, Int,
    Bool
) -> Void

private enum WhiskerFrameApplicationError: Error {
    case missingElementFactory(String)
}

private struct HostPaint {
    let background: UIColor
    let widths: [WhiskerMobileLengthPercentage]
    let colors: [UIColor]
    let styles: [UInt32]
    let radii: [WhiskerMobileLengthPercentage]
}
private struct DecodedMember {
    let id: Int
    let name: String
    let kind: WhiskerValueKind
    let optional: Bool
}

@_silgen_name("whisker_view_create")
private func whiskerViewCreate(
    _ width: Float, _ height: Float, _ scale: Float,
    _ requestFrame: WhiskerRequestFrame, _ requestData: UnsafeMutableRawPointer?,
    _ bootstrap: WhiskerBootstrapHost, _ bootstrapData: UnsafeMutableRawPointer?,
    _ measure: WhiskerMeasureHost, _ measureData: UnsafeMutableRawPointer?,
    _ presentFrame: WhiskerPresentFrame, _ presentData: UnsafeMutableRawPointer?,
    _ invokeModule: WhiskerInvokeModule, _ observeModule: WhiskerObserveModule,
    _ moduleData: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer?

@_silgen_name("whisker_view_tick")
private func whiskerViewTick(
    _ handle: UnsafeMutableRawPointer?, _ timestampMs: Double,
    _ width: Float, _ height: Float, _ scale: Float
) -> Bool

@_silgen_name("whisker_view_destroy")
private func whiskerViewDestroy(_ handle: UnsafeMutableRawPointer?)

@_silgen_name("whisker_view_dispatch_event")
private func whiskerViewDispatchEvent(
    _ handle: UnsafeMutableRawPointer?, _ timestampMs: Double, _ node: UInt64,
    _ name: UnsafePointer<UInt8>?, _ nameLength: Int,
    _ detail: UnsafePointer<WhiskerValueRaw>?
) -> Bool

@_silgen_name("whisker_view_dispatch_module_event")
private func whiskerViewDispatchModuleEvent(
    _ handle: UnsafeMutableRawPointer?,
    _ module: UnsafePointer<UInt8>?, _ moduleLength: Int,
    _ event: UnsafePointer<UInt8>?, _ eventLength: Int,
    _ payload: UnsafePointer<WhiskerValueRaw>?
) -> Bool

private final class WhiskerNodeView: UIView {
    let element: String
    var contentFrame = CGRect.zero
    var fillColor = UIColor.clear
    var borderColor = UIColor.clear
    var borderWidth: CGFloat = 0
    var cornerRadii = [CGSize](repeating: .zero, count: 4)
    var paint: HostPaint?
    var mountedElement: WhiskerMountedElement?

    init(element: String) {
        self.element = element
        super.init(frame: .zero)
        isOpaque = false
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        if let mountedElement {
            mountedElement.view.frame = contentFrame
        }
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        let path = roundedPath(in: bounds.insetBy(dx: borderWidth / 2, dy: borderWidth / 2), radii: cornerRadii)
        fillColor.setFill()
        path.fill()
        if borderWidth > 0 {
            borderColor.setStroke()
            path.lineWidth = borderWidth
            path.stroke()
        }
    }
}

/** The single iOS View that owns a Whisker runtime and its native scene. */
public final class WhiskerView: UIView {
    private var nodes: [UInt64: WhiskerNodeView] = [:]
    private var parents: [UInt64: UInt64] = [:]
    private var displayLink: CADisplayLink?
    private var hostToken: UnsafeMutableRawPointer?
    private var runtimeHandle: UnsafeMutableRawPointer?
    private var isApplicationActive = true
    private var sceneEpoch: UInt32 = 0
    private var revision: UInt64 = 0
    private var applyingFrame = false
    private var deferredEvents: [() -> Void] = []

    private var logicalBounds: CGRect { bounds.inset(by: safeAreaInsets) }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        autoresizingMask = [.flexibleWidth, .flexibleHeight]
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(applicationDidBecomeActive),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(applicationWillResignActive),
            name: UIApplication.willResignActiveNotification,
            object: nil
        )
    }

    public required init?(coder: NSCoder) { nil }

    deinit {
        unmount()
        NotificationCenter.default.removeObserver(self)
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            WhiskerInsetsDispatcher.attach(self)
            mountWhenSized()
        } else {
            unmount()
            WhiskerInsetsDispatcher.detach(self)
        }
    }

    public override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        WhiskerInsetsDispatcher.update(self)
        if runtimeHandle != nil { requestFrame() }
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        mountWhenSized()
        if runtimeHandle != nil { requestFrame() }
    }

    private func mountWhenSized() {
        let viewport = logicalBounds
        guard runtimeHandle == nil, window != nil, viewport.width > 0, viewport.height > 0 else { return }
        let token = Unmanaged.passRetained(self).toOpaque()
        hostToken = token
        WhiskerModuleEventCenter.installEventSink { [weak self] module, event, payload in
            self?.dispatchModuleEvent(module: module, event: event, payload: payload)
        }
        runtimeHandle = whiskerViewCreate(
            Float(viewport.width), Float(viewport.height), Float(window?.screen.scale ?? 1),
            whiskerIOSRequestFrame, token,
            whiskerIOSBootstrap, token,
            whiskerIOSMeasure, token,
            whiskerIOSPresentFrame, token,
            whiskerIOSInvokeModule, whiskerIOSObserveModule, token
        )
        if runtimeHandle != nil {
            driveRuntimeFrame(timestampMs: ProcessInfo.processInfo.systemUptime * 1_000)
            requestFrame()
        } else {
            Unmanaged<WhiskerView>.fromOpaque(token).release()
            hostToken = nil
        }
    }

    private func unmount() {
        guard let handle = runtimeHandle else { return }
        runtimeHandle = nil
        displayLink?.invalidate()
        displayLink = nil
        whiskerViewDestroy(handle)
        WhiskerModuleEventCenter.installEventSink(nil)
        clearScene()
        if let token = hostToken {
            Unmanaged<WhiskerView>.fromOpaque(token).release()
            hostToken = nil
        }
    }

    fileprivate func requestFrame() {
        guard runtimeHandle != nil, isApplicationActive, window != nil else { return }
        if displayLink == nil {
            displayLink = CADisplayLink(target: self, selector: #selector(driveFrame(_:)))
            displayLink?.add(to: .main, forMode: .common)
        }
        displayLink?.isPaused = false
    }

    @objc private func driveFrame(_ link: CADisplayLink) {
        guard runtimeHandle != nil, isApplicationActive else { link.isPaused = true; return }
        let idle = driveRuntimeFrame(timestampMs: link.timestamp * 1_000)
        link.isPaused = idle
    }

    @discardableResult
    private func driveRuntimeFrame(timestampMs: Double) -> Bool {
        guard let handle = runtimeHandle else { return true }
        return whiskerViewTick(
            handle,
            timestampMs,
            Float(logicalBounds.width), Float(logicalBounds.height), Float(window?.screen.scale ?? 1)
        )
    }

    @objc private func applicationDidBecomeActive() {
        isApplicationActive = true
        mountWhenSized()
        requestFrame()
    }

    @objc private func applicationWillResignActive() {
        isApplicationActive = false
        displayLink?.isPaused = true
    }

    fileprivate func bootstrap(_ raw: WhiskerMobileBootstrap) -> Bool {
        guard raw.abi_major == UInt16(WHISKER_MOBILE_ABI_MAJOR),
              raw.protocol_major == 1,
              let base = raw.registrations else { return false }
        let registrations = (0..<raw.registration_count).map { index -> WhiskerElementRegistration in
            let value = base.advanced(by: index).pointee
            return WhiskerElementRegistration(
                elementType: Int(value.element_type), name: hostString(value.name),
                childPolicy: [WhiskerChildPolicy.none, .elements, .plainText][Int(value.child_policy)],
                measurement: [WhiskerMeasurement.none, .text, .replacedContent, .custom][Int(value.measurement)],
                properties: decodeMembers(value.properties, value.property_count).map {
                    WhiskerPropertyBinding(id: $0.id, name: $0.name, value: $0.kind)
                },
                events: decodeMembers(value.events, value.event_count).map {
                    WhiskerEventBinding(id: $0.id, name: $0.name, detail: $0.optional ? $0.kind : nil)
                },
                commands: decodeMembers(value.commands, value.command_count).map {
                    WhiskerCommandBinding(id: $0.id, name: $0.name, arguments: $0.kind)
                }
            )
        }
        return WhiskerElementRegistry.bind(registrations)
    }

    fileprivate func applyFrame(
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
        if frame.mode == UInt8(WHISKER_FRAME_SNAPSHOT) { clearScene() }
        for operation in values where !apply(operation) {
            response.status = UInt8(WHISKER_APPLY_REJECTED)
            response.revision = revision
            return true
        }
        attachRoots()
        sceneEpoch = frame.scene_epoch
        revision = frame.target_revision
        response.status = UInt8(WHISKER_APPLY_ACCEPTED)
        response.revision = revision
        return true
    }

#if WHISKER_HOST_CONFORMANCE
    func applyConformanceFrame(
        _ frame: WhiskerMobileFrame,
        response: inout WhiskerMobileApplyResponse
    ) -> Bool {
        applyFrame(frame, response: &response)
    }
#endif

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
                      WhiskerElementRegistry.registration(Int(operation.member)) != nil else { return false }
                existing.insert(operation.node)
                elementTypes[operation.node] = Int(operation.member)
            case UInt32(WHISKER_OP_DELETE):
                guard existing.remove(operation.node) != nil else { return false }
                elementTypes.removeValue(forKey: operation.node)
                stagedParents = stagedParents.filter { $0.key != operation.node && $0.value != operation.node }
            case UInt32(WHISKER_OP_INSERT):
                guard existing.contains(operation.parent), existing.contains(operation.child),
                      stagedParents[operation.child] == nil,
                      elementTypes[operation.parent].flatMap({ WhiskerElementRegistry.registration($0) })?.childPolicy.acceptsElements == true
                else { return false }
                stagedParents[operation.child] = operation.parent
            case UInt32(WHISKER_OP_REMOVE):
                guard stagedParents[operation.child] == operation.parent else { return false }
                stagedParents.removeValue(forKey: operation.child)
            case UInt32(WHISKER_OP_MOVE):
                guard stagedParents[operation.child] == operation.parent else { return false }
            case UInt32(WHISKER_OP_LAYOUT), UInt32(WHISKER_OP_PAINT), UInt32(WHISKER_OP_TEXT),
                 UInt32(WHISKER_OP_PROPERTY):
                guard existing.contains(operation.node), operation.payload != nil else { return false }
            case UInt32(WHISKER_OP_CLIP), UInt32(WHISKER_OP_OPACITY),
                 UInt32(WHISKER_OP_VISIBILITY), UInt32(WHISKER_OP_Z_ORDER),
                 UInt32(WHISKER_OP_CLEAR_PROPERTY), UInt32(WHISKER_OP_EVENT_MASK):
                guard existing.contains(operation.node) else { return false }
            default:
                // Transform, hit-test, capture and commands are rejected until
                // their native behavior is implemented; never acknowledge a no-op.
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
                  let mounted = WhiskerElementRegistry.mount(Int(operation.member), eventSink: { [weak self] event, detail in
                      self?.dispatchElementEvent(node: id, name: event.name, detail: detail)
                  }) else { return false }
            let node = WhiskerNodeView(element: registration.name)
            node.mountedElement = mounted
            mounted.view.frame = node.bounds
            mounted.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            node.addSubview(mounted.view)
            nodes[id] = node
        case UInt32(WHISKER_OP_DELETE): deleteNode(id)
        case UInt32(WHISKER_OP_INSERT), UInt32(WHISKER_OP_MOVE):
            insertChild(parent: operation.parent, child: operation.child, index: Int(operation.index))
        case UInt32(WHISKER_OP_REMOVE): detachChild(parent: operation.parent, child: operation.child)
        case UInt32(WHISKER_OP_LAYOUT):
            guard let payload = operation.payload?.assumingMemoryBound(to: WhiskerMobileLayoutGeometry.self).pointee else { return false }
            applyLayout(id, nodes[id], payload)
        case UInt32(WHISKER_OP_PAINT):
            guard let payload = operation.payload?.assumingMemoryBound(to: WhiskerMobileBoxPaint.self).pointee else { return false }
            applyPaint(nodes[id], payload)
        case UInt32(WHISKER_OP_CLIP): nodes[id]?.clipsToBounds = operation.flags & 3 != 0
        case UInt32(WHISKER_OP_OPACITY): nodes[id]?.alpha = CGFloat(operation.scalar)
        case UInt32(WHISKER_OP_VISIBILITY): nodes[id]?.isHidden = operation.integer == 0
        case UInt32(WHISKER_OP_Z_ORDER): nodes[id]?.layer.zPosition = CGFloat(operation.integer)
        case UInt32(WHISKER_OP_TEXT):
            guard let payload = operation.payload?.assumingMemoryBound(to: WhiskerMobileText.self).pointee else { return false }
            applyText(nodes[id], payload)
        case UInt32(WHISKER_OP_PROPERTY):
            guard let payload = operation.payload?.assumingMemoryBound(to: WhiskerValueRaw.self).pointee else { return false }
            nodes[id]?.mountedElement?.setProperty(Int(operation.member), value: WhiskerValue.from(raw: payload))
        case UInt32(WHISKER_OP_CLEAR_PROPERTY): nodes[id]?.mountedElement?.clearProperty(Int(operation.member))
        case UInt32(WHISKER_OP_EVENT_MASK): nodes[id]?.mountedElement?.setEventMask(operation.wide)
        default: return false
        }
        return true
    }

    private func dispatchElementEvent(node: UInt64, name: String, detail: WhiskerValue) {
        if applyingFrame {
            deferredEvents.append { [weak self] in self?.dispatchElementEvent(node: node, name: name, detail: detail) }
            return
        }
        guard let handle = runtimeHandle else { return }
        var raw = detail.toRaw()
        defer { WhiskerValue.releaseRaw(&raw) }
        let nameBytes = Array(name.utf8)
        nameBytes.withUnsafeBytes { nameBuffer in
            _ = whiskerViewDispatchEvent(
                handle, ProcessInfo.processInfo.systemUptime * 1_000, node,
                nameBuffer.bindMemory(to: UInt8.self).baseAddress, nameBytes.count, &raw
            )
        }
    }

    fileprivate func invokeModule(
        module name: String,
        method: String,
        rawArgs: UnsafePointer<WhiskerValueRaw>?,
        argumentCount: Int,
        isAsync: Bool,
        result: @escaping WhiskerModuleResult,
        resultData: UnsafeMutableRawPointer?
    ) -> Bool {
        guard let module = WhiskerModuleRegistry.module(named: name) else {
            deliverModuleResult(.error("module not registered: \(name)"), result, resultData)
            return true
        }
        let args = WhiskerValue.decodeArray(rawArgs, count: argumentCount)
        let settle: (WhiskerValue) -> Void = { value in
            self.deliverModuleResult(value, result, resultData)
        }
        if isAsync {
            let promise = WhiskerPromise(onSettle: settle)
            if module.dispatchModuleFunctionAsync(method, args, promise) { return true }
        }
        settle(module.dispatchModuleFunction(method, args))
        return true
    }

    fileprivate func observeModule(module: String, event: String, observing: Bool) {
        if observing {
            WhiskerModuleEventCenter.fireStart(module: module, event: event)
        } else {
            WhiskerModuleEventCenter.fireStop(module: module, event: event)
        }
    }

    private func deliverModuleResult(
        _ value: WhiskerValue,
        _ result: WhiskerModuleResult,
        _ resultData: UnsafeMutableRawPointer?
    ) {
        var raw = value.toRaw()
        result(resultData, &raw)
        WhiskerValue.releaseRaw(&raw)
    }

    private func dispatchModuleEvent(module: String, event: String, payload: WhiskerValue) {
        guard let handle = runtimeHandle else {
            // OnStartObserving may emit synchronously from inside
            // whiskerViewCreate, before its returned handle is assigned.
            DispatchQueue.main.async { [weak self] in
                guard self?.runtimeHandle != nil else { return }
                self?.dispatchModuleEvent(module: module, event: event, payload: payload)
            }
            return
        }
        if applyingFrame {
            deferredEvents.append { [weak self] in self?.dispatchModuleEvent(module: module, event: event, payload: payload) }
            return
        }
        var raw = payload.toRaw()
        defer { WhiskerValue.releaseRaw(&raw) }
        let moduleBytes = Array(module.utf8)
        let eventBytes = Array(event.utf8)
        moduleBytes.withUnsafeBytes { moduleBuffer in
            eventBytes.withUnsafeBytes { eventBuffer in
                _ = whiskerViewDispatchModuleEvent(
                    handle,
                    moduleBuffer.bindMemory(to: UInt8.self).baseAddress, moduleBytes.count,
                    eventBuffer.bindMemory(to: UInt8.self).baseAddress, eventBytes.count, &raw
                )
            }
        }
    }

    private func clearScene() {
        nodes.values.forEach { $0.mountedElement?.dispose() }
        nodes.values.forEach { $0.removeFromSuperview() }
        nodes.removeAll()
        parents.removeAll()
    }

    private func attachRoots() {
        for (id, node) in nodes where parents[id] == nil && node.superview !== self {
            node.removeFromSuperview()
            addSubview(node)
        }
        for (id, node) in nodes where parents[id] == nil {
            node.frame.origin = logicalBounds.origin
        }
    }

    private func insertChild(parent parentID: UInt64, child childID: UInt64, index: Int) {
        guard let parent = nodes[parentID], let child = nodes[childID] else { return }
        guard let mounted = parent.mountedElement else {
            preconditionFailure("parent element is not mounted")
        }
        precondition(mounted.registration.childPolicy.acceptsElements, "\(mounted.registration.name) does not accept element children")
        child.removeFromSuperview()
        parents[childID] = parentID
        if let childrenHost = mounted.childrenHost() {
            childrenHost.insertSubview(child, at: min(max(index, 0), childrenHost.subviews.count))
        } else {
            // The native content view occupies slot zero. Logical children are
            // mounted by the common presenter above it.
            parent.insertSubview(child, at: min(max(index + 1, 1), parent.subviews.count))
        }
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
        parents.removeValue(forKey: id)
        node.mountedElement?.dispose()
        node.removeFromSuperview()
    }

    private func isDescendant(_ candidate: UInt64, of ancestor: UInt64) -> Bool {
        var current = parents[candidate]
        while let value = current {
            if value == ancestor { return true }
            current = parents[value]
        }
        return false
    }

    private func applyLayout(_ id: UInt64, _ node: WhiskerNodeView?, _ geometry: WhiskerMobileLayoutGeometry) {
        guard let node else { return }
        var frame = rect(geometry.border)
        if let parentID = parents[id],
           let parent = nodes[parentID],
           parent.mountedElement?.childrenHost() != nil {
            frame.origin.x -= parent.contentFrame.origin.x
            frame.origin.y -= parent.contentFrame.origin.y
        }
        node.frame = frame
        node.contentFrame = rect(geometry.content)
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
            color: parseColor(content.color)
        )) else {
            preconditionFailure(
                "text operation sent to element \(mounted.registration.name) without a text implementation"
            )
        }
    }


    private func applyPaint(_ node: WhiskerNodeView?, _ raw: WhiskerMobileBoxPaint) {
        guard let node else { return }
        let paint = HostPaint(
            background: parseColor(raw.background),
            widths: tupleArray(raw.widths),
            colors: tupleArray(raw.colors).map(parseColor),
            styles: tupleArray(raw.styles),
            radii: tupleArray(raw.radii)
        )
        applyPaint(node, paint)
    }

    private func applyPaint(_ node: WhiskerNodeView, _ paint: HostPaint) {
        node.paint = paint
        node.fillColor = paint.background
        node.borderWidth = resolve(paint.widths[0], axis: node.bounds.height)
        node.borderColor = paint.colors[0]
        node.cornerRadii = paint.radii.map { value in CGSize(
            width: resolve(value, axis: node.bounds.width),
            height: resolve(value, axis: node.bounds.height)
        ) }
        node.setNeedsLayout()
    }
}

private let whiskerIOSRequestFrame: WhiskerRequestFrame = { data in
    guard let data else { return }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    DispatchQueue.main.async { view.requestFrame() }
}

private let whiskerIOSBootstrap: WhiskerBootstrapHost = { data, bootstrap in
    guard let data, let bootstrap else { return false }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    return view.bootstrap(bootstrap.pointee)
}

private let whiskerIOSMeasure: WhiskerMeasureHost = { data, requests, count, responses in
    guard data != nil, let requests, let responses else { return false }
    for index in 0..<count {
        let request = requests.advanced(by: index).pointee
        var response = responses.advanced(by: index).pointee
        response.key = request.key
        response.environment_epoch = request.environment_epoch
        switch request.kind {
        case UInt32(WHISKER_MEASURE_TEXT):
            let family = hostString(request.font_family)
            let weightValue = max(-1, min(1, CGFloat(Int(request.font_weight) - 400) / 500))
            var baseFont = family.isEmpty
                ? UIFont.systemFont(ofSize: CGFloat(request.font_size), weight: UIFont.Weight(rawValue: weightValue))
                : UIFont(name: family, size: CGFloat(request.font_size)) ?? UIFont.systemFont(ofSize: CGFloat(request.font_size))
            if request.font_style != 0,
               let descriptor = baseFont.fontDescriptor.withSymbolicTraits(
                   baseFont.fontDescriptor.symbolicTraits.union(.traitItalic)
               ) {
                baseFont = UIFont(descriptor: descriptor, size: CGFloat(request.font_size))
            }
            let paragraph = NSMutableParagraphStyle()
            if request.line_height > 0 {
                paragraph.minimumLineHeight = CGFloat(request.line_height)
                paragraph.maximumLineHeight = CGFloat(request.line_height)
            }
            let attributes: [NSAttributedString.Key: Any] = [
                .font: baseFont,
                .kern: CGFloat(request.letter_spacing),
                .paragraphStyle: paragraph,
            ]
            let width = request.available_width_kind == 0 && request.wrap != 0
                ? CGFloat(request.available_width) : CGFloat.greatestFiniteMagnitude
            var measured = (hostString(request.text) as NSString).boundingRect(
                with: CGSize(width: width, height: .greatestFiniteMagnitude),
                options: [.usesLineFragmentOrigin, .usesFontLeading], attributes: attributes, context: nil
            ).size
            if request.max_lines > 0 {
                let lineHeight = request.line_height > 0 ? CGFloat(request.line_height) : baseFont.lineHeight
                measured.height = min(measured.height, lineHeight * CGFloat(request.max_lines))
            }
            response.status = UInt32(WHISKER_MEASURE_READY)
            response.width = request.known_mask & 1 != 0 ? request.known_width : Float(ceil(measured.width))
            response.height = request.known_mask & 2 != 0 ? request.known_height : Float(ceil(measured.height))
            response.first_baseline = Float(baseFont.ascender)
            response.last_baseline = max(response.first_baseline, response.height - Float(abs(baseFont.descender)))
            response.metrics_mask = 3
        case UInt32(WHISKER_MEASURE_REPLACED_CONTENT) where request.intrinsic_mask == 3,
             UInt32(WHISKER_MEASURE_EMBEDDED_SURFACE) where request.intrinsic_mask == 3:
            response.status = UInt32(WHISKER_MEASURE_READY)
            response.width = request.known_mask & 1 != 0 ? request.known_width : request.intrinsic_width
            response.height = request.known_mask & 2 != 0 ? request.known_height : request.intrinsic_height
        default:
            let payload = request.payload.ptr.map { Data(bytes: $0, count: request.payload.len) } ?? Data()
            let custom = WhiskerElementRegistry.measure(Int(request.element_type), request: WhiskerMeasureRequest(
                availableWidth: request.available_width_kind == 0 ? CGFloat(request.available_width) : nil,
                availableHeight: request.available_height_kind == 0 ? CGFloat(request.available_height) : nil,
                knownWidth: request.known_mask & 1 != 0 ? CGFloat(request.known_width) : nil,
                knownHeight: request.known_mask & 2 != 0 ? CGFloat(request.known_height) : nil,
                payloadVersion: request.payload_version, payload: payload
            ))
            if let custom {
                response.status = UInt32(WHISKER_MEASURE_READY)
                response.width = request.known_mask & 1 != 0 ? request.known_width : Float(custom.width)
                response.height = request.known_mask & 2 != 0 ? request.known_height : Float(custom.height)
            } else {
                response.status = UInt32(WHISKER_MEASURE_UNSUPPORTED)
                response.reason = 1
            }
        }
        responses.advanced(by: index).pointee = response
    }
    return true
}

private let whiskerIOSPresentFrame: WhiskerPresentFrame = { data, frame, response in
    guard let data, let frame, let response else { return false }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    if Thread.isMainThread { return view.applyFrame(frame.pointee, response: &response.pointee) }
    return DispatchQueue.main.sync { view.applyFrame(frame.pointee, response: &response.pointee) }
}

private let whiskerIOSInvokeModule: WhiskerInvokeModule = {
    data,
    moduleBytes, moduleLength,
    methodBytes, methodLength,
    arguments, argumentCount,
    isAsync,
    result, resultData in
    guard
        let data,
        let moduleBytes,
        let methodBytes,
        let module = String(bytes: UnsafeBufferPointer(start: moduleBytes, count: moduleLength), encoding: .utf8),
        let method = String(bytes: UnsafeBufferPointer(start: methodBytes, count: methodLength), encoding: .utf8)
    else { return false }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    return view.invokeModule(
        module: module,
        method: method,
        rawArgs: arguments,
        argumentCount: argumentCount,
        isAsync: isAsync,
        result: result,
        resultData: resultData
    )
}

private let whiskerIOSObserveModule: WhiskerObserveModule = {
    data, moduleBytes, moduleLength, eventBytes, eventLength, observing in
    guard
        let data,
        let moduleBytes,
        let eventBytes,
        let module = String(bytes: UnsafeBufferPointer(start: moduleBytes, count: moduleLength), encoding: .utf8),
        let event = String(bytes: UnsafeBufferPointer(start: eventBytes, count: eventLength), encoding: .utf8)
    else { return }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    view.observeModule(module: module, event: event, observing: observing)
}

private func hostString(_ value: WhiskerStringRef) -> String {
    guard let pointer = value.ptr, value.len > 0 else { return "" }
    return String(decoding: UnsafeBufferPointer(
        start: UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self), count: value.len
    ), as: UTF8.self)
}

private func decodeMembers(
    _ pointer: UnsafePointer<WhiskerMobileMemberRegistration>?, _ count: Int
) -> [DecodedMember] {
    guard let pointer else { return [] }
    return (0..<count).map { index in
        let value = pointer.advanced(by: index).pointee
        let kinds: [WhiskerValueKind] = [.null, .bool, .int, .float, .string, .bytes, .array, .map]
        return DecodedMember(
            id: Int(value.id), name: hostString(value.name),
            kind: kinds[Int(value.value_kind)], optional: value.optional_kind != 0
        )
    }
}

private func tupleArray<T>(_ value: (T, T, T, T)) -> [T] {
    [value.0, value.1, value.2, value.3]
}

private func rect(_ value: WhiskerMobileRect) -> CGRect {
    CGRect(x: CGFloat(value.x), y: CGFloat(value.y), width: CGFloat(value.width), height: CGFloat(value.height))
}
private func resolve(_ value: WhiskerMobileLengthPercentage, axis: CGFloat) -> CGFloat {
    CGFloat(value.length) + CGFloat(value.fraction) * axis
}

private func parseColor(_ value: WhiskerMobileColor) -> UIColor {
    if value.kind == 1 {
        return UIColor(
            red: CGFloat(value.red) / 255, green: CGFloat(value.green) / 255,
            blue: CGFloat(value.blue) / 255, alpha: CGFloat(value.alpha)
        )
    } else {
        switch hostString(value.name).lowercased() {
        case "black": return .black
        case "white": return .white
        case "red": return .red
        case "green": return .green
        case "blue": return .blue
        default: return .clear
        }
    }
}

private func roundedPath(in rect: CGRect, radii: [CGSize]) -> UIBezierPath {
    var r = Array((radii + [.zero, .zero, .zero, .zero]).prefix(4)).map {
        CGSize(width: max($0.width, 0), height: max($0.height, 0))
    }
    let horizontalTop = r[0].width + r[1].width
    let horizontalBottom = r[3].width + r[2].width
    let verticalLeft = r[0].height + r[3].height
    let verticalRight = r[1].height + r[2].height
    let scale = [
        CGFloat(1),
        horizontalTop > 0 ? rect.width / horizontalTop : 1,
        horizontalBottom > 0 ? rect.width / horizontalBottom : 1,
        verticalLeft > 0 ? rect.height / verticalLeft : 1,
        verticalRight > 0 ? rect.height / verticalRight : 1
    ].min() ?? 1
    if scale < 1 {
        r = r.map { CGSize(width: $0.width * scale, height: $0.height * scale) }
    }

    // Cubic approximation of each quarter ellipse. Keeping independent x/y
    // radii is what makes `border-radius: 50%` on a non-square box an ellipse
    // instead of a capsule made from circular corners.
    let k: CGFloat = 0.552_284_749_830_793_6
    let path = UIBezierPath()
    path.move(to: CGPoint(x: rect.minX + r[0].width, y: rect.minY))
    path.addLine(to: CGPoint(x: rect.maxX - r[1].width, y: rect.minY))
    path.addCurve(
        to: CGPoint(x: rect.maxX, y: rect.minY + r[1].height),
        controlPoint1: CGPoint(x: rect.maxX - r[1].width + k * r[1].width, y: rect.minY),
        controlPoint2: CGPoint(x: rect.maxX, y: rect.minY + r[1].height - k * r[1].height)
    )
    path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - r[2].height))
    path.addCurve(
        to: CGPoint(x: rect.maxX - r[2].width, y: rect.maxY),
        controlPoint1: CGPoint(x: rect.maxX, y: rect.maxY - r[2].height + k * r[2].height),
        controlPoint2: CGPoint(x: rect.maxX - r[2].width + k * r[2].width, y: rect.maxY)
    )
    path.addLine(to: CGPoint(x: rect.minX + r[3].width, y: rect.maxY))
    path.addCurve(
        to: CGPoint(x: rect.minX, y: rect.maxY - r[3].height),
        controlPoint1: CGPoint(x: rect.minX + r[3].width - k * r[3].width, y: rect.maxY),
        controlPoint2: CGPoint(x: rect.minX, y: rect.maxY - r[3].height + k * r[3].height)
    )
    path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + r[0].height))
    path.addCurve(
        to: CGPoint(x: rect.minX + r[0].width, y: rect.minY),
        controlPoint1: CGPoint(x: rect.minX, y: rect.minY + r[0].height - k * r[0].height),
        controlPoint2: CGPoint(x: rect.minX + r[0].width - k * r[0].width, y: rect.minY)
    )
    path.close()
    return path
}
