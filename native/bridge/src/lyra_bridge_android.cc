// lyra_bridge_android.cc
//
// Android-specific glue: extracts the LynxShell from a Java LynxView and
// drives the host-wake-up callback through JNI back into Kotlin. All actual
// Element PAPI work lives in lyra_bridge_common.cc.
//
// The whole file is gated on `__ANDROID__` so it compiles to nothing on
// non-Android platforms. Lyra's Cargo build scripts already select per
// platform, but the guard is defense-in-depth: any future build system
// (CMake / Bazel / Xcode) that scans this directory wholesale will get
// an empty translation unit on iOS / macOS / Linux instead of a
// `<jni.h> not found` failure.

#if defined(__ANDROID__)

#include <jni.h>
#include <android/log.h>
#include <dlfcn.h>

#include <atomic>
#include <cstdint>
#include <map>
#include <mutex>

#include "core/shell/lynx_shell.h"

#include "lyra_bridge.h"
#include "lyra_bridge_internal.h"

#define LOG_TAG "LyraBridge"
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)

namespace {

// Cached JVM + the Kotlin object/method we need to call back into for
// the "wake the render loop" path. Set lazily on the first attach so we
// don't pay reflection cost more than once.
struct JvmHandles {
    JavaVM* jvm = nullptr;
    jclass lyra_view_class = nullptr;        // global ref
    jmethodID request_frame_method = nullptr; // void requestFrameFromNative()
};

JvmHandles& Handles() {
    static JvmHandles h;
    return h;
}

// Stored per-engine: the Kotlin LyraView weak global ref we call back into.
// Held in a side map keyed by engine pointer so the common code doesn't
// need to know about Java.
struct EngineJavaState {
    jobject lyra_view_weak = nullptr;  // weak global ref
};

std::mutex& JavaStateMutex() {
    static std::mutex m;
    return m;
}
std::map<LyraEngine*, EngineJavaState*>& JavaStateMap() {
    static std::map<LyraEngine*, EngineJavaState*> m;
    return m;
}
EngineJavaState* LookupJavaState(LyraEngine* engine) {
    auto& m = JavaStateMap();
    auto it = m.find(engine);
    return it == m.end() ? nullptr : it->second;
}

// Extract LynxShell* from a Java LynxView via:
//   LynxView.mLynxTemplateRender   (protected LynxTemplateRender)
//     → LynxTemplateRender.mNativePtr   (private long; cast to LynxShell*)
lynx::shell::LynxShell* ExtractShell(JNIEnv* env, jobject lynx_view) {
    if (env == nullptr || lynx_view == nullptr) {
        LOGE("ExtractShell: env or lynx_view is null");
        return nullptr;
    }
    // GetObjectClass returns the runtime class (LyraView), but a JNI
    // GetFieldID call looks up inherited fields too.
    jclass view_class = env->GetObjectClass(lynx_view);
    if (view_class == nullptr) {
        LOGE("ExtractShell: GetObjectClass returned null");
        return nullptr;
    }
    // Belt-and-suspenders: also look up the *declaring* class to make
    // sure the lookup hits regardless of how JNI handles inherited
    // protected fields with obfuscation toolchains.
    jclass lynx_view_class = env->FindClass("com/lynx/tasm/LynxView");
    if (lynx_view_class == nullptr) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        LOGE("ExtractShell: FindClass com/lynx/tasm/LynxView failed");
        env->DeleteLocalRef(view_class);
        return nullptr;
    }
    jfieldID render_field = env->GetFieldID(
        lynx_view_class, "mLynxTemplateRender",
        "Lcom/lynx/tasm/LynxTemplateRender;");
    env->DeleteLocalRef(view_class);
    env->DeleteLocalRef(lynx_view_class);
    if (env->ExceptionCheck()) {
        env->ExceptionDescribe();
        env->ExceptionClear();
        LOGE("ExtractShell: GetFieldID(mLynxTemplateRender) failed");
        return nullptr;
    }
    if (render_field == nullptr) {
        LOGE("ExtractShell: render_field is nullptr (field not found?)");
        return nullptr;
    }
    jobject render = env->GetObjectField(lynx_view, render_field);
    if (render == nullptr) {
        LOGE("ExtractShell: mLynxTemplateRender is null on instance");
        return nullptr;
    }
    jclass render_class = env->FindClass("com/lynx/tasm/LynxTemplateRender");
    if (render_class == nullptr) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        LOGE("ExtractShell: FindClass com/lynx/tasm/LynxTemplateRender failed");
        env->DeleteLocalRef(render);
        return nullptr;
    }
    jfieldID native_ptr_field = env->GetFieldID(render_class, "mNativePtr", "J");
    env->DeleteLocalRef(render_class);
    if (env->ExceptionCheck()) {
        env->ExceptionDescribe();
        env->ExceptionClear();
        LOGE("ExtractShell: GetFieldID(mNativePtr) failed");
        env->DeleteLocalRef(render);
        return nullptr;
    }
    if (native_ptr_field == nullptr) {
        LOGE("ExtractShell: native_ptr_field is nullptr");
        env->DeleteLocalRef(render);
        return nullptr;
    }
    jlong native = env->GetLongField(render, native_ptr_field);
    env->DeleteLocalRef(render);
    return reinterpret_cast<lynx::shell::LynxShell*>(static_cast<intptr_t>(native));
}

