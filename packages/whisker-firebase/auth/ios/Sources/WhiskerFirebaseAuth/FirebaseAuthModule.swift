import Foundation
import FirebaseAuth
import WhiskerModule

@WhiskerModule
public final class FirebaseAuthModule: Module {
    private enum Handle {
        case auth(AuthStateDidChangeListenerHandle)
        case idToken(IDTokenDidChangeListenerHandle)
    }

    private let lock = NSLock()
    private var listeners: [Int64: Handle] = [:]

    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("FirebaseAuth")
            Events("authState")
            Function("useEmulator") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let host = args[0].asString,
                      let port = args[1].asInt, (1...65535).contains(port) else {
                    return Self.failure("invalid-argument", "Expected emulator host and port")
                }
                Auth.auth().useEmulator(withHost: host, port: Int(port))
                return Self.success(.null)
            }
            Function("currentUser") { (_: [WhiskerValue]) -> WhiskerValue in
                Self.success(Auth.auth().currentUser.map(Self.encodeUser) ?? .null)
            }
            AsyncFunction("signInAnonymously") { (_: [WhiskerValue], promise: WhiskerPromise) in
                Auth.auth().signInAnonymously(completion: Self.credentialCallback(promise))
            }
            AsyncFunction("signInWithEmailAndPassword") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let email = args[0].asString, let password = args[1].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected email and password")); return
                }
                Auth.auth().signIn(withEmail: email, password: password, completion: Self.credentialCallback(promise))
            }
            AsyncFunction("createUserWithEmailAndPassword") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let email = args[0].asString, let password = args[1].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected email and password")); return
                }
                Auth.auth().createUser(withEmail: email, password: password, completion: Self.credentialCallback(promise))
            }
            AsyncFunction("signInWithCustomToken") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let token = args.first?.asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected a token")); return
                }
                Auth.auth().signIn(withCustomToken: token, completion: Self.credentialCallback(promise))
            }
            AsyncFunction("signInWithCredential") { (args: [WhiskerValue], promise: WhiskerPromise) in
                do {
                    Auth.auth().signIn(with: try Self.credential(args.first), completion: Self.credentialCallback(promise))
                } catch { promise.resolve(Self.failure("invalid-argument", error.localizedDescription)) }
            }
            AsyncFunction("sendPasswordResetEmail") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let email = args.first?.asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected an email")); return
                }
                Auth.auth().sendPasswordReset(withEmail: email, completion: Self.unitCallback(promise))
            }
            Function("signOut") { (_: [WhiskerValue]) -> WhiskerValue in
                do {
                    try Auth.auth().signOut()
                    return Self.success(.null)
                } catch { return Self.sdkFailure(error) }
            }
            Function("addListener") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let id = args[0].asInt, let kind = args[1].asString else {
                    return Self.failure("invalid-argument", "Expected listener id and kind")
                }
                let handle: Handle
                if kind == "id_token" {
                    handle = .idToken(Auth.auth().addIDTokenDidChangeListener { [weak self] _, user in self?.emit(id, user) })
                } else {
                    handle = .auth(Auth.auth().addStateDidChangeListener { [weak self] _, user in self?.emit(id, user) })
                }
                self.lock.lock()
                self.listeners[id] = handle
                self.lock.unlock()
                return Self.success(.null)
            }
            Function("removeListener") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt else {
                    return Self.failure("invalid-argument", "Missing listener id")
                }
                self.lock.lock()
                let handle = self.listeners.removeValue(forKey: id)
                self.lock.unlock()
                handle.map(Self.remove)
                return Self.success(.null)
            }
            Function("removeAllListeners") { (_: [WhiskerValue]) -> WhiskerValue in
                self.lock.lock()
                let handles = self.listeners.values
                self.listeners.removeAll()
                self.lock.unlock()
                handles.forEach(Self.remove)
                return Self.success(.null)
            }
            AsyncFunction("getIdToken") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    user.getIDTokenForcingRefresh(args.count > 1 && args[1].asBool == true) { token, error in
                        if let error { promise.resolve(Self.sdkFailure(error)); return }
                        promise.resolve(Self.success(token.map { .string($0) } ?? .null))
                    }
                }
            }
            AsyncFunction("reload") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    user.reload { error in promise.resolve(Self.userResult(error)) }
                }
            }
            AsyncFunction("updateProfile") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    guard args.count == 2, case .map(let fields) = args[1] else {
                        promise.resolve(Self.failure("invalid-argument", "Expected profile changes")); return
                    }
                    let request = user.createProfileChangeRequest()
                    if let name = fields["display_name"] { request.displayName = name.asString }
                    if let url = fields["photo_url"] { request.photoURL = url.asString.flatMap(URL.init(string:)) }
                    request.commitChanges { error in promise.resolve(Self.userResult(error)) }
                }
            }
            AsyncFunction("updatePassword") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    guard args.count == 2, let password = args[1].asString else {
                        promise.resolve(Self.failure("invalid-argument", "Expected a password")); return
                    }
                    user.updatePassword(to: password, completion: Self.unitCallback(promise))
                }
            }
            AsyncFunction("verifyBeforeUpdateEmail") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    guard args.count == 2, let email = args[1].asString else {
                        promise.resolve(Self.failure("invalid-argument", "Expected an email")); return
                    }
                    user.sendEmailVerification(beforeUpdatingEmail: email, completion: Self.unitCallback(promise))
                }
            }
            AsyncFunction("sendEmailVerification") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    user.sendEmailVerification(completion: Self.unitCallback(promise))
                }
            }
            AsyncFunction("deleteUser") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    user.delete(completion: Self.unitCallback(promise))
                }
            }
            AsyncFunction("linkWithCredential") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    do {
                        user.link(with: try Self.credential(args.count > 1 ? args[1] : nil), completion: Self.credentialCallback(promise))
                    } catch { promise.resolve(Self.failure("invalid-argument", error.localizedDescription)) }
                }
            }
            AsyncFunction("reauthenticateWithCredential") { (args: [WhiskerValue], promise: WhiskerPromise) in
                self.withUser(args, promise) { user in
                    do {
                        user.reauthenticate(with: try Self.credential(args.count > 1 ? args[1] : nil), completion: Self.credentialCallback(promise))
                    } catch { promise.resolve(Self.failure("invalid-argument", error.localizedDescription)) }
                }
            }
        }
    }

    private func withUser(_ args: [WhiskerValue], _ promise: WhiskerPromise, _ body: (User) -> Void) {
        guard let user = Auth.auth().currentUser else {
            promise.resolve(Self.failure("no-current-user", "No user is signed in")); return
        }
        guard args.first?.asString == user.uid else {
            promise.resolve(Self.failure("user-mismatch", "A different user is signed in")); return
        }
        body(user)
    }

    private func emit(_ id: Int64, _ user: User?) {
        let send = { [weak self] in
            guard let self else { return }
            self.lock.lock()
            let active = self.listeners[id] != nil
            self.lock.unlock()
            guard active else { return }
            self.sendEvent("authState", .map(["id": .int(id), "value": user.map(Self.encodeUser) ?? .null]))
        }
        if Thread.isMainThread { send() } else { DispatchQueue.main.async(execute: send) }
    }

    private static func remove(_ handle: Handle) {
        switch handle {
        case .auth(let handle): Auth.auth().removeStateDidChangeListener(handle)
        case .idToken(let handle): Auth.auth().removeIDTokenDidChangeListener(handle)
        }
    }

    private static func credential(_ wire: WhiskerValue?) throws -> AuthCredential {
        guard case .map(let fields) = wire, let provider = fields["provider"]?.asString else {
            throw NSError(domain: "WhiskerFirebaseAuth", code: 0, userInfo: [NSLocalizedDescriptionKey: "Missing credential"])
        }
        let idToken = fields["id_token"]?.asString
        let accessToken = fields["access_token"]?.asString
        let missing = NSError(domain: "WhiskerFirebaseAuth", code: 0,
                              userInfo: [NSLocalizedDescriptionKey: "Incomplete \(provider) credential"])
        switch provider {
        case "password":
            guard let email = fields["email"]?.asString, let password = fields["password"]?.asString else { throw missing }
            return EmailAuthProvider.credential(withEmail: email, password: password)
        case "google.com":
            guard let idToken, let accessToken else { throw missing }
            return GoogleAuthProvider.credential(withIDToken: idToken, accessToken: accessToken)
        case "apple.com":
            guard let idToken else { throw missing }
            return OAuthProvider.appleCredential(withIDToken: idToken, rawNonce: fields["raw_nonce"]?.asString, fullName: nil)
        default:
            guard let idToken else { throw missing }
            return OAuthProvider.credential(providerID: .custom(provider), idToken: idToken, accessToken: accessToken)
        }
    }

    // MARK: - Results

    private static func success(_ value: WhiskerValue) -> WhiskerValue { .map(["value": value]) }
    private static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
    private static func sdkFailure(_ error: Error) -> WhiskerValue {
        let error = error as NSError
        let name = error.userInfo["FIRAuthErrorUserInfoNameKey"] as? String ?? "ERROR_INTERNAL_ERROR"
        return failure(name, error.localizedDescription)
    }
    private static func unitCallback(_ promise: WhiskerPromise) -> (Error?) -> Void {
        { error in promise.resolve(error.map(sdkFailure) ?? success(.null)) }
    }
    private static func userResult(_ error: Error?) -> WhiskerValue {
        if let error { return sdkFailure(error) }
        guard let user = Auth.auth().currentUser else { return failure("no-current-user", "No user is signed in") }
        return success(encodeUser(user))
    }
    private static func credentialCallback(_ promise: WhiskerPromise) -> (AuthDataResult?, Error?) -> Void {
        { result, error in
            if let error { promise.resolve(sdkFailure(error)); return }
            guard let result else { promise.resolve(failure("internal-error", "Missing result")); return }
            var value: [String: WhiskerValue] = ["user": encodeUser(result.user)]
            if let info = result.additionalUserInfo {
                value["additional_user_info"] = .map([
                    "is_new_user": .bool(info.isNewUser),
                    "provider_id": .string(info.providerID),
                    "username": info.username.map { .string($0) } ?? .null,
                ])
            }
            promise.resolve(success(.map(value)))
        }
    }

    private static func optional(_ value: String?) -> WhiskerValue { value.map { .string($0) } ?? .null }
    private static func millis(_ date: Date?) -> WhiskerValue {
        date.map { .int(Int64(($0.timeIntervalSince1970 * 1000).rounded())) } ?? .null
    }

    private static func encodeUser(_ user: User) -> WhiskerValue {
        .map([
            "uid": .string(user.uid),
            "email": optional(user.email),
            "display_name": optional(user.displayName),
            "photo_url": optional(user.photoURL?.absoluteString),
            "phone_number": optional(user.phoneNumber),
            "email_verified": .bool(user.isEmailVerified),
            "is_anonymous": .bool(user.isAnonymous),
            "creation_time": millis(user.metadata.creationDate),
            "last_sign_in_time": millis(user.metadata.lastSignInDate),
            "provider_data": .array(user.providerData.map { info in
                .map([
                    "provider_id": .string(info.providerID),
                    "uid": .string(info.uid),
                    "email": optional(info.email),
                    "display_name": optional(info.displayName),
                    "photo_url": optional(info.photoURL?.absoluteString),
                    "phone_number": optional(info.phoneNumber),
                ])
            }),
        ])
    }
}
