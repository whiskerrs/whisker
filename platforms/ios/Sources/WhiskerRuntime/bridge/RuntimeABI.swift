import Foundation
import WhiskerModule

typealias WhiskerRequestFrame = WhiskerMobileRequestFrameCallback
typealias WhiskerBootstrapHost = WhiskerMobileBootstrapCallback
typealias WhiskerMeasureHost = WhiskerMobileMeasureCallback
typealias WhiskerPresentFrame = WhiskerMobilePresentFrameCallback
typealias WhiskerResourceCommandHost = WhiskerMobileResourceCommandCallback
typealias WhiskerModuleResult = WhiskerMobileModuleResultCallback
typealias WhiskerInvokeModule = WhiskerMobileInvokeModuleCallback
typealias WhiskerObserveModule = WhiskerMobileObserveModuleCallback

func whiskerViewCreate(
    _ width: Float, _ height: Float, _ scale: Float,
    _ capabilities: WhiskerMobileHostCapabilities,
    _ requestFrame: WhiskerRequestFrame, _ requestData: UnsafeMutableRawPointer?,
    _ bootstrap: WhiskerBootstrapHost, _ bootstrapData: UnsafeMutableRawPointer?,
    _ measure: WhiskerMeasureHost, _ measureData: UnsafeMutableRawPointer?,
    _ presentFrame: WhiskerPresentFrame, _ presentData: UnsafeMutableRawPointer?,
    _ resourceCommand: WhiskerResourceCommandHost, _ resourceData: UnsafeMutableRawPointer?,
    _ invokeModule: WhiskerInvokeModule, _ observeModule: WhiskerObserveModule,
    _ moduleData: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer? {
    var capabilities = capabilities
    return withUnsafePointer(to: &capabilities) { capabilities in
        whisker_view_create(
            width, height, scale, capabilities,
            requestFrame, requestData,
            bootstrap, bootstrapData,
            measure, measureData,
            presentFrame, presentData,
            resourceCommand, resourceData,
            invokeModule, observeModule, moduleData
        )
    }
}

func whiskerViewTick(
    _ handle: UnsafeMutableRawPointer?, _ timestampMs: Double,
    _ width: Float, _ height: Float, _ scale: Float
) -> Bool {
    whisker_view_tick(handle, timestampMs, width, height, scale)
}

func whiskerViewPause(_ handle: UnsafeMutableRawPointer?) -> Bool {
    whisker_view_pause(handle)
}

func whiskerViewResume(_ handle: UnsafeMutableRawPointer?) -> Bool {
    whisker_view_resume(handle)
}

func whiskerViewDestroy(_ handle: UnsafeMutableRawPointer?) {
    whisker_view_destroy(handle)
}

func whiskerViewDispatchEvent(
    _ handle: UnsafeMutableRawPointer?, _ timestampMs: Double, _ node: UInt64,
    _ name: UnsafePointer<UInt8>?, _ nameLength: Int,
    _ detail: UnsafePointer<WhiskerValueRaw>?
) -> Bool {
    whisker_view_dispatch_event(handle, timestampMs, node, name, nameLength, detail)
}

func whiskerViewDispatchPointer(
    _ handle: UnsafeMutableRawPointer?,
    _ timestampMs: Double,
    _ event: UInt32,
    _ pointerID: UInt64,
    _ pointerKind: UInt32,
    _ x: Float,
    _ y: Float,
    _ buttons: UInt32,
    _ changedButton: Int16,
    _ scrollNodes: UnsafePointer<UInt64>?,
    _ scrollOffsets: UnsafePointer<Float>?,
    _ scrollCount: Int
) -> Bool {
    whisker_view_dispatch_pointer(
        handle, timestampMs, event, pointerID, pointerKind,
        x, y, buttons, changedButton, scrollNodes, scrollOffsets, scrollCount
    )
}

struct WhiskerPointerDispatch: Equatable {
    let timestampMs: Double
    let event: UInt32
    let pointerID: UInt64
    let pointerKind: UInt32
    let x: Float
    let y: Float
    let buttons: UInt32
    let changedButton: Int16
}

func makeWhiskerPointerDispatch(
    timestampMs: Double,
    event: HostPointerEvent,
    pointerID: UInt64,
    pointerKind: HostPointerKind,
    x: Float,
    y: Float
) -> WhiskerPointerDispatch? {
    guard timestampMs.isFinite, pointerID != 0, x.isFinite, y.isFinite else { return nil }
    return WhiskerPointerDispatch(
        timestampMs: timestampMs,
        event: event.rawValue,
        pointerID: pointerID,
        pointerKind: pointerKind.rawValue,
        x: x,
        y: y,
        buttons: event.buttons,
        changedButton: pointerKind.changedButton(for: event)
    )
}

@discardableResult
func dispatchWhiskerPointer(
    handle: UnsafeMutableRawPointer,
    input: WhiskerPointerDispatch,
    scrollOffsets: [UInt64: CGPoint]
) -> Bool {
    let nodes = Array(scrollOffsets.keys)
    var values = [Float]()
    values.reserveCapacity(nodes.count * 2)
    for node in nodes {
        let offset = scrollOffsets[node] ?? .zero
        values.append(Float(offset.x))
        values.append(Float(offset.y))
    }
    return nodes.withUnsafeBufferPointer { nodeBuffer in
        values.withUnsafeBufferPointer { valueBuffer in
            whiskerViewDispatchPointer(
                handle,
                input.timestampMs,
                input.event,
                input.pointerID,
                input.pointerKind,
                input.x,
                input.y,
                input.buttons,
                input.changedButton,
                nodeBuffer.baseAddress,
                valueBuffer.baseAddress,
                nodes.count
            )
        }
    }
}

func whiskerViewDispatchModuleEvent(
    _ handle: UnsafeMutableRawPointer?,
    _ module: UnsafePointer<UInt8>?, _ moduleLength: Int,
    _ event: UnsafePointer<UInt8>?, _ eventLength: Int,
    _ payload: UnsafePointer<WhiskerValueRaw>?
) -> Bool {
    whisker_view_dispatch_module_event(
        handle, module, moduleLength, event, eventLength, payload
    )
}

func whiskerViewDispatchResourceEvent(
    _ handle: UnsafeMutableRawPointer?,
    _ event: UnsafePointer<WhiskerMobileResourceEvent>?
) -> Bool {
    whisker_view_dispatch_resource_event(handle, event)
}

let whiskerIOSRequestFrame: WhiskerRequestFrame = { data in
    guard let data else { return }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    DispatchQueue.main.async { view.requestFrame() }
}

let whiskerIOSBootstrap: WhiskerBootstrapHost = { data, bootstrap in
    guard let data, let bootstrap else { return false }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    return view.bootstrap(bootstrap.pointee)
}

let whiskerIOSPresentFrame: WhiskerPresentFrame = { data, frame, response in
    guard let data, let frame, let response else { return false }
    precondition(Thread.isMainThread, "Whisker frame presentation must run on the UI thread")
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    return view.applyFrame(frame.pointee, response: &response.pointee)
}

let whiskerIOSResourceCommand: WhiskerResourceCommandHost = { data, command in
    guard let data, let command, let decoded = hostResourceCommand(command.pointee) else {
        return false
    }
    precondition(Thread.isMainThread, "Whisker resource commands must run on the UI thread")
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    return view.applyResourceCommand(decoded)
}

let whiskerIOSInvokeModule: WhiskerInvokeModule = {
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
        let result,
        let module = String(
            bytes: UnsafeBufferPointer(start: moduleBytes, count: moduleLength),
            encoding: .utf8
        ),
        let method = String(
            bytes: UnsafeBufferPointer(start: methodBytes, count: methodLength),
            encoding: .utf8
        )
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

let whiskerIOSObserveModule: WhiskerObserveModule = {
    data, moduleBytes, moduleLength, eventBytes, eventLength, observing in
    guard
        let data,
        let moduleBytes,
        let eventBytes,
        let module = String(
            bytes: UnsafeBufferPointer(start: moduleBytes, count: moduleLength),
            encoding: .utf8
        ),
        let event = String(
            bytes: UnsafeBufferPointer(start: eventBytes, count: eventLength),
            encoding: .utf8
        )
    else { return }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    view.observeModule(module: module, event: event, observing: observing)
}

func hostString(_ value: WhiskerStringRef) -> String {
    guard let pointer = value.ptr, value.len > 0 else { return "" }
    return String(
        decoding: UnsafeBufferPointer(
            start: UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
            count: value.len
        ),
        as: UTF8.self
    )
}
