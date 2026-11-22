package rs.whisker.modules.firebaseauth

import android.net.Uri
import android.os.Handler
import android.os.Looper
import com.google.android.gms.tasks.Task
import com.google.firebase.FirebaseException
import com.google.firebase.FirebaseNetworkException
import com.google.firebase.FirebaseTooManyRequestsException
import com.google.firebase.auth.AuthCredential
import com.google.firebase.auth.AuthResult
import com.google.firebase.auth.EmailAuthProvider
import com.google.firebase.auth.FirebaseAuth
import com.google.firebase.auth.FirebaseAuthException
import com.google.firebase.auth.FirebaseUser
import com.google.firebase.auth.GoogleAuthProvider
import com.google.firebase.auth.OAuthProvider
import com.google.firebase.auth.PhoneAuthCredential
import com.google.firebase.auth.PhoneAuthOptions
import com.google.firebase.auth.PhoneAuthProvider
import com.google.firebase.auth.UserProfileChangeRequest
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerPromise
import rs.whisker.runtime.WhiskerValue
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

@WhiskerModule
class FirebaseAuthModule : Module() {
    private val authListeners = ConcurrentHashMap<Long, FirebaseAuth.AuthStateListener>()
    private val tokenListeners = ConcurrentHashMap<Long, FirebaseAuth.IdTokenListener>()
    private val verifiedPhones = ConcurrentHashMap<String, PhoneAuthCredential>()
    private val resendTokens = ConcurrentHashMap<String, PhoneAuthProvider.ForceResendingToken>()
    private val mainHandler = Handler(Looper.getMainLooper())

    private val auth get() = FirebaseAuth.getInstance()

