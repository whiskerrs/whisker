import UIKit
import WhiskerModule

/** The single iOS View that owns a Whisker runtime and its native scene. */
public final class WhiskerView: UIView {
    private var displayLink: CADisplayLink?
    private var hostToken: UnsafeMutableRawPointer?
    private var runtimeHandle: UnsafeMutableRawPointer?
    private var runtimePaused = false
    private var isApplicationActive = true
    private var moduleEventSinkEpoch: UInt64 = 0
    private let pendingModuleEvents = PendingModuleEvents()
    private lazy var pointerInputRecognizer: WhiskerTouchObserverGestureRecognizer = {
        let recognizer = WhiskerTouchObserverGestureRecognizer(target: nil, action: nil)
        recognizer.touchHandler = { [weak self] touches, event in
            self?.dispatchTouches(touches, event: event)
        }
        return recognizer
    }()
    private var touchIdentities = HostTouchIdentityMap()
    private var dirtyScrollOffsets: [UInt64: CGPoint] = [:]
    private let modules = HostModuleDispatcher()
    private let resources = HostResourceStore()
    private lazy var resourceService = HostResourceService(store: resources)
    private var rasterResourceObserver: ((WhiskerRasterResourceEvent) -> Void)?
    private lazy var scene = HostScene(
        root: self,
        resources: resources,
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

    private var logicalBounds: CGRect {
        edgeToEdgeViewportBounds(bounds, safeAreaInsets: safeAreaInsets)
    }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        autoresizingMask = [.flexibleWidth, .flexibleHeight]
        isMultipleTouchEnabled = true
        addGestureRecognizer(pointerInputRecognizer)
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(applicationDidBecomeActive),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
        resourceService.eventHandler = { [weak self] event in
            DispatchQueue.main.async { self?.dispatchRasterResourceEvent(event) }
        }
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(applicationWillResignActive),
            name: UIApplication.willResignActiveNotification,
            object: nil
        )
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
        unmount()
        NotificationCenter.default.removeObserver(self)
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            WhiskerInsetsDispatcher.attach(self)
            mountWhenSized()
            resumeRuntime()
            flushModuleEventsOnMain()
        } else {
            pauseRuntime()
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
            Float(viewport.width), Float(viewport.height), Float(window?.screen.scale ?? 1),
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
                        diagnostic: "resource kind is unsupported by the iOS Host"
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

    private func dispatchTouches(_ touches: Set<UITouch>, event: HostPointerEvent) {
        for touch in touches {
            let key = ObjectIdentifier(touch)
            let pointerID: UInt64
            if event == .down {
                pointerID = touchIdentities.begin(key)
            } else if let existing = touchIdentities.existing(key) {
                pointerID = existing
            } else { continue }
            let location = touch.location(in: self)
            let viewport = logicalBounds
            let logicalPosition = logicalPointerPosition(location, viewport: viewport)
            dispatchTouchSample(
                timestampMs: touch.timestamp * 1_000,
                event: event,
                pointerID: pointerID,
                pointerKind: hostPointerKind(for: touch.type),
                x: Float(logicalPosition.x),
                y: Float(logicalPosition.y)
            )
            if event == .up || event == .cancel { touchIdentities.end(key) }
        }
    }

    private func dispatchTouchSample(
        timestampMs: Double,
        event: HostPointerEvent,
        pointerID: UInt64,
        pointerKind: HostPointerKind,
        x: Float,
        y: Float
    ) {
        guard let handle = runtimeHandle,
              let input = makeWhiskerPointerDispatch(
                  timestampMs: timestampMs,
                  event: event,
                  pointerID: pointerID,
                  pointerKind: pointerKind,
                  x: x,
                  y: y
              ) else { return }
        _ = dispatchWhiskerPointer(
            handle: handle,
            input: input,
            scrollOffsets: takeScrollOffsets()
        )
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
        // never mounted. Its initializer captures `weak self`, which UIKit
        // correctly rejects once object deallocation has begun.
        if ownedRuntime { scene.clear() }
        pendingModuleEvents.clear()
        touchIdentities.clear()
        dirtyScrollOffsets.removeAll(keepingCapacity: false)
        if let token = hostToken {
            Unmanaged<WhiskerView>.fromOpaque(token).release()
            hostToken = nil
        }
    }

    func requestFrame() {
        guard runtimeHandle != nil, !runtimePaused, isApplicationActive, window != nil else { return }
        if displayLink == nil {
            displayLink = CADisplayLink(target: self, selector: #selector(driveFrame(_:)))
            displayLink?.add(to: .main, forMode: .common)
        }
        displayLink?.isPaused = false
    }

    @objc private func driveFrame(_ link: CADisplayLink) {
        guard runtimeHandle != nil, !runtimePaused, isApplicationActive else {
            link.isPaused = true
            return
        }
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
        resumeRuntime()
        requestFrame()
    }

    @objc private func applicationWillResignActive() {
        isApplicationActive = false
        pauseRuntime()
    }

    private func pauseRuntime() {
        displayLink?.invalidate()
        displayLink = nil
        guard let handle = runtimeHandle, !runtimePaused else { return }
        if whiskerViewPause(handle) { runtimePaused = true }
    }

    private func resumeRuntime() {
        guard isApplicationActive, window != nil,
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
        scene.applyFrame(frame, response: &response)
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
