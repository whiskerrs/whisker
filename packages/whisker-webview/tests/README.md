# Native bridge security regressions

`BridgeOriginsTest.kt` and `BridgeOriginsTest.swift` run against the pure origin helpers. Compile each helper and its test together for the corresponding platform.

`BridgeSecurityTest.kt` is an Android instrumentation test. Include the module's Android sources in an Android library test host, depend on the local Whisker runtime SDK and the module's declared AndroidX WebKit version, and include this file in `androidTest`. It uses the platform `android.test.InstrumentationTestRunner`, with the `android.test.runner` and `android.test.base` SDK libraries. No additional test dependency is needed.

`BridgeSecurityTests.swift` is an iOS XCTest. Build the module's iOS sources as `whisker_webview` with the local `WhiskerModule` SDK, and include this file in an iOS test target that depends on both modules. Run it on an iOS Simulator.

Both device tests exercise the production WebView: an allowed main frame can deliver a message, a same-origin iframe cannot, and a different top-level origin cannot. The iOS iframe calls the native WebKit handler directly, so the test also covers bypassing the main-frame-only JavaScript wrapper.
