package rs.whisker.modules.securestore

import android.test.AndroidTestCase

/** Exercise the real store before any application or cryptographic initialization. */
class ReservedKeyTest : AndroidTestCase() {
    fun testReservedKeyCannotReadOverwriteOrDeleteKeyset() {
        val key = "__whisker_secure_keyset__"
        val prefs = context.getSharedPreferences("WhiskerSecureStore", 0)
        prefs.edit().putString(key, "existing wrapped keyset").commit()
        val operations: List<() -> Any?> = listOf(
            { SecureStore.load(key) },
            { SecureStore.save(key, "replacement") },
            { SecureStore.remove(key) },
        )
        for (operation in operations) {
            try {
                operation()
                fail("reserved key must be rejected before context or keyset access")
            } catch (expected: IllegalArgumentException) {
                assertEquals("reserved secure-store key", expected.message)
            }
            assertEquals("existing wrapped keyset", prefs.getString(key, null))
        }
        prefs.edit().clear().commit()
    }
}