// Trampoline the Rust runtime calls when a signal update needs a frame.
// `user_data` is a `LyraEngine*` we keep paired with the Kotlin view in
// the JavaStateFor map.
extern "C" void RequestFrameTrampoline(void* user_data) {
    auto* engine = static_cast<LyraEngine*>(user_data);
    JvmHandles& handles = Handles();
    if (handles.jvm == nullptr || handles.request_frame_method == nullptr) return;

    EngineJavaState* state = nullptr;
    {
        std::lock_guard<std::mutex> lock(JavaStateMutex());
        state = LookupJavaState(engine);
    }
    if (state == nullptr || state->lyra_view_weak == nullptr) return;

    JNIEnv* env = nullptr;
    bool attached = false;
    if (handles.jvm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6)
            != JNI_OK) {
        if (handles.jvm->AttachCurrentThread(&env, nullptr) != JNI_OK) {
            LOGE("RequestFrame: failed to attach thread to JVM");
            return;
        }
        attached = true;
    }
    jobject view = env->NewLocalRef(state->lyra_view_weak);
    if (view != nullptr) {
        env->CallVoidMethod(view, handles.request_frame_method);
        if (env->ExceptionCheck()) {
            env->ExceptionDescribe();
            env->ExceptionClear();
        }
        env->DeleteLocalRef(view);
    }
    if (attached) {
        handles.jvm->DetachCurrentThread();
    }
}

}  // namespace

// JNI_OnLoad — cache the JVM pointer + LyraView class/method handles.
extern "C" JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM* vm, void* /*reserved*/) {
    JvmHandles& handles = Handles();
    handles.jvm = vm;

    JNIEnv* env = nullptr;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) {
        return JNI_ERR;
    }
    jclass local = env->FindClass("dev/lyra/runtime/LyraView");
    if (local == nullptr) {
        LOGE("JNI_OnLoad: dev/lyra/runtime/LyraView not found");
        return JNI_ERR;
    }
    handles.lyra_view_class = static_cast<jclass>(env->NewGlobalRef(local));
    handles.request_frame_method = env->GetMethodID(
        handles.lyra_view_class, "requestFrameFromNative", "()V");
    env->DeleteLocalRef(local);
    if (handles.request_frame_method == nullptr) {
        LOGE("JNI_OnLoad: requestFrameFromNative not found on LyraView");
        return JNI_ERR;
    }
    return JNI_VERSION_1_6;
}

// dev.lyra.runtime.LyraView.nativeEngineAttach
extern "C" JNIEXPORT jlong JNICALL
Java_dev_lyra_runtime_LyraView_nativeEngineAttach(
    JNIEnv* env, jobject /*self*/, jobject lynx_view) {
    lynx::shell::LynxShell* shell = ExtractShell(env, lynx_view);
    if (shell == nullptr) {
        LOGE("nativeEngineAttach: could not extract LynxShell* from LynxView");
        return 0;
    }
    LyraEngine* engine = lyra_bridge_internal_engine_create(shell);
    if (engine == nullptr) {
        LOGE("nativeEngineAttach: lyra_bridge_internal_engine_create failed");
        return 0;
    }
    return reinterpret_cast<jlong>(engine);
}

