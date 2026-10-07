import Foundation
import FirebaseAnalytics
import FirebaseCore
import WhiskerModule

@WhiskerModule
public final class FirebaseAnalyticsModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("FirebaseAnalytics")
            Function("logEvent") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let name = args[0].asString, case .map(let params) = args[1] else {
                    return Self.failure("invalid-argument", "Expected an event name and parameters")
                }
                return Self.configured { Analytics.logEvent(name, parameters: Self.decode(params)) }
            }
            Function("setUserId") { (args: [WhiskerValue]) -> WhiskerValue in
                Self.configured { Analytics.setUserID(args.first?.asString) }
            }
            Function("setUserProperty") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let name = args[0].asString else {
                    return Self.failure("invalid-argument", "Expected a user property name and value")
                }
                return Self.configured { Analytics.setUserProperty(args[1].asString, forName: name) }
            }
            Function("setAnalyticsCollectionEnabled") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let enabled = args.first?.asBool else {
                    return Self.failure("invalid-argument", "Expected a boolean")
                }
                return Self.configured { Analytics.setAnalyticsCollectionEnabled(enabled) }
            }
            Function("setDefaultEventParameters") { (args: [WhiskerValue]) -> WhiskerValue in
                guard case .map(let params) = args.first else {
                    return Self.failure("invalid-argument", "Expected parameters")
                }
                return Self.configured { Analytics.setDefaultEventParameters(params.isEmpty ? nil : Self.decode(params)) }
            }
            Function("resetAnalyticsData") { (_: [WhiskerValue]) -> WhiskerValue in
                Self.configured { Analytics.resetAnalyticsData() }
            }
            AsyncFunction("getAppInstanceId") { (_: [WhiskerValue], promise: WhiskerPromise) in
                guard FirebaseApp.app() != nil else { promise.resolve(Self.notConfigured()); return }
                promise.resolve(Self.success(Analytics.appInstanceID().map { .string($0) } ?? .null))
            }
            Function("setConsent") { (args: [WhiskerValue]) -> WhiskerValue in
                guard case .map(let fields) = args.first else {
                    return Self.failure("invalid-argument", "Expected consent settings")
                }
                let types: [String: ConsentType] = [
                    "analytics_storage": .analyticsStorage, "ad_storage": .adStorage,
                    "ad_user_data": .adUserData, "ad_personalization": .adPersonalization,
                ]
                var consent: [ConsentType: ConsentStatus] = [:]
                for (key, value) in fields {
                    guard let type = types[key], let granted = value.asBool else { continue }
                    consent[type] = granted ? .granted : .denied
                }
                return Self.configured { Analytics.setConsent(consent) }
            }
            Function("setSessionTimeout") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let millis = args.first?.asInt else {
                    return Self.failure("invalid-argument", "Expected milliseconds")
                }
                return Self.configured { Analytics.setSessionTimeoutInterval(TimeInterval(millis) / 1000) }
            }
        }
    }

    private static func configured(_ body: () -> Void) -> WhiskerValue {
        guard FirebaseApp.app() != nil else { return notConfigured() }
        body()
        return success(.null)
    }

    private static func decode(_ params: [String: WhiskerValue]) -> [String: Any] {
        params.compactMapValues { value -> Any? in
            switch value {
            case .string(let v): return v
            case .int(let v): return NSNumber(value: v)
            case .float(let v): return NSNumber(value: v)
            case .array(let items):
                return items.compactMap { item -> [String: Any]? in
                    guard case .map(let fields) = item else { return nil }
                    return decode(fields)
                }
            default: return nil
            }
        }
    }

    private static func success(_ value: WhiskerValue) -> WhiskerValue { .map(["value": value]) }
    private static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
    private static func notConfigured() -> WhiskerValue {
        failure("app-not-configured", "Call Analytics::instance() before using Analytics")
    }
}
