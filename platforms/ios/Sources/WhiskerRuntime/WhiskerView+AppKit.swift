#if !canImport(UIKit) && canImport(AppKit)
import AppKit
import WhiskerModule

/** The single macOS View that owns a Whisker runtime and its native scene. */
public final class WhiskerView: NSView {
    private var displayLink: CADisplayLink?
    private var hostToken: UnsafeMutableRawPointer?
    private var runtimeHandle: UnsafeMutableRawPointer?
    private var runtimePaused = false
    private var isWindowVisible = true
    private var moduleEventSinkEpoch: UInt64 = 0
    private let pendingModuleEvents = PendingModuleEvents()
    private var mouseMonitor: Any?
    private var mouseTrackingArea: NSTrackingArea?
    private var mouseCaptured = false
    private var dirtyScrollOffsets: [UInt64: CGPoint] = [:]
    private let modules = HostModuleDispatcher()
    let preparedParagraphs = PreparedParagraphs()
    let textMeasurementCache = TextMeasurementCache()
    private let resources = HostResourceStore()
    private lazy var resourceService = HostResourceService(store: resources)
    private var rasterResourceObserver: ((WhiskerRasterResourceEvent) -> Void)?
    private lazy var scene = HostScene(
        root: self,
        resources: resources,
        preparedParagraphs: preparedParagraphs,
        logicalBounds: { [unowned self] in self.logicalBounds },
        emitElementEvent: { [weak self] node, name, detail in
            self?.dispatchElementEvent(node: node, name: name, detail: detail)
        },
        updateScrollOffset: { [weak self] node, offset in
            self?.dirtyScrollOffsets[node] = offset
        },
        removeScrollOffset: { [weak self] node in
            self?.dirtyScrollOffsets.removeValue(forKey: node)
        }
    )

    private static let mousePointerID: UInt64 = 1

    private var logicalBounds: CGRect {
        edgeToEdgeViewportBounds(bounds, safeAreaInsets: safeAreaInsets)
    }

    private var backingScale: CGFloat {
        window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    }