    override fun definition() = ModuleDefinition {
        Name("FirebaseAuth")
        Events("authState")
        Function("useEmulator") { args ->
            try {
                val host = requireNotNull(args.getOrNull(0)?.asString())
                val port = requireNotNull(args.getOrNull(1)?.asInt()).toInt()
                require(port in 1..65535)
                auth.useEmulator(host, port)
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        Function("currentUser") { _ -> success(auth.currentUser?.let(::encodeUser) ?: WhiskerValue.Null) }
        AsyncFunction("signInAnonymously") { _, promise -> resolveCredential(promise) { auth.signInAnonymously() } }
        AsyncFunction("signInWithEmailAndPassword") { args, promise ->
            resolveCredential(promise) { auth.signInWithEmailAndPassword(string(args, 0), string(args, 1)) }
        }
        AsyncFunction("createUserWithEmailAndPassword") { args, promise ->
            resolveCredential(promise) { auth.createUserWithEmailAndPassword(string(args, 0), string(args, 1)) }
        }
        AsyncFunction("signInWithCustomToken") { args, promise ->
            resolveCredential(promise) { auth.signInWithCustomToken(string(args, 0)) }
        }
        AsyncFunction("signInWithCredential") { args, promise ->
            resolveCredential(promise) { auth.signInWithCredential(credential(args.getOrNull(0))) }
        }
        AsyncFunction("verifyPhoneNumber") { args, promise ->
            val settled = AtomicBoolean(false)
            fun settle(value: WhiskerValue) = settled.compareAndSet(false, true).also { if (it) promise.resolve(value) }
            try {
                val phoneNumber = string(args, 0)
                val resendHandle = args.getOrNull(1)?.asString()
                val activity = appContext.currentActivity
                val resendToken = resendHandle?.let { resendTokens[it] }
                if (activity == null) {
                    settle(failure("missing-activity", "Phone verification needs a visible activity"))
                } else if (resendHandle != null && resendToken == null) {
                    settle(failure("invalid-resend-token", "The resend token is unknown or was replaced by a newer code"))
                } else {
                    val callbacks = object : PhoneAuthProvider.OnVerificationStateChangedCallbacks() {
                        override fun onCodeSent(verificationId: String, token: PhoneAuthProvider.ForceResendingToken) {
                            val handle = UUID.randomUUID().toString()
                            val older = resendTokens.keys.toList()
                            resendTokens[handle] = token
                            val sent = settle(
                                success(
                                    WhiskerValue.Map(
                                        mapOf(
                                            "verification_id" to WhiskerValue.Str(verificationId),
                                            "resend" to WhiskerValue.Str(handle),
                                        ),
                                    ),
                                ),
                            )
                            // Only the latest token stays usable, so a failed resend keeps its token.
                            if (sent) older.forEach(resendTokens::remove) else resendTokens.remove(handle)
                        }

                        override fun onVerificationCompleted(credential: PhoneAuthCredential) {
                            val handle = UUID.randomUUID().toString()
                            val older = verifiedPhones.keys.toList()
                            verifiedPhones[handle] = credential
                            if (settle(success(WhiskerValue.Map(mapOf("credential" to WhiskerValue.Str(handle)))))) {
                                older.forEach(verifiedPhones::remove)
                            } else {
                                verifiedPhones.remove(handle)
                            }
                        }

                        override fun onVerificationFailed(error: FirebaseException) {
                            settle(failure(error))
                        }
                    }
                    val options = PhoneAuthOptions.newBuilder(auth)
                        .setPhoneNumber(phoneNumber)
                        .setTimeout(60L, TimeUnit.SECONDS)
                        .setActivity(activity)
                        .setCallbacks(callbacks)
                    resendToken?.let(options::setForceResendingToken)
                    PhoneAuthProvider.verifyPhoneNumber(options.build())
                }
            } catch (error: Exception) { settle(failure(error)) }
        }
        Function("setAppVerificationDisabledForTesting") { args ->
            try {
                auth.firebaseAuthSettings.setAppVerificationDisabledForTesting(requireNotNull(args.getOrNull(0)?.asBool()))
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        AsyncFunction("sendPasswordResetEmail") { args, promise ->
            resolveUnit(promise) { auth.sendPasswordResetEmail(string(args, 0)) }
        }
        Function("signOut") { _ ->
            auth.signOut()
            success(WhiskerValue.Null)
        }
        Function("addListener") { args ->
            try {
                val id = requireNotNull(args.getOrNull(0)?.asInt())
                if (args.getOrNull(1)?.asString() == "id_token") {
                    val listener = FirebaseAuth.IdTokenListener { emit(id, it.currentUser) }
                    tokenListeners[id] = listener
                    auth.addIdTokenListener(listener)
                } else {
                    val listener = FirebaseAuth.AuthStateListener { emit(id, it.currentUser) }
                    authListeners[id] = listener
                    auth.addAuthStateListener(listener)
                }
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        Function("removeListener") { args ->
            args.firstOrNull()?.asInt()?.let(::remove)
            success(WhiskerValue.Null)
        }
        Function("removeAllListeners") { _ ->
            (authListeners.keys + tokenListeners.keys).forEach(::remove)
            success(WhiskerValue.Null)
        }
        AsyncFunction("getIdToken") { args, promise ->
            withUser(args, promise) { user ->
                user.getIdToken(args.getOrNull(1)?.asBool() == true).addOnCompleteListener { task ->
                    promise.resolve(
                        if (task.isSuccessful) success(task.result.token?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null)
                        else failure(task.exception),
                    )
                }
            }
        }
        AsyncFunction("reload") { args, promise ->
            withUser(args, promise) { user -> resolveUser(promise) { user.reload() } }
        }
        AsyncFunction("updateProfile") { args, promise ->
            withUser(args, promise) { user ->
                val fields = requireNotNull(args.getOrNull(1) as? WhiskerValue.Map).value
                val request = UserProfileChangeRequest.Builder().apply {
                    if ("display_name" in fields) setDisplayName(fields["display_name"]?.asString())
                    if ("photo_url" in fields) setPhotoUri(fields["photo_url"]?.asString()?.let(Uri::parse))
                }.build()
                resolveUser(promise) { user.updateProfile(request) }
            }
        }
        AsyncFunction("updatePassword") { args, promise ->
            withUser(args, promise) { user -> resolveUnit(promise) { user.updatePassword(string(args, 1)) } }
        }
        AsyncFunction("verifyBeforeUpdateEmail") { args, promise ->
            withUser(args, promise) { user -> resolveUnit(promise) { user.verifyBeforeUpdateEmail(string(args, 1)) } }
        }
        AsyncFunction("sendEmailVerification") { args, promise ->
            withUser(args, promise) { user -> resolveUnit(promise) { user.sendEmailVerification() } }
        }
        AsyncFunction("deleteUser") { args, promise ->
            withUser(args, promise) { user -> resolveUnit(promise) { user.delete() } }
        }
        AsyncFunction("linkWithCredential") { args, promise ->
            withUser(args, promise) { user -> resolveCredential(promise) { user.linkWithCredential(credential(args.getOrNull(1))) } }
        }
        AsyncFunction("reauthenticateWithCredential") { args, promise ->
            withUser(args, promise) { user ->
                resolveCredential(promise) { user.reauthenticateAndRetrieveData(credential(args.getOrNull(1))) }
            }
        }
    }

    private fun remove(id: Long) {
        authListeners.remove(id)?.let { auth.removeAuthStateListener(it) }
        tokenListeners.remove(id)?.let { auth.removeIdTokenListener(it) }
    }

    private fun emit(id: Long, user: FirebaseUser?) {
        val send = Runnable {
            if (!authListeners.containsKey(id) && !tokenListeners.containsKey(id)) return@Runnable
            sendEvent("authState", WhiskerValue.Map(mapOf(
                "id" to WhiskerValue.Int(id),
                "value" to (user?.let(::encodeUser) ?: WhiskerValue.Null),
            )))
        }
        if (Looper.myLooper() == Looper.getMainLooper()) send.run() else mainHandler.post(send)
    }

    private fun withUser(args: List<WhiskerValue>, promise: WhiskerPromise, body: (FirebaseUser) -> Unit) {
        val user = auth.currentUser
        when {
            user == null -> promise.resolve(failure("no-current-user", "No user is signed in"))
            args.firstOrNull()?.asString() != user.uid -> promise.resolve(failure("user-mismatch", "A different user is signed in"))
            else -> try { body(user) } catch (error: Exception) { promise.resolve(failure(error)) }
        }
    }

    private fun string(args: List<WhiskerValue>, index: Int) =
        requireNotNull(args.getOrNull(index)?.asString()) { "Missing argument ${index + 1}" }

    private fun credential(wire: WhiskerValue?): AuthCredential {
        val fields = requireNotNull(wire as? WhiskerValue.Map) { "Missing credential" }.value
        val provider = requireNotNull(fields["provider"]?.asString())
        val idToken = fields["id_token"]?.asString()
        val accessToken = fields["access_token"]?.asString()
        return when (provider) {
            "password" -> EmailAuthProvider.getCredential(
                requireNotNull(fields["email"]?.asString()), requireNotNull(fields["password"]?.asString()),
            )
            "google.com" -> GoogleAuthProvider.getCredential(idToken, accessToken)
            "phone" -> fields["handle"]?.asString()?.let { handle ->
                requireNotNull(verifiedPhones.remove(handle)) { "Unknown or already used phone credential" }
            } ?: PhoneAuthProvider.getCredential(
                requireNotNull(fields["verification_id"]?.asString()), requireNotNull(fields["sms_code"]?.asString()),
            )
            else -> OAuthProvider.newCredentialBuilder(provider).apply {
                val rawNonce = fields["raw_nonce"]?.asString()
                if (idToken != null && rawNonce != null) setIdTokenWithRawNonce(idToken, rawNonce)
                else if (idToken != null) setIdToken(idToken)
                if (accessToken != null) setAccessToken(accessToken)
            }.build()
        }
    }

    private fun resolveUnit(promise: WhiskerPromise, start: () -> Task<Void>) {
        try {
            start().addOnCompleteListener { promise.resolve(if (it.isSuccessful) success(WhiskerValue.Null) else failure(it.exception)) }
        } catch (error: Exception) { promise.resolve(failure(error)) }
    }

    private fun resolveUser(promise: WhiskerPromise, start: () -> Task<Void>) {
        try {
            start().addOnCompleteListener { task ->
                val user = auth.currentUser
                promise.resolve(when {
                    !task.isSuccessful -> failure(task.exception)
                    user == null -> failure("no-current-user", "No user is signed in")
                    else -> success(encodeUser(user))
                })
            }
        } catch (error: Exception) { promise.resolve(failure(error)) }
    }

    private fun resolveCredential(promise: WhiskerPromise, start: () -> Task<AuthResult>) {
        try {
            start().addOnCompleteListener { task ->
                if (!task.isSuccessful) { promise.resolve(failure(task.exception)); return@addOnCompleteListener }
                val result = task.result
                val user = result.user ?: return@addOnCompleteListener promise.resolve(failure("internal-error", "Missing user"))
                val value = mutableMapOf<String, WhiskerValue>("user" to encodeUser(user))
                result.additionalUserInfo?.let { info ->
                    value["additional_user_info"] = WhiskerValue.Map(mapOf(
                        "is_new_user" to WhiskerValue.Bool(info.isNewUser),
                        "provider_id" to optional(info.providerId),
                        "username" to optional(info.username),
                    ))
                }
                promise.resolve(success(WhiskerValue.Map(value)))
            }
        } catch (error: Exception) { promise.resolve(failure(error)) }
    }

    private fun success(value: WhiskerValue) = WhiskerValue.Map(mapOf("value" to value))
    private fun failure(code: String, message: String) = WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
        "code" to WhiskerValue.Str(code), "message" to WhiskerValue.Str(message),
    ))))
    private fun failure(error: Exception?): WhiskerValue {
        val code = when (error) {
            is FirebaseAuthException -> error.errorCode
            is FirebaseNetworkException -> "network-request-failed"
            is FirebaseTooManyRequestsException -> "too-many-requests"
            is IllegalArgumentException -> "invalid-argument"
            else -> "internal-error"
        }
        return failure(code, error?.message ?: "Authentication failed")
    }

    private fun optional(value: String?) = value?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null

    private fun encodeUser(user: FirebaseUser) = WhiskerValue.Map(mapOf(
        "uid" to WhiskerValue.Str(user.uid),
        "email" to optional(user.email),
        "display_name" to optional(user.displayName),
        "photo_url" to optional(user.photoUrl?.toString()),
        "phone_number" to optional(user.phoneNumber),
        "email_verified" to WhiskerValue.Bool(user.isEmailVerified),
        "is_anonymous" to WhiskerValue.Bool(user.isAnonymous),
        "creation_time" to (user.metadata?.creationTimestamp?.let { WhiskerValue.Int(it) } ?: WhiskerValue.Null),
        "last_sign_in_time" to (user.metadata?.lastSignInTimestamp?.let { WhiskerValue.Int(it) } ?: WhiskerValue.Null),
        "provider_data" to WhiskerValue.Array(user.providerData.map { info ->
            WhiskerValue.Map(mapOf(
                "provider_id" to WhiskerValue.Str(info.providerId),
                // The phone provider's UserInfo has a null uid on Android, although the SDK
                // annotates it as non-null; iOS and the web use the phone number there.
                "uid" to WhiskerValue.Str((info.uid as String?) ?: info.phoneNumber.orEmpty()),
                "email" to optional(info.email),
                "display_name" to optional(info.displayName),
                "photo_url" to optional(info.photoUrl?.toString()),
                "phone_number" to optional(info.phoneNumber),
            ))
        }),
    ))
}
