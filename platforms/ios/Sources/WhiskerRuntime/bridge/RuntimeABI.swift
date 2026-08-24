import Foundation
import WhiskerModule

typealias WhiskerRequestFrame = @convention(c) (UnsafeMutableRawPointer?) -> Void
typealias WhiskerBootstrapHost = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileBootstrap>?
) -> Bool
typealias WhiskerMeasureHost = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileMeasureRequest>?, Int,
    UnsafeMutablePointer<WhiskerMobileMeasureResponse>?
) -> Bool
typealias WhiskerPresentFrame = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileFrame>?,
    UnsafeMutablePointer<WhiskerMobileApplyResponse>?
) -> Bool
typealias WhiskerResourceCommandHost = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerMobileResourceCommand>?
) -> Bool
typealias WhiskerModuleResult = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<WhiskerValueRaw>?
) -> Void
typealias WhiskerInvokeModule = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?, Int,
    UnsafePointer<UInt8>?, Int,
    UnsafePointer<WhiskerValueRaw>?, Int,
    Bool,
    WhiskerModuleResult,
    UnsafeMutableRawPointer?
) -> Bool
typealias WhiskerObserveModule = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?, Int,
    UnsafePointer<UInt8>?, Int,
    Bool
) -> Void

@_silgen_name("whisker_view_create")
func whiskerViewCreate(
    _ width: Float, _ height: Float, _ scale: Float,
    _ requestFrame: WhiskerRequestFrame, _ requestData: UnsafeMutableRawPointer?,
    _ bootstrap: WhiskerBootstrapHost, _ bootstrapData: UnsafeMutableRawPointer?,
    _ measure: WhiskerMeasureHost, _ measureData: UnsafeMutableRawPointer?,
    _ presentFrame: WhiskerPresentFrame, _ presentData: UnsafeMutableRawPointer?,
    _ resourceCommand: WhiskerResourceCommandHost, _ resourceData: UnsafeMutableRawPointer?,
    _ invokeModule: WhiskerInvokeModule, _ observeModule: WhiskerObserveModule,
    _ moduleData: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer?

@_silgen_name("whisker_view_tick")
func whiskerViewTick(
    _ handle: UnsafeMutableRawPointer?, _ timestampMs: Double,
    _ width: Float, _ height: Float, _ scale: Float
) -> Bool

@_silgen_name("whisker_view_destroy")
func whiskerViewDestroy(_ handle: UnsafeMutableRawPointer?)

@_silgen_name("whisker_view_dispatch_event")
func whiskerViewDispatchEvent(
    _ handle: UnsafeMutableRawPointer?, _ timestampMs: Double, _ node: UInt64,
    _ name: UnsafePointer<UInt8>?, _ nameLength: Int,
    _ detail: UnsafePointer<WhiskerValueRaw>?
) -> Bool

@_silgen_name("whisker_view_dispatch_module_event")
func whiskerViewDispatchModuleEvent(
    _ handle: UnsafeMutableRawPointer?,
    _ module: UnsafePointer<UInt8>?, _ moduleLength: Int,
    _ event: UnsafePointer<UInt8>?, _ eventLength: Int,
    _ payload: UnsafePointer<WhiskerValueRaw>?
) -> Bool

@_silgen_name("whisker_view_dispatch_resource_event")
func whiskerViewDispatchResourceEvent(
    _ handle: UnsafeMutableRawPointer?,
    _ event: UnsafePointer<WhiskerMobileResourceEvent>?
) -> Bool

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
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    if Thread.isMainThread {
        return view.applyFrame(frame.pointee, response: &response.pointee)
    }
    return DispatchQueue.main.sync {
        view.applyFrame(frame.pointee, response: &response.pointee)
    }
}

let whiskerIOSResourceCommand: WhiskerResourceCommandHost = { data, command in
    guard let data, let command, let decoded = hostResourceCommand(command.pointee) else {
        return false
    }
    let view = Unmanaged<WhiskerView>.fromOpaque(data).takeUnretainedValue()
    if Thread.isMainThread {
        return view.applyResourceCommand(decoded)
    }
    return DispatchQueue.main.sync { view.applyResourceCommand(decoded) }
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