// dev.lyra.runtime.LyraView.nativeBindLyraView
//
// Pairs the LyraEngine with the Kotlin LyraView that owns it so the
// `request_frame` trampoline can call back into Kotlin's render-loop
// pause/unpause logic.
extern "C" JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraView_nativeBindLyraView(
    JNIEnv* env, jobject self, jlong engine_raw) {
    auto* engine = reinterpret_cast<LyraEngine*>(engine_raw);
    if (engine == nullptr) return;
    auto* state = new EngineJavaState();
    state->lyra_view_weak = env->NewWeakGlobalRef(self);
    std::lock_guard<std::mutex> lock(JavaStateMutex());
    JavaStateMap()[engine] = state;
}

// dev.lyra.runtime.LyraView.nativeRequestFrameCallback
//
// Returns the C function pointer + user_data pair that Rust should call
// when signals dirty. Bundled into a small (fn, data) tuple via two
// jlong returns isn't great, so we use a dedicated init entry point —
// the caller (Kotlin) just hands us the engine and we wire it up.
//
// Used by LyraView right after nativeEngineAttach.
extern "C" JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraView_nativeEngineRelease(
    JNIEnv* env, jobject /*self*/, jlong engine_raw) {
    auto* engine = reinterpret_cast<LyraEngine*>(engine_raw);
    if (engine == nullptr) return;
    {
        std::lock_guard<std::mutex> lock(JavaStateMutex());
        EngineJavaState* state = LookupJavaState(engine);
        if (state != nullptr) {
            if (state->lyra_view_weak != nullptr) {
                env->DeleteWeakGlobalRef(state->lyra_view_weak);
            }
            delete state;
            JavaStateMap().erase(engine);
        }
    }
    lyra_bridge_engine_release(engine);
}

// Exposed to Kotlin so it can hand the trampoline + engine pair to the
// Rust runtime's `lyra_mobile_app_main`. Returning the function pointer
// directly as jlong keeps the bridge ABI tidy on the Kotlin side.
extern "C" JNIEXPORT jlong JNICALL
Java_dev_lyra_runtime_LyraView_nativeRequestFrameFnPtr(
    JNIEnv* /*env*/, jclass /*clazz*/) {
    return reinterpret_cast<jlong>(&RequestFrameTrampoline);
}

// Rust runtime entry points. Defined by the user's `#[lyra::main]` crate
// (e.g. examples/hello-world); on Android both these symbols and the
// bridge code below land in the same .so (build.rs compiles the bridge
// straight into the cdylib), so we can just call them directly — no
// dlsym dance needed.
extern "C" void lyra_mobile_app_main(void* engine,
                                     void (*request_frame)(void*),
                                     void* request_frame_data);
extern "C" bool lyra_mobile_tick(void* engine);

extern "C" JNIEXPORT void JNICALL
Java_dev_lyra_runtime_LyraView_nativeAppMain(
    JNIEnv* /*env*/, jobject /*self*/, jlong engine_raw) {
    lyra_mobile_app_main(reinterpret_cast<void*>(engine_raw),
                          &RequestFrameTrampoline,
                          reinterpret_cast<void*>(engine_raw));
}

extern "C" JNIEXPORT jboolean JNICALL
Java_dev_lyra_runtime_LyraView_nativeTick(
    JNIEnv* /*env*/, jobject /*self*/, jlong engine_raw) {
    return lyra_mobile_tick(reinterpret_cast<void*>(engine_raw))
        ? JNI_TRUE : JNI_FALSE;
}

// Called from Kotlin's EventEmitter.LynxEventReporter hook for every
// LynxEvent the engine fires. We look up the (element_sign, event_name)
// pair in the bridge's callback registry; if a Rust closure was
// registered for it, fire and report consumed.
//
// `engine_raw` isn't strictly needed today (the registry is global) but
// kept in the signature so future per-engine registries don't require
// an ABI break.
extern "C" JNIEXPORT jboolean JNICALL
Java_dev_lyra_runtime_LyraView_nativeOnLynxEvent(
    JNIEnv* env, jobject /*self*/, jlong /*engine_raw*/,
    jint tag, jstring name_jstr) {
    if (name_jstr == nullptr) return JNI_FALSE;
    const char* name = env->GetStringUTFChars(name_jstr, nullptr);
    if (name == nullptr) return JNI_FALSE;
    bool handled = lyra_bridge_internal_dispatch_event(
        static_cast<int32_t>(tag), name);
    env->ReleaseStringUTFChars(name_jstr, name);
    return handled ? JNI_TRUE : JNI_FALSE;
}

#endif  // __ANDROID__