    public override var isFlipped: Bool { true }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        wantsLayer = true
        autoresizingMask = [.width, .height]
        resourceService.eventHandler = { [weak self] event in
            DispatchQueue.main.async { self?.dispatchRasterResourceEvent(event) }
        }
    }

    public required init?(coder: NSCoder) { nil }

    /// Registers an already-decoded raster for use by resource-backed paint images.
    @discardableResult
    public func registerRasterResource(id: UInt64, image: CGImage) -> Bool {
        resources.registerRasterImage(image, id: id)
    }

    /// Starts native acquisition and decode for one monotonically newer generation.
    @discardableResult
    public func loadRasterResource(
        id: UInt64,
        generation: UInt64,
        source: WhiskerRasterResourceSource
    ) -> Bool {
        resourceService.load(id: id, generation: generation, source: source)
    }

    /// Releases exactly one generation without evicting a newer replacement.
    @discardableResult
    public func releaseRasterResource(id: UInt64, generation: UInt64) -> Bool {
        resourceService.release(id: id, generation: generation)
    }

    /// Returns the retained lifecycle state for one exact generation.
    public func rasterResourceState(
        id: UInt64,
        generation: UInt64
    ) -> WhiskerRasterResourceState? {
        resourceService.state(id: id, generation: generation)
    }

    /// Installs the Host-to-runtime notification boundary for lifecycle events.
    public func observeRasterResourceEvents(
        _ handler: ((WhiskerRasterResourceEvent) -> Void)?
    ) {
        rasterResourceObserver = handler
    }

    deinit {
        MainActor.assumeIsolated {
            removeMouseMonitor()
            unmount()
        }
        NotificationCenter.default.removeObserver(self)
    }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        NotificationCenter.default.removeObserver(
            self,
            name: NSWindow.didChangeOcclusionStateNotification,
            object: nil
        )
        if let window {
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(windowOcclusionChanged),
                name: NSWindow.didChangeOcclusionStateNotification,
                object: window
            )
            isWindowVisible = window.occlusionState.contains(.visible) || !window.isVisible
            installMouseMonitor()
            WhiskerInsetsDispatcher.attach(self)
            mountWhenSized()
            resumeRuntime()
            flushModuleEventsOnMain()
        } else {
            removeMouseMonitor()
            pauseRuntime()
            WhiskerInsetsDispatcher.detach(self)
        }
    }

    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        if runtimeHandle != nil { requestFrame() }
    }

    public override func layout() {
        super.layout()
        WhiskerInsetsDispatcher.update(self)
        mountWhenSized()
        if runtimeHandle != nil { requestFrame() }
    }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsLayout = true
    }

    public override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let mouseTrackingArea { removeTrackingArea(mouseTrackingArea) }
        let area = NSTrackingArea(
            rect: .zero,
            options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        mouseTrackingArea = area
    }

    private func mountWhenSized() {
        let viewport = logicalBounds
        guard runtimeHandle == nil, window != nil, viewport.width > 0, viewport.height > 0 else { return }
        let token = Unmanaged.passRetained(self).toOpaque()
        hostToken = token
        moduleEventSinkEpoch &+= 1
        let sinkEpoch = moduleEventSinkEpoch
        WhiskerModuleEventCenter.installEventSink(owner: self) { [weak self] module, event, payload in
            guard let self else { return }
            let shouldSchedule = self.pendingModuleEvents.offer(PendingModuleEvent(
                epoch: sinkEpoch,
                module: module,
                event: event,
                payload: payload
            ))
            if shouldSchedule {
                DispatchQueue.main.async { [weak self] in self?.flushModuleEventsOnMain() }
            }
        }
        runtimeHandle = whiskerViewCreate(
            Float(viewport.width), Float(viewport.height), Float(backingScale),
            IOSHostCapabilities.current.rawValue,
            whiskerIOSRequestFrame, token,
            whiskerIOSBootstrap, token,
            whiskerIOSMeasure, token,
            whiskerIOSPresentFrame, token,
            whiskerIOSResourceCommand, token,
            whiskerIOSInvokeModule, whiskerIOSObserveModule, token
        )
        if runtimeHandle != nil {
            runtimePaused = false
            driveRuntimeFrame(timestampMs: ProcessInfo.processInfo.systemUptime * 1_000)
            requestFrame()
        } else {
            moduleEventSinkEpoch &+= 1
            WhiskerModuleEventCenter.installEventSink(owner: self, nil)
            Unmanaged<WhiskerView>.fromOpaque(token).release()
            hostToken = nil
        }
    }

    func applyResourceCommand(_ command: HostResourceCommand) -> Bool {
        switch command {
        case let .load(id, generation, kind, source):
            guard kind == .rasterImage else {
                DispatchQueue.main.async { [weak self] in
                    self?.dispatchRasterResourceEvent(WhiskerRasterResourceEvent(
                        id: id,
                        generation: generation,
                        state: .failed,
                        failureCode: .unsupported,
                        diagnostic: "resource kind is unsupported by the macOS Host"
                    ))
                }
                return true
            }
            return resourceService.load(id: id, generation: generation, source: source)
        case let .release(id, generation):
            _ = resourceService.release(id: id, generation: generation)
            return true
        }
    }

    private func dispatchRasterResourceEvent(_ event: WhiskerRasterResourceEvent) {
        rasterResourceObserver?(event)
        guard let handle = runtimeHandle else { return }
        _ = withMobileResourceEvent(event) { raw in
            whiskerViewDispatchResourceEvent(handle, &raw)
        }
    }

    /// Observes window mouse events without consuming them, so native controls
    /// and scroll views below the root still receive and handle every event.
    private func installMouseMonitor() {
        guard mouseMonitor == nil else { return }
        let mask: NSEvent.EventTypeMask = [
            .leftMouseDown, .leftMouseUp, .leftMouseDragged,
            .rightMouseDown, .rightMouseUp, .rightMouseDragged,
            .otherMouseDown, .otherMouseUp, .otherMouseDragged,
            .mouseMoved,
        ]
        mouseMonitor = NSEvent.addLocalMonitorForEvents(matching: mask) { [weak self] event in
            self?.observeMouse(event)
            return event
        }
    }

    private func removeMouseMonitor() {
        if let mouseMonitor { NSEvent.removeMonitor(mouseMonitor) }
        mouseMonitor = nil
        mouseCaptured = false
    }

    private func observeMouse(_ event: NSEvent) {
        guard event.window === window, runtimeHandle != nil else { return }
        let location = convert(event.locationInWindow, from: nil)
        let inside = bounds.contains(location)
        let pressed = UInt32(truncatingIfNeeded: NSEvent.pressedMouseButtons)
        let hostEvent: HostPointerEvent
        switch event.type {
        case .leftMouseDown, .rightMouseDown, .otherMouseDown:
            guard inside || mouseCaptured else { return }
            mouseCaptured = true
            hostEvent = .down
        case .leftMouseUp, .rightMouseUp, .otherMouseUp:
            guard mouseCaptured || inside else { return }
            if pressed == 0 { mouseCaptured = false }
            hostEvent = .up
        default:
            guard inside || mouseCaptured else { return }
            hostEvent = .move
        }
        let changedButton: Int16 = switch event.type {
        case .leftMouseDown, .rightMouseDown, .otherMouseDown,
             .leftMouseUp, .rightMouseUp, .otherMouseUp:
            whiskerButton(for: event.buttonNumber)
        default:
            -1
        }
        let logicalPosition = logicalPointerPosition(location, viewport: logicalBounds)
        let input = WhiskerPointerDispatch(
            timestampMs: event.timestamp * 1_000,
            event: hostEvent.rawValue,
            pointerID: Self.mousePointerID,
            pointerKind: HostPointerKind.mouse.rawValue,
            x: Float(logicalPosition.x),
            y: Float(logicalPosition.y),
            buttons: pressed & 0x1f,
            changedButton: changedButton
        )
        guard let handle = runtimeHandle, input.x.isFinite, input.y.isFinite else { return }
        _ = dispatchWhiskerPointer(
            handle: handle,
            input: input,
            scrollOffsets: takeScrollOffsets()
        )
    }

    /// AppKit numbers buttons left, right, middle; the shared pointer model uses left, middle, right.
    private func whiskerButton(for buttonNumber: Int) -> Int16 {
        switch buttonNumber {
        case 1: 2
        case 2: 1
        default: Int16(clamping: buttonNumber)
        }
    }

    private func takeScrollOffsets() -> [UInt64: CGPoint] {
        let result = dirtyScrollOffsets
        dirtyScrollOffsets.removeAll(keepingCapacity: true)
        return result
    }

    private func unmount() {
        moduleEventSinkEpoch &+= 1
        WhiskerModuleEventCenter.installEventSink(owner: self, nil)
        let handle = runtimeHandle
        let ownedRuntime = handle != nil || hostToken != nil
        runtimeHandle = nil
        runtimePaused = false
        displayLink?.invalidate()
        displayLink = nil
        if let handle { whiskerViewDestroy(handle) }
        // Avoid initializing the lazy scene from `deinit` for a View that was
        // never mounted; its initializer captures `weak self`.
        if ownedRuntime { scene.clear() }
        pendingModuleEvents.clear()
        dirtyScrollOffsets.removeAll(keepingCapacity: false)
        if let token = hostToken {
            Unmanaged<WhiskerView>.fromOpaque(token).release()
            hostToken = nil
        }
    }

    func requestFrame() {
        guard runtimeHandle != nil, !runtimePaused, isWindowVisible, window != nil else { return }
        if displayLink == nil {
            let link = displayLink(target: self, selector: #selector(driveFrame(_:)))
            link.add(to: .main, forMode: .common)
            displayLink = link
        }
        displayLink?.isPaused = false
    }

    @objc private func driveFrame(_ link: CADisplayLink) {
        guard runtimeHandle != nil, !runtimePaused, isWindowVisible else {
            link.isPaused = true
            return
        }
        let idle = driveRuntimeFrame(timestampMs: link.timestamp * 1_000)
        link.isPaused = idle
    }

    @discardableResult
    private func driveRuntimeFrame(timestampMs: Double) -> Bool {
        textMeasurementCache.removeAll()
        defer { textMeasurementCache.removeAll() }
        guard let handle = runtimeHandle else { return true }
        return whiskerViewTick(
            handle,
            timestampMs,
            Float(logicalBounds.width), Float(logicalBounds.height), Float(backingScale)
        )
    }

    /// A covered or minimized window stops rendering; losing key focus does not.
    @objc private func windowOcclusionChanged() {
        guard let window else { return }
        isWindowVisible = window.occlusionState.contains(.visible)
        if isWindowVisible {
            mountWhenSized()
            resumeRuntime()
            requestFrame()
        } else {
            pauseRuntime()
        }
    }

    private func pauseRuntime() {
        displayLink?.invalidate()
        displayLink = nil
        guard let handle = runtimeHandle, !runtimePaused else { return }
        if whiskerViewPause(handle) { runtimePaused = true }
    }

    private func resumeRuntime() {
        guard isWindowVisible, window != nil,
              let handle = runtimeHandle, runtimePaused else { return }
        if whiskerViewResume(handle) { runtimePaused = false }
    }

    func bootstrap(_ raw: WhiskerMobileBootstrap) -> Bool {
        HostElementBootstrap.bind(raw)
    }

    func applyFrame(
        _ frame: WhiskerMobileFrame,
        response: inout WhiskerMobileApplyResponse
    ) -> Bool {
        textMeasurementCache.removeAll()
        return scene.applyFrame(frame, response: &response)
    }

    private func dispatchElementEvent(node: UInt64, name: String, detail: WhiskerValue) {
        scene.dispatchOrDefer { [weak self] in
            guard let self, let handle = runtimeHandle else { return }
            var raw = detail.toRaw()
            defer { WhiskerValue.releaseRaw(&raw) }
            let nameBytes = Array(name.utf8)
            nameBytes.withUnsafeBytes { nameBuffer in
                _ = whiskerViewDispatchEvent(
                    handle,
                    ProcessInfo.processInfo.systemUptime * 1_000,
                    node,
                    nameBuffer.bindMemory(to: UInt8.self).baseAddress,
                    nameBytes.count,
                    &raw
                )
            }
        }
    }

    func invokeModule(
        module name: String,
        method: String,
        rawArgs: UnsafePointer<WhiskerValueRaw>?,
        argumentCount: Int,
        isAsync: Bool,
        result: @escaping WhiskerModuleResult,
        resultData: UnsafeMutableRawPointer?
    ) -> Bool {
        modules.invoke(
            module: name,
            method: method,
            rawArgs: rawArgs,
            argumentCount: argumentCount,
            isAsync: isAsync,
            result: result,
            resultData: resultData
        )
    }

    func observeModule(module: String, event: String, observing: Bool) {
        modules.observe(owner: self, module: module, event: event, observing: observing)
    }

    private func flushModuleEventsOnMain() {
        dispatchPrecondition(condition: .onQueue(.main))
        let pending = pendingModuleEvents.drain(epoch: moduleEventSinkEpoch)
        guard !pending.isEmpty else { return }
        scene.dispatchOrDefer { [weak self] in
            guard let self, let handle = runtimeHandle else { return }
            var dispatched = false
            for moduleEvent in pending {
                var raw = moduleEvent.payload.toRaw()
                defer { WhiskerValue.releaseRaw(&raw) }
                let moduleBytes = Array(moduleEvent.module.utf8)
                let eventBytes = Array(moduleEvent.event.utf8)
                moduleBytes.withUnsafeBytes { moduleBuffer in
                    eventBytes.withUnsafeBytes { eventBuffer in
                        dispatched = whiskerViewDispatchModuleEvent(
                            handle,
                            moduleBuffer.bindMemory(to: UInt8.self).baseAddress,
                            moduleBytes.count,
                            eventBuffer.bindMemory(to: UInt8.self).baseAddress,
                            eventBytes.count,
                            &raw
                        ) || dispatched
                    }
                }
            }
            if dispatched { requestFrame() }
        }
    }
}
#endif
