// jni_flint.cc
//
// JNI shim. Translates Kotlin -> Rust runtime calls. The actual Rust
// runtime exports its functions via `extern "C"` in the cdylib that ships
// alongside this library; we declare them here and call them.
//
// Naming convention follows JNI: Java_<package>_<class>_<method>.

#include <jni.h>

extern "C" {

// Rust runtime exports (defined in lux_runtime cdylib).
// These are stubs for now; real signatures will land with the runtime.
void flint_runtime_init();
long flint_runtime_attach_view(void* lynx_shell_ptr);
void flint_runtime_on_enter_foreground(long handle);
void flint_runtime_on_enter_background(long handle);
void flint_runtime_destroy(long handle);

// dev/flint/runtime/FlintApplication.nativeInitRuntime
JNIEXPORT void JNICALL
Java_dev_flint_runtime_FlintApplication_nativeInitRuntime(JNIEnv* /*env*/, jobject /*self*/) {
    flint_runtime_init();
}

// dev/flint/runtime/FlintView.nativeAttachView
JNIEXPORT jlong JNICALL
Java_dev_flint_runtime_FlintView_nativeAttachView(JNIEnv* /*env*/, jobject /*self*/, jlong shellPtr) {
    return static_cast<jlong>(
        flint_runtime_attach_view(reinterpret_cast<void*>(shellPtr)));
}

JNIEXPORT void JNICALL
Java_dev_flint_runtime_FlintView_nativeOnEnterForeground(JNIEnv* /*env*/, jobject /*self*/, jlong handle) {
    flint_runtime_on_enter_foreground(handle);
}

JNIEXPORT void JNICALL
Java_dev_flint_runtime_FlintView_nativeOnEnterBackground(JNIEnv* /*env*/, jobject /*self*/, jlong handle) {
    flint_runtime_on_enter_background(handle);
}

JNIEXPORT void JNICALL
Java_dev_flint_runtime_FlintView_nativeDestroy(JNIEnv* /*env*/, jobject /*self*/, jlong handle) {
    flint_runtime_destroy(handle);
}

}  // extern "C"
