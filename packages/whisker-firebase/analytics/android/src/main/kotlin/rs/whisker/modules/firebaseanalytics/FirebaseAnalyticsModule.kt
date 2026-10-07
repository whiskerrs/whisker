package rs.whisker.modules.firebaseanalytics

import android.os.Bundle
import com.google.firebase.FirebaseApp
import com.google.firebase.analytics.FirebaseAnalytics
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerValue

@WhiskerModule
class FirebaseAnalyticsModule : Module() {
    private val analytics get() = FirebaseAnalytics.getInstance(FirebaseApp.getInstance().applicationContext)

    override fun definition() = ModuleDefinition {
        Name("FirebaseAnalytics")
        Function("logEvent") { args ->
            attempt {
                val params = requireNotNull(args.getOrNull(1) as? WhiskerValue.Map).value
                analytics.logEvent(requireNotNull(args.firstOrNull()?.asString()), bundle(params))
            }
        }
        Function("setUserId") { args -> attempt { analytics.setUserId(args.firstOrNull()?.asString()) } }
        Function("setUserProperty") { args ->
            attempt { analytics.setUserProperty(requireNotNull(args.firstOrNull()?.asString()), args.getOrNull(1)?.asString()) }
        }
        Function("setAnalyticsCollectionEnabled") { args ->
            attempt { analytics.setAnalyticsCollectionEnabled(requireNotNull(args.firstOrNull()?.asBool())) }
        }
        Function("setDefaultEventParameters") { args ->
            attempt {
                val params = requireNotNull(args.firstOrNull() as? WhiskerValue.Map).value
                analytics.setDefaultEventParameters(if (params.isEmpty()) null else bundle(params))
            }
        }
        Function("resetAnalyticsData") { _ -> attempt { analytics.resetAnalyticsData() } }
        AsyncFunction("getAppInstanceId") { _, promise ->
            try {
                analytics.appInstanceId.addOnCompleteListener { task ->
                    promise.resolve(
                        if (task.isSuccessful) success(task.result?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null)
                        else failure(task.exception),
                    )
                }
            } catch (error: Exception) { promise.resolve(failure(error)) }
        }
        Function("setConsent") { args ->
            attempt {
                val fields = requireNotNull(args.firstOrNull() as? WhiskerValue.Map).value
                val types = mapOf(
                    "analytics_storage" to FirebaseAnalytics.ConsentType.ANALYTICS_STORAGE,
                    "ad_storage" to FirebaseAnalytics.ConsentType.AD_STORAGE,
                    "ad_user_data" to FirebaseAnalytics.ConsentType.AD_USER_DATA,
                    "ad_personalization" to FirebaseAnalytics.ConsentType.AD_PERSONALIZATION,
                )
                analytics.setConsent(fields.entries.mapNotNull { (key, value) ->
                    val type = types[key] ?: return@mapNotNull null
                    val granted = value.asBool() ?: return@mapNotNull null
                    type to if (granted) FirebaseAnalytics.ConsentStatus.GRANTED else FirebaseAnalytics.ConsentStatus.DENIED
                }.toMap())
            }
        }
        Function("setSessionTimeout") { args ->
            attempt { analytics.setSessionTimeoutDuration(requireNotNull(args.firstOrNull()?.asInt())) }
        }
    }

    private fun attempt(body: () -> Unit): WhiskerValue =
        try { body(); success(WhiskerValue.Null) } catch (error: Exception) { failure(error) }

    private fun bundle(params: Map<String, WhiskerValue>): Bundle = Bundle().apply {
        for ((key, value) in params) {
            when (value) {
                is WhiskerValue.Str -> putString(key, value.value)
                is WhiskerValue.Int -> putLong(key, value.value)
                is WhiskerValue.Float -> putDouble(key, value.value)
                is WhiskerValue.Array -> putParcelableArray(
                    key,
                    value.value.mapNotNull { (it as? WhiskerValue.Map)?.value?.let(::bundle) }.toTypedArray(),
                )
                else -> {}
            }
        }
    }

    private fun success(value: WhiskerValue) = WhiskerValue.Map(mapOf("value" to value))
    private fun failure(error: Exception?): WhiskerValue {
        val code = when (error) {
            is IllegalArgumentException -> "invalid-argument"
            is IllegalStateException -> "app-not-configured"
            else -> "unknown"
        }
        return WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
            "code" to WhiskerValue.Str(code),
            "message" to WhiskerValue.Str(error?.message ?: "Analytics operation failed"),
        ))))
    }
}
