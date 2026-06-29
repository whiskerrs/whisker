// Secure string store backed by Google Tink AES-256-GCM, with the Tink
// keyset envelope-encrypted by an Android Keystore master key
// (hardware-backed where available). Ciphertext is held in an
// app-private SharedPreferences. Persists across launches; never plain.
//
// Tink is Google's recommended replacement for the deprecated
// androidx.security EncryptedSharedPreferences — it owns the payload
// crypto (consistent across OEMs, upgradeable), while Keystore only
// protects the keyset-wrapping key.
//
// Plain helper — no Whisker / Lynx types. The DSL module that exposes
// it to Rust lives in `SecureStoreModule.kt`.

package rs.whisker.modules.securestore

import android.content.Context
import android.content.SharedPreferences
import android.util.Base64
import com.google.crypto.tink.Aead
import com.google.crypto.tink.KeyTemplates
import com.google.crypto.tink.aead.AeadConfig
import com.google.crypto.tink.integration.android.AndroidKeysetManager
import rs.whisker.runtime.WhiskerApplication

internal object SecureStore {
    /// App-private prefs file holding both the (wrapped) Tink keyset and
    /// the per-key ciphertext entries.
    private const val PREFS_NAME = "WhiskerSecureStore"

    /// Pref key under which Tink stores its (Keystore-wrapped) keyset.
    private const val KEYSET_NAME = "__whisker_secure_keyset__"

    /// Android Keystore alias for the master key that envelope-encrypts
    /// the Tink keyset. Tink creates it on first use.
    private const val MASTER_KEY_URI = "android-keystore://whisker_secure_store_master_key"

    @Volatile
    private var cachedAead: Aead? = null

    /**
     * Resolve the app context. Lazy via `WhiskerApplication.appContext`,
     * which the host app's `Application.onCreate()` installs before any
     * module dispatch could reach here.
     */
    private fun context(): Context =
        WhiskerApplication.appContext
            ?: throw IllegalStateException(
                "WhiskerSecureStore: WhiskerApplication.appContext not initialised — " +
                    "ensure your Application extends WhiskerApplication and " +
                    "super.onCreate() runs before any module dispatch",
            )

    private fun prefs(): SharedPreferences =
        context().getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    /**
     * The AEAD primitive, built once and cached. `AndroidKeysetManager`
     * loads the keyset if present or generates one on first run, wrapping
     * it with the Keystore master key. Double-checked locking keeps the
     * (mildly expensive, Keystore-touching) build single-shot and
     * thread-safe — Tink also requires keyset reads/writes to be
     * serialized.
     */
    private fun aead(): Aead {
        cachedAead?.let { return it }
        synchronized(this) {
            cachedAead?.let { return it }
            AeadConfig.register()
            val keysetHandle = AndroidKeysetManager.Builder()
                .withSharedPref(context(), KEYSET_NAME, PREFS_NAME)
                .withKeyTemplate(KeyTemplates.get("AES256_GCM"))
                .withMasterKeyUri(MASTER_KEY_URI)
                .build()
                .keysetHandle
            return keysetHandle.getPrimitive(Aead::class.java).also { cachedAead = it }
        }
    }

    /**
     * Encrypt [value] and persist it under [key]. The key bytes are the
     * AEAD associated data, binding each ciphertext to its key so an
     * entry can't be swapped under another key. Returns the
     * SharedPreferences commit result.
     */
    fun save(key: String, value: String): Boolean {
        val ciphertext = aead().encrypt(
            value.toByteArray(Charsets.UTF_8),
            key.toByteArray(Charsets.UTF_8),
        )
        val encoded = Base64.encodeToString(ciphertext, Base64.NO_WRAP)
        return prefs().edit().putString(key, encoded).commit()
    }

    /** Read + decrypt [key]; `null` on miss (→ `Option::None` in Rust). */
    fun load(key: String): String? {
        val encoded = prefs().getString(key, null) ?: return null
        val ciphertext = Base64.decode(encoded, Base64.NO_WRAP)
        val plaintext = aead().decrypt(ciphertext, key.toByteArray(Charsets.UTF_8))
        return String(plaintext, Charsets.UTF_8)
    }

    /** Drop [key]'s entry. */
    fun remove(key: String) {
        prefs().edit().remove(key).apply()
    }
}
