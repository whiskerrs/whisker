# Android reserved-key regression

Include `ReservedKeyTest.kt` in the module's `androidTest` source set and run it with the platform `android.test.InstrumentationTestRunner`. The host needs the `android.test.runner` and `android.test.base` SDK libraries and the module's existing Whisker SDK/Tink dependencies; no extra test dependency is required.

The test uses the real `SecureStore`, before application or cryptographic initialization. Reading, overwriting and removing the reserved key must all throw `IllegalArgumentException`, and the existing keyset preference must remain unchanged. The previous implementation instead reaches application-context resolution and fails the test.
