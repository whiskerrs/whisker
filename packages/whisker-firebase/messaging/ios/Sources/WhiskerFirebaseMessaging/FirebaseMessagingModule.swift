import Foundation
import FirebaseCore
import FirebaseMessaging
import UIKit
import UserNotifications
import WhiskerModule

@WhiskerModule
public final class FirebaseMessagingModule: Module {
    private let bridge = MessagingBridge()

    public override func definition() -> ModuleDefinition {
        // Modules are installed from didFinishLaunching, early enough to receive the
        // notification response that launched the app.
        bridge.module = self
        UNUserNotificationCenter.current().delegate = bridge
        bridge.observeLaunch()
        return ModuleDefinition {
            Name("FirebaseMessaging")
            Events("message", "messageOpened", "token")
            OnStartObserving("messageOpened") { self.bridge.setOpenedObserved(true) }
            OnStopObserving("messageOpened") { self.bridge.setOpenedObserved(false) }
            AsyncFunction("requestPermission") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard case .map(let fields) = args.first else {
                    promise.resolve(Self.failure("invalid-argument", "Expected permission options")); return
                }
                var options: UNAuthorizationOptions = []
                if fields["alert"]?.asBool == true { options.insert(.alert) }
                if fields["badge"]?.asBool == true { options.insert(.badge) }
                if fields["sound"]?.asBool == true { options.insert(.sound) }
                if fields["provisional"]?.asBool == true { options.insert(.provisional) }
                UNUserNotificationCenter.current().requestAuthorization(options: options) { _, error in
                    if let error { promise.resolve(Self.failure("unknown", error.localizedDescription)); return }
                    Self.settings { settings in
                        if settings.authorizationStatus != .denied {
                            DispatchQueue.main.async { UIApplication.shared.registerForRemoteNotifications() }
                        }
                        promise.resolve(Self.success(Self.encode(settings)))
                    }
                }
            }
            AsyncFunction("getNotificationSettings") { (_: [WhiskerValue], promise: WhiskerPromise) in
                Self.settings { promise.resolve(Self.success(Self.encode($0))) }
            }
            AsyncFunction("getToken") { (_: [WhiskerValue], promise: WhiskerPromise) in
                guard let messaging = self.messaging(promise) else { return }
                messaging.token { token, error in
                    if let error { promise.resolve(Self.sdkFailure(error)); return }
                    promise.resolve(Self.success(token.map { .string($0) } ?? .null))
                }
            }
            AsyncFunction("deleteToken") { (_: [WhiskerValue], promise: WhiskerPromise) in
                guard let messaging = self.messaging(promise) else { return }
                messaging.deleteToken { error in promise.resolve(error.map(Self.sdkFailure) ?? Self.success(.null)) }
            }
            Function("getApnsToken") { (_: [WhiskerValue]) -> WhiskerValue in
                guard FirebaseApp.app() != nil, let token = Messaging.messaging().apnsToken else {
                    return Self.success(.null)
                }
                return Self.success(.string(token.map { String(format: "%02x", $0) }.joined()))
            }
            AsyncFunction("subscribeToTopic") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let topic = args.first?.asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected a topic")); return
                }
                guard let messaging = self.messaging(promise) else { return }
                messaging.subscribe(toTopic: topic) { error in
                    promise.resolve(error.map(Self.sdkFailure) ?? Self.success(.null))
                }
            }
            AsyncFunction("unsubscribeFromTopic") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let topic = args.first?.asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected a topic")); return
                }
                guard let messaging = self.messaging(promise) else { return }
                messaging.unsubscribe(fromTopic: topic) { error in
                    promise.resolve(error.map(Self.sdkFailure) ?? Self.success(.null))
                }
            }
            Function("isAutoInitEnabled") { (_: [WhiskerValue]) -> WhiskerValue in
                guard FirebaseApp.app() != nil else { return Self.notConfigured() }
                return Self.success(.bool(Messaging.messaging().isAutoInitEnabled))
            }
            Function("setAutoInitEnabled") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let enabled = args.first?.asBool else {
                    return Self.failure("invalid-argument", "Expected a boolean")
                }
                guard FirebaseApp.app() != nil else { return Self.notConfigured() }
                Messaging.messaging().isAutoInitEnabled = enabled
                return Self.success(.null)
            }
            Function("setForegroundPresentation") { (args: [WhiskerValue]) -> WhiskerValue in
                guard case .map(let fields) = args.first else {
                    return Self.failure("invalid-argument", "Expected presentation options")
                }
                var options: UNNotificationPresentationOptions = []
                if fields["banner"]?.asBool == true { options.insert(.banner) }
                if fields["list"]?.asBool == true { options.insert(.list) }
                if fields["sound"]?.asBool == true { options.insert(.sound) }
                if fields["badge"]?.asBool == true { options.insert(.badge) }
                self.bridge.setPresentation(options)
                return Self.success(.null)
            }
            AsyncFunction("getInitialMessage") { (_: [WhiskerValue], promise: WhiskerPromise) in
                self.bridge.waitForInitialMessage { promise.resolve(Self.success($0 ?? .null)) }
            }
        }
    }

    /// Messaging requires the default app, which Rust configures before calling in.
    private func messaging(_ promise: WhiskerPromise) -> Messaging? {
        guard FirebaseApp.app() != nil else { promise.resolve(Self.notConfigured()); return nil }
        let messaging = Messaging.messaging()
        if messaging.delegate == nil { messaging.delegate = bridge }
        return messaging
    }

    func emit(_ event: String, _ value: WhiskerValue) {
        let send = { self.sendEvent(event, .map(["value": value])) }
        if Thread.isMainThread { send() } else { DispatchQueue.main.async(execute: send) }
    }

    private static func settings(_ completion: @escaping (UNNotificationSettings) -> Void) {
        UNUserNotificationCenter.current().getNotificationSettings(completionHandler: completion)
    }

    private static func encode(_ settings: UNNotificationSettings) -> WhiskerValue {
        let status: String
        switch settings.authorizationStatus {
        case .notDetermined: status = "not_determined"
        case .denied: status = "denied"
        case .authorized: status = "authorized"
        case .provisional: status = "provisional"
        case .ephemeral: status = "ephemeral"
        @unknown default: status = "denied"
        }
        return .map(["authorization_status": .string(status)])
    }

    static func success(_ value: WhiskerValue) -> WhiskerValue { .map(["value": value]) }
    static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
    private static func notConfigured() -> WhiskerValue {
        failure("app-not-configured", "Call Messaging::instance() before using Firebase Messaging")
    }
    private static func sdkFailure(_ error: Error) -> WhiskerValue {
        let error = error as NSError
        let codes = [0: "unknown", 1: "authentication", 2: "no-access", 3: "timeout", 4: "network",
                     5: "operation-in-progress", 7: "invalid-request", 8: "invalid-topic-name"]
        let code = error.domain == MessagingErrorDomain ? codes[error.code] ?? "unknown" : "unknown"
        return failure(code, error.localizedDescription)
    }
}

