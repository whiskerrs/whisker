package rs.whisker.modules.firebasemessaging

import android.os.Handler
import android.os.Looper
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import rs.whisker.runtime.WhiskerValue

/** Forwards FCM callbacks to the Firebase Messaging module while the Whisker runtime is running. */
class WhiskerMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        MessagingEvents.deliver("message", encodeMessage(message))
    }

    override fun onNewToken(token: String) {
        MessagingEvents.deliver("token", WhiskerValue.Str(token))
    }
}

internal object MessagingEvents {
    // Messages that arrive before the module is installed (e.g. in a service-only
    // process) are dropped; the system still displays notification messages.
    @Volatile var sink: ((String, WhiskerValue) -> Unit)? = null
    private val main = Handler(Looper.getMainLooper())

    fun deliver(event: String, value: WhiskerValue) {
        main.post { sink?.invoke(event, value) }
    }
}

internal fun encodeMessage(message: RemoteMessage): WhiskerValue {
    val fields = mutableMapOf<String, WhiskerValue>(
        "data" to WhiskerValue.Map(message.data.mapValues { WhiskerValue.Str(it.value) }),
        "sent_time" to WhiskerValue.Int(message.sentTime),
    )
    message.messageId?.let { fields["message_id"] = WhiskerValue.Str(it) }
    message.from?.let { fields["from"] = WhiskerValue.Str(it) }
    message.collapseKey?.let { fields["collapse_key"] = WhiskerValue.Str(it) }
    message.notification?.let { notification ->
        val parts = mutableMapOf<String, WhiskerValue>()
        notification.title?.let { parts["title"] = WhiskerValue.Str(it) }
        notification.body?.let { parts["body"] = WhiskerValue.Str(it) }
        notification.imageUrl?.let { parts["image_url"] = WhiskerValue.Str(it.toString()) }
        fields["notification"] = WhiskerValue.Map(parts)
    }
    return WhiskerValue.Map(fields)
}
