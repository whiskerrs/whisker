package {{android_application_id}}

import rs.whisker.runtime.WhiskerActivity{{main_activity_imports}}

/**
 * Host Activity for the Whisker app.
 *
 * Empty by default — [WhiskerActivity] handles WhiskerView instantiation,
 * lifecycle forwarding, edge-to-edge window configuration, and system-bar
 * styling. A plugin may inject an `onCreate` override (e.g. to install a
 * SplashScreen before `super.onCreate`) via the manifest IR's
 * `main_activity_pre_super` / `main_activity_post_super`.
 */
class MainActivity : WhiskerActivity(){{main_activity_body}}
