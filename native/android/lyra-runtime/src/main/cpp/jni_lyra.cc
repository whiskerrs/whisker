// jni_lyra.cc
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
void lyra_runtime_init();
long lyra_runtime_attach_view(void* lynx_shell_ptr);
void lyra_runtime_on_enter_foreground(long handle);
void lyra_runtime_on_enter_background(long handle);
void lyra_runtime_destroy(long handle);

// dev/lyra/runtime/LyraApplication.nativeInitRuntime
JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraApplication_nativeInitRuntime(JNIEnv* /*env*/, jobject /*self*/) {
    lyra_runtime_init();
}

// dev/lyra/runtime/LyraView.nativeAttachView
JNIEXPORT jlong JNICALL
Java_dev_lyra_runtime_LyraView_nativeAttachView(JNIEnv* /*env*/, jobject /*self*/, jlong shellPtr) {
    return static_cast<jlong>(
        lyra_runtime_attach_view(reinterpret_cast<void*>(shellPtr)));
}

JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraView_nativeOnEnterForeground(JNIEnv* /*env*/, jobject /*self*/, jlong handle) {
    lyra_runtime_on_enter_foreground(handle);
}

JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraView_nativeOnEnterBackground(JNIEnv* /*env*/, jobject /*self*/, jlong handle) {
    lyra_runtime_on_enter_background(handle);
}

JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraView_nativeDestroy(JNIEnv* /*env*/, jobject /*self*/, jlong handle) {
    lyra_runtime_destroy(handle);
}

}  // extern "C"