/// Receives notification-center and Messaging callbacks, which require an NSObject.
final class MessagingBridge: NSObject, UNUserNotificationCenterDelegate, MessagingDelegate {
    weak var module: FirebaseMessagingModule?
    private let lock = NSLock()
    private var presentation: UNNotificationPresentationOptions = [.banner, .list, .sound, .badge]
    private var openedObserved = false
    private var initialMessage: WhiskerValue?
    private var launchMessageId: String?
    private var launchFinished = false
    private var launchObserver: NSObjectProtocol?

    /// launchOptions name the push that launched the app, so its tap response is the
    /// initial message even if Rust already listens for opened messages.
    func observeLaunch() {
        launchObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didFinishLaunchingNotification, object: nil, queue: nil
        ) { [weak self] notification in
            guard let self else { return }
            let payload = notification.userInfo?[UIApplication.LaunchOptionsKey.remoteNotification] as? [AnyHashable: Any]
            self.lock.lock()
            self.launchMessageId = payload?["gcm.message_id"] as? String
            self.launchFinished = true
            let launchedByPush = self.launchMessageId != nil
            self.lock.unlock()
            if !launchedByPush { self.resolveInitialWaiters() }
            self.launchObserver.map(NotificationCenter.default.removeObserver)
            self.launchObserver = nil
        }
    }

    func setPresentation(_ options: UNNotificationPresentationOptions) {
        lock.lock(); presentation = options; lock.unlock()
    }

    func setOpenedObserved(_ observed: Bool) {
        lock.lock(); openedObserved = observed; lock.unlock()
    }

    private var initialWaiters: [(WhiskerValue?) -> Void] = []

    /// Rust can ask during didFinishLaunching, before launchOptions are known, and the
    /// launch tap's response arrives later still; wait briefly for both. Background
    /// launches name a push but never produce a response.
    func waitForInitialMessage(_ completion: @escaping (WhiskerValue?) -> Void) {
        lock.lock()
        if launchFinished && (launchMessageId == nil || initialMessage != nil) {
            let message = initialMessage
            initialMessage = nil
            lock.unlock()
            completion(message)
            return
        }
        initialWaiters.append(completion)
        lock.unlock()
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
            self?.lock.lock()
            self?.launchMessageId = nil
            self?.lock.unlock()
            self?.resolveInitialWaiters()
        }
    }

    private func resolveInitialWaiters() {
        lock.lock()
        let waiters = initialWaiters
        initialWaiters.removeAll()
        let message = waiters.isEmpty ? nil : initialMessage
        if !waiters.isEmpty { initialMessage = nil }
        launchFinished = true
        lock.unlock()
        waiters.forEach { $0(message) }
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        if let message = Self.remoteMessage(notification) {
            if FirebaseApp.app() != nil {
                Messaging.messaging().appDidReceiveMessage(notification.request.content.userInfo)
            }
            module?.emit("message", message)
        }
        lock.lock(); let options = presentation; lock.unlock()
        completionHandler(options)
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        if let message = Self.remoteMessage(response.notification) {
            let id = response.notification.request.content.userInfo["gcm.message_id"] as? String
            lock.lock()
            let launch = id != nil && id == launchMessageId
            if launch { launchMessageId = nil }
            let opened = openedObserved && !launch
            if !opened { initialMessage = message }
            lock.unlock()
            if opened { module?.emit("messageOpened", message) }
            if launch { resolveInitialWaiters() }
        }
        completionHandler()
    }

    func messaging(_ messaging: Messaging, didReceiveRegistrationToken fcmToken: String?) {
        if let fcmToken { module?.emit("token", .string(fcmToken)) }
    }

    /// FCM notifications carry `gcm.message_id`; other notifications are not reported.
    static func remoteMessage(_ notification: UNNotification) -> WhiskerValue? {
        let content = notification.request.content
        let userInfo = content.userInfo
        guard let id = userInfo["gcm.message_id"] as? String else { return nil }
        var data: [String: WhiskerValue] = [:]
        for (key, value) in userInfo {
            guard let key = key as? String, key != "aps", key != "fcm_options",
                  !key.hasPrefix("gcm."), !key.hasPrefix("google.") else { continue }
            if let value = value as? String { data[key] = .string(value) }
            else if let value = value as? NSNumber { data[key] = .string(value.stringValue) }
        }
        var message: [String: WhiskerValue] = ["message_id": .string(id), "data": .map(data)]
        if let sender = userInfo["google.c.sender.id"] as? String { message["from"] = .string(sender) }
        let image = (userInfo["fcm_options"] as? [String: Any])?["image"] as? String
        if !content.title.isEmpty || !content.body.isEmpty || image != nil {
            var parts: [String: WhiskerValue] = [:]
            if !content.title.isEmpty { parts["title"] = .string(content.title) }
            if !content.body.isEmpty { parts["body"] = .string(content.body) }
            if let image { parts["image_url"] = .string(image) }
            message["notification"] = .map(parts)
        }
        return .map(message)
    }
}

