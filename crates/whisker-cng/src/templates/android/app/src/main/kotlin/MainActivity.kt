package {{android_application_id}}

import rs.whisker.runtime.WhiskerActivity

/**
 * Host Activity for the Whisker app.
 *
 * Empty by design — [WhiskerActivity] handles WhiskerView instantiation,
 * lifecycle forwarding, edge-to-edge window configuration, and system-bar
 * styling. Override `onCreate(savedInstanceState)` here only if the app
 * needs to wire something **before** the WhiskerView is set as the
 * content view (e.g. installing a SplashScreen). Always call
 * `super.onCreate(savedInstanceState)` last in that case.
 */
class MainActivity : WhiskerActivity()
