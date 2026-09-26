package rs.whisker.modules.firebasemessaging

import android.Manifest
import android.app.Activity
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.util.Consumer
import com.google.android.gms.tasks.Task
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.RemoteMessage
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.RuntimeAttachedListener
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerPromise
import rs.whisker.runtime.WhiskerValue
import java.util.concurrent.atomic.AtomicInteger

@WhiskerModule
class FirebaseMessagingModule : Module() {
    private val main = Handler(Looper.getMainLooper())
    private val requests = AtomicInteger()
    private val consumedMessageIds = mutableSetOf<String>()
    private var intentActivity: ComponentActivity? = null
    private val newIntentListener = Consumer<Intent> { intent ->
        messageFrom(intent)?.let { sendEvent("messageOpened", WhiskerValue.Map(mapOf("value" to it))) }
    }
    private val attachListener = RuntimeAttachedListener { attachIntentListener() }

    init {
        MessagingEvents.sink = { event, value -> sendEvent(event, WhiskerValue.Map(mapOf("value" to value))) }
    }

    override fun definition() = ModuleDefinition {
        Name("FirebaseMessaging")
        Events("message", "messageOpened", "token")
        OnStartObserving("messageOpened") { appContext.addOnRuntimeAttachedListener(attachListener) }
        OnStopObserving("messageOpened") {
            appContext.removeOnRuntimeAttachedListener(attachListener)
            main.post {
                intentActivity?.removeOnNewIntentListener(newIntentListener)
                intentActivity = null
            }
        }
        AsyncFunction("requestPermission") { _, promise ->
            main.post { requestPermission(promise) }
        }
        AsyncFunction("getNotificationSettings") { _, promise -> promise.resolve(success(settings())) }
        AsyncFunction("getToken") { _, promise ->
            resolve(promise, { WhiskerValue.Str(it) }) { FirebaseMessaging.getInstance().token }
        }
        AsyncFunction("deleteToken") { _, promise ->
            resolve(promise, { WhiskerValue.Null }) { FirebaseMessaging.getInstance().deleteToken() }
        }
        Function("getApnsToken") { _ -> success(WhiskerValue.Null) }
        AsyncFunction("subscribeToTopic") { args, promise ->
            resolve(promise, { WhiskerValue.Null }) {
                FirebaseMessaging.getInstance().subscribeToTopic(requireNotNull(args.firstOrNull()?.asString()))
            }
        }
        AsyncFunction("unsubscribeFromTopic") { args, promise ->
            resolve(promise, { WhiskerValue.Null }) {
                FirebaseMessaging.getInstance().unsubscribeFromTopic(requireNotNull(args.firstOrNull()?.asString()))
            }
        }
        Function("isAutoInitEnabled") { _ ->
            try { success(WhiskerValue.Bool(FirebaseMessaging.getInstance().isAutoInitEnabled)) } catch (error: Exception) { failure(error) }
        }
        Function("setAutoInitEnabled") { args ->
            try {
                FirebaseMessaging.getInstance().isAutoInitEnabled = requireNotNull(args.firstOrNull()?.asBool())
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        Function("setForegroundPresentation") { _ -> success(WhiskerValue.Null) }
        AsyncFunction("getInitialMessage") { _, promise ->
            main.post {
                val message = appContext.currentActivity?.intent?.let(::messageFrom)
                promise.resolve(success(message ?: WhiskerValue.Null))
            }
        }
    }

    private val context: Context get() = FirebaseApp.getInstance().applicationContext

    private fun attachIntentListener() {
        main.post {
            val activity = appContext.currentActivity as? ComponentActivity ?: return@post
            if (activity === intentActivity) return@post
            intentActivity?.removeOnNewIntentListener(newIntentListener)
            activity.addOnNewIntentListener(newIntentListener)
            intentActivity = activity
        }
    }

    /** A notification tap launches the activity with the message fields as extras. */
    private fun messageFrom(intent: Intent): WhiskerValue? {
        val extras = intent.extras ?: return null
        val id = extras.getString("google.message_id") ?: extras.getString("message_id") ?: return null
        synchronized(consumedMessageIds) { if (!consumedMessageIds.add(id)) return null }
        return encodeMessage(RemoteMessage(extras))
    }

    private fun settings(): WhiskerValue {
        val granted = Build.VERSION.SDK_INT < 33 ||
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        val enabled = context.getSystemService(NotificationManager::class.java)?.areNotificationsEnabled() == true
        val status = if (granted && enabled) "authorized" else "denied"
        return WhiskerValue.Map(mapOf("authorization_status" to WhiskerValue.Str(status)))
    }

    private fun requestPermission(promise: WhiskerPromise) {
        if (Build.VERSION.SDK_INT < 33 ||
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        ) {
            promise.resolve(success(settings()))
            return
        }
        val activity: Activity? = appContext.currentActivity
        if (activity !is ComponentActivity) {
            promise.resolve(failure("no-activity", "Notification permission needs a visible Whisker activity"))
            return
        }
        var launcher: androidx.activity.result.ActivityResultLauncher<String>? = null
        launcher = activity.activityResultRegistry.register(
            "whisker-firebase-messaging-permission-${requests.incrementAndGet()}",
            ActivityResultContracts.RequestPermission(),
        ) {
            launcher?.unregister()
            promise.resolve(success(settings()))
        }
        launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    private fun <T> resolve(promise: WhiskerPromise, encode: (T) -> WhiskerValue, start: () -> Task<T>) {
        try {
            start().addOnCompleteListener { task ->
                promise.resolve(if (task.isSuccessful) success(encode(task.result)) else failure(task.exception))
            }
        } catch (error: Exception) { promise.resolve(failure(error)) }
    }

    private fun success(value: WhiskerValue) = WhiskerValue.Map(mapOf("value" to value))
    private fun failure(code: String, message: String) = WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
        "code" to WhiskerValue.Str(code), "message" to WhiskerValue.Str(message),
    ))))

    /** FCM reports failures as IOExceptions whose message is a code such as SERVICE_NOT_AVAILABLE. */
    private fun failure(error: Exception?): WhiskerValue {
        val message = error?.message ?: "Messaging operation failed"
        val cause = generateSequence(error as Throwable?) { it.cause }.mapNotNull { it.message }
            .firstOrNull { it.matches(Regex("[A-Z_]+")) }
        val code = when {
            cause != null -> cause.lowercase(java.util.Locale.ROOT).replace('_', '-')
            error is IllegalArgumentException -> "invalid-argument"
            error is IllegalStateException -> "app-not-configured"
            else -> "unknown"
        }
        return failure(code, message)
    }
}
