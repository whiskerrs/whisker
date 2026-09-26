import Foundation
import FirebaseCore
import FirebaseCrashlytics
import WhiskerModule

@WhiskerModule
public final class FirebaseCrashlyticsModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("FirebaseCrashlytics")
            Function("log") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let message = args.first?.asString else {
                    return Self.failure("invalid-argument", "Expected a message")
                }
                return Self.with { $0.log(message) }
            }
            Function("setUserId") { (args: [WhiskerValue]) -> WhiskerValue in
                Self.with { $0.setUserID(args.first?.asString) }
            }
            Function("setCustomKey") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let key = args[0].asString else {
                    return Self.failure("invalid-argument", "Expected a key and value")
                }
                let value: Any
                switch args[1] {
                case .string(let v): value = v
                case .int(let v): value = NSNumber(value: v)
                case .float(let v): value = NSNumber(value: v)
                case .bool(let v): value = NSNumber(value: v)
                default: return Self.failure("invalid-argument", "Unsupported custom key value")
                }
                return Self.with { $0.setCustomValue(value, forKey: key) }
            }
            Function("recordError") { (args: [WhiskerValue]) -> WhiskerValue in
                guard case .map(let report) = args.first, let name = report["name"]?.asString,
                      let reason = report["reason"]?.asString else {
                    return Self.failure("invalid-argument", "Expected an error report")
                }
                let model = ExceptionModel(name: name, reason: reason)
                if case .array(let frames) = report["frames"] {
                    model.stackTrace = frames.compactMap { frame in
                        guard case .map(let fields) = frame, let symbol = fields["symbol"]?.asString else { return nil }
                        return StackFrame(symbol: symbol, file: fields["file"]?.asString ?? "",
                                          line: Int(fields["line"]?.asInt ?? 0))
                    }
                }
                return Self.with { $0.record(exceptionModel: model) }
            }
            Function("recordPanic") { (args: [WhiskerValue]) -> WhiskerValue in
                guard case .map(let report) = args.first, let reason = report["reason"]?.asString else {
                    return Self.failure("invalid-argument", "Expected a panic report")
                }
                var text = reason
                if case .array(let frames) = report["frames"] {
                    for case .map(let frame) in frames {
                        text += "\n  at \(frame["symbol"]?.asString ?? "?")"
                        if let file = frame["file"]?.asString { text += " (\(file):\(frame["line"]?.asInt ?? 0))" }
                    }
                }
                // Crashlytics records through the main queue, which a panicking main thread never
                // reaches, but its terminate handler writes an uncaught NSException synchronously.
                // The exception is raised on a thread without Rust frames, which cannot unwind it.
                let exception = NSException(name: NSExceptionName("RustPanic"), reason: text)
                Thread.detachNewThread { exception.raise() }
                Thread.sleep(forTimeInterval: 5)
                return Self.success(.null)
            }
            Function("isCollectionEnabled") { (_: [WhiskerValue]) -> WhiskerValue in
                guard FirebaseApp.app() != nil else { return Self.notConfigured() }
                return Self.success(.bool(Crashlytics.crashlytics().isCrashlyticsCollectionEnabled()))
            }
            Function("setCollectionEnabled") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let enabled = args.first?.asBool else {
                    return Self.failure("invalid-argument", "Expected a boolean")
                }
                return Self.with { $0.setCrashlyticsCollectionEnabled(enabled) }
            }
            AsyncFunction("checkForUnsentReports") { (_: [WhiskerValue], promise: WhiskerPromise) in
                guard FirebaseApp.app() != nil else { promise.resolve(Self.notConfigured()); return }
                Crashlytics.crashlytics().checkForUnsentReports { promise.resolve(Self.success(.bool($0))) }
            }
            Function("sendUnsentReports") { (_: [WhiskerValue]) -> WhiskerValue in
                Self.with { $0.sendUnsentReports() }
            }
            Function("deleteUnsentReports") { (_: [WhiskerValue]) -> WhiskerValue in
                Self.with { $0.deleteUnsentReports() }
            }
            Function("didCrashOnPreviousExecution") { (_: [WhiskerValue]) -> WhiskerValue in
                guard FirebaseApp.app() != nil else { return Self.notConfigured() }
                return Self.success(.bool(Crashlytics.crashlytics().didCrashDuringPreviousExecution()))
            }
            Function("crash") { (_: [WhiskerValue]) -> WhiskerValue in
                fatalError("Whisker Firebase Crashlytics test crash")
            }
        }
    }

    private static func with(_ body: (Crashlytics) -> Void) -> WhiskerValue {
        guard FirebaseApp.app() != nil else { return notConfigured() }
        body(Crashlytics.crashlytics())
        return success(.null)
    }

    private static func success(_ value: WhiskerValue) -> WhiskerValue { .map(["value": value]) }
    private static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
    private static func notConfigured() -> WhiskerValue {
        failure("app-not-configured", "Call Crashlytics::instance() before using Crashlytics")
    }
}
