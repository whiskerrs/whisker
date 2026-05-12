package dev.flint.runtime

import android.content.Context
import android.util.AttributeSet
import com.lynx.tasm.LynxView

/**
 * Hosts the Lynx engine and bridges it to the Rust runtime.
 *
 * Inherits from [LynxView] to reuse Lynx's Android SDK polish (Surface
 * management, vsync, lifecycle, touch dispatch, accessibility, IME).
 *
 * Instead of using LynxView's template-loading path, we obtain the engine
 * shell pointer and hand it to the Rust runtime via JNI. The Rust side
 * then drives the element tree directly via the bridge (Element PAPI).
 */
class FlintView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : LynxView(context, attrs) {

    private var nativeHandle: Long = 0

    init {
        nativeHandle = nativeAttachView(/* shellPtr = */ 0L /* TODO: extract from LynxView */)
    }

    fun onEnterForeground() {
        if (nativeHandle != 0L) nativeOnEnterForeground(nativeHandle)
    }

    fun onEnterBackground() {
        if (nativeHandle != 0L) nativeOnEnterBackground(nativeHandle)
    }

    fun destroy() {
        if (nativeHandle != 0L) {
            nativeDestroy(nativeHandle)
            nativeHandle = 0
        }
    }

    private external fun nativeAttachView(shellPtr: Long): Long
    private external fun nativeOnEnterForeground(handle: Long)
    private external fun nativeOnEnterBackground(handle: Long)
    private external fun nativeDestroy(handle: Long)
}
