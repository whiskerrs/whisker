// `whisker-web-browser` Module (Android).
//
// Mirrors Expo's expo-web-browser: `openBrowser`/`openAuthSession`
// via Chrome Custom Tabs. Unlike iOS's ASWebAuthenticationSession,
// Custom Tabs has no built-in "session ended" callback, so:
//
//  * success — the OAuth redirect lands back in the app as a new
//    Intent, forwarded via `WhiskerAppContext.dispatchDeepLink`.
//    Requires `Config::url_scheme` so the redirect's scheme matches
//    a registered intent-filter.
//  * cancel — detected via `Application.ActivityLifecycleCallbacks`:
//    if the launching Activity resumes while a redirect is still
//    pending (i.e. `onNewIntent` never fired), the user dismissed
//    the Custom Tab without completing.

package rs.whisker.modules.webbrowser

import android.app.Activity
import android.app.Application
import android.net.Uri
import android.os.Bundle
import androidx.browser.customtabs.CustomTabsIntent
import rs.whisker.runtime.DeepLinkListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerAppContext
import rs.whisker.runtime.WhiskerValue

class WebBrowserModule : Module() {
    private var pendingRedirectPrefix: String? = null
    private var deepLinkListener: DeepLinkListener? = null
    private var lifecycleCallbacks: Application.ActivityLifecycleCallbacks? = null
    private var launchingActivity: Activity? = null

    override fun definition() = ModuleDefinition {
        Name("WebBrowser")
        Events("authSessionCompleted", "browserClosed")

        Function("openAuthSession") { args ->
            val url = args.getOrNull(0)?.asString()
            val redirectUrl = args.getOrNull(1)?.asString()
            val activity = appContext.currentActivity
            if (url != null && redirectUrl != null && activity != null) {
                startAuthSession(activity, url, redirectUrl)
            }
            WhiskerValue.Null
        }

        Function("dismissAuthSession") {
            resolveAuthSession(WhiskerValue.Map(mapOf("type" to WhiskerValue.Str("cancel"))))
            WhiskerValue.Null
        }

        Function("openBrowser") { args ->
            val url = args.getOrNull(0)?.asString()
            val activity = appContext.currentActivity
            if (url != null && activity != null) {
                CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(url))
            }
            WhiskerValue.Null
        }

        Function("dismissBrowser") {
            sendEvent("browserClosed", WhiskerValue.Map(mapOf("type" to WhiskerValue.Str("dismiss"))))
            WhiskerValue.Null
        }
    }

    private fun startAuthSession(activity: Activity, url: String, redirectUrl: String) {
        clearPending()
        pendingRedirectPrefix = redirectUrl
        launchingActivity = activity

        val listener = DeepLinkListener { receivedUrl ->
            if (pendingRedirectPrefix != null && receivedUrl.startsWith(pendingRedirectPrefix!!)) {
                resolveAuthSession(
                    WhiskerValue.Map(
                        mapOf("type" to WhiskerValue.Str("success"), "url" to WhiskerValue.Str(receivedUrl))
                    )
                )
            }
        }
        deepLinkListener = listener
        WhiskerAppContext.addDeepLinkListener(listener)

        val callbacks = object : Application.ActivityLifecycleCallbacks {
            override fun onActivityResumed(resumed: Activity) {
                if (resumed !== launchingActivity) return
                if (pendingRedirectPrefix != null) {
                    resolveAuthSession(WhiskerValue.Map(mapOf("type" to WhiskerValue.Str("cancel"))))
                }
            }
            override fun onActivityCreated(a: Activity, b: Bundle?) {}
            override fun onActivityStarted(a: Activity) {}
            override fun onActivityPaused(a: Activity) {}
            override fun onActivityStopped(a: Activity) {}
            override fun onActivitySaveInstanceState(a: Activity, b: Bundle) {}
            override fun onActivityDestroyed(a: Activity) {}
        }
        lifecycleCallbacks = callbacks
        activity.application.registerActivityLifecycleCallbacks(callbacks)

        CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(url))
    }

    private fun resolveAuthSession(result: WhiskerValue) {
        if (pendingRedirectPrefix == null) return
        clearPending()
        sendEvent("authSessionCompleted", result)
    }

    private fun clearPending() {
        pendingRedirectPrefix = null
        deepLinkListener?.let { WhiskerAppContext.removeDeepLinkListener(it) }
        deepLinkListener = null
        lifecycleCallbacks?.let { launchingActivity?.application?.unregisterActivityLifecycleCallbacks(it) }
        lifecycleCallbacks = null
        launchingActivity = null
    }
}
