// whisker_bridge_android.cc
//
// Android-specific glue: extracts the LynxShell from a Java LynxView and
// drives the host-wake-up callback through JNI back into Kotlin. All actual
// Element PAPI work lives in whisker_bridge_common.cc.
//
// The whole file is gated on `__ANDROID__` so it compiles to nothing on
// non-Android platforms. Whisker's Cargo build scripts already select per
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

#include "whisker_bridge.h"
#include "whisker_bridge_internal.h"

#define LOG_TAG "WhiskerBridge"
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)

namespace {

// Cached JVM + the Kotlin object/method we need to call back into for
// the "wake the render loop" path. Set lazily on the first attach so we
// don't pay reflection cost more than once.
struct JvmHandles {
    JavaVM* jvm = nullptr;
    jclass whisker_view_class = nullptr;        // global ref
    jmethodID request_frame_method = nullptr; // void requestFrameFromNative()
};

JvmHandles& Handles() {
    static JvmHandles h;
    return h;
}

// Stored per-engine: the Kotlin WhiskerView weak global ref we call back into.
// Held in a side map keyed by engine pointer so the common code doesn't
// need to know about Java.
struct EngineJavaState {
    jobject whisker_view_weak = nullptr;  // weak global ref
};

std::mutex& JavaStateMutex() {
    static std::mutex m;
    return m;
}
std::map<WhiskerEngine*, EngineJavaState*>& JavaStateMap() {
    static std::map<WhiskerEngine*, EngineJavaState*> m;
    return m;
}
EngineJavaState* LookupJavaState(WhiskerEngine* engine) {
    auto& m = JavaStateMap();
    auto it = m.find(engine);
    return it == m.end() ? nullptr : it->second;
}

// Extract the raw LynxShell pointer from a Java LynxView via:
//   LynxView.mLynxTemplateRender   (protected LynxTemplateRender)
//     → LynxTemplateRender.mNativePtr   (private long; jlong-cast pointer)
//
// We deliberately do NOT cast to `lynx::shell::LynxShell*` here — the
// bridge no longer pulls in Lynx C++ headers; the void* gets handed
// to `lynx_shell_from_native_ptr` (defined inside liblynx.so) which
// does the cast on the Lynx side.
void* ExtractShell(JNIEnv* env, jobject lynx_view) {
    if (env == nullptr || lynx_view == nullptr) {
        LOGE("ExtractShell: env or lynx_view is null");
        return nullptr;
    }
    // GetObjectClass returns the runtime class (WhiskerView), but a JNI
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
    return reinterpret_cast<void*>(static_cast<intptr_t>(native));
}

// Trampoline the Rust runtime calls when a signal update needs a frame.
// `user_data` is a `WhiskerEngine*` we keep paired with the Kotlin view in
// the JavaStateFor map.
extern "C" void RequestFrameTrampoline(void* user_data) {
    auto* engine = static_cast<WhiskerEngine*>(user_data);
    JvmHandles& handles = Handles();
    if (handles.jvm == nullptr || handles.request_frame_method == nullptr) return;

    EngineJavaState* state = nullptr;
    {
        std::lock_guard<std::mutex> lock(JavaStateMutex());
        state = LookupJavaState(engine);
    }
    if (state == nullptr || state->whisker_view_weak == nullptr) return;

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
    jobject view = env->NewLocalRef(state->whisker_view_weak);
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

// JNI_OnLoad — cache the JVM pointer + WhiskerView class/method handles.
extern "C" JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM* vm, void* /*reserved*/) {
    JvmHandles& handles = Handles();
    handles.jvm = vm;

    JNIEnv* env = nullptr;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) {
        return JNI_ERR;
    }
    jclass local = env->FindClass("rs/whisker/runtime/WhiskerView");
    if (local == nullptr) {
        LOGE("JNI_OnLoad: rs/whisker/runtime/WhiskerView not found");
        return JNI_ERR;
    }
    handles.whisker_view_class = static_cast<jclass>(env->NewGlobalRef(local));
    handles.request_frame_method = env->GetMethodID(
        handles.whisker_view_class, "requestFrameFromNative", "()V");
    env->DeleteLocalRef(local);
    if (handles.request_frame_method == nullptr) {
        LOGE("JNI_OnLoad: requestFrameFromNative not found on WhiskerView");
        return JNI_ERR;
    }
    return JNI_VERSION_1_6;
}

// rs.whisker.runtime.WhiskerView.nativeEngineAttach
extern "C" JNIEXPORT jlong JNICALL
Java_rs_whisker_runtime_WhiskerView_nativeEngineAttach(
    JNIEnv* env, jobject /*self*/, jobject lynx_view) {
    void* native_shell_ptr = ExtractShell(env, lynx_view);
    if (native_shell_ptr == nullptr) {
        LOGE("nativeEngineAttach: could not extract LynxShell* from LynxView");
        return 0;
    }
    WhiskerEngine* engine = whisker_bridge_internal_engine_create(native_shell_ptr);
    if (engine == nullptr) {
        LOGE("nativeEngineAttach: whisker_bridge_internal_engine_create failed");
        return 0;
    }
    return reinterpret_cast<jlong>(engine);
}

// rs.whisker.runtime.WhiskerView.nativeBindWhiskerView
//
// Pairs the WhiskerEngine with the Kotlin WhiskerView that owns it so the
// `request_frame` trampoline can call back into Kotlin's render-loop
// pause/unpause logic.
extern "C" JNIEXPORT void JNICALL
Java_rs_whisker_runtime_WhiskerView_nativeBindWhiskerView(
    JNIEnv* env, jobject self, jlong engine_raw) {
    auto* engine = reinterpret_cast<WhiskerEngine*>(engine_raw);
    if (engine == nullptr) return;
    auto* state = new EngineJavaState();
    state->whisker_view_weak = env->NewWeakGlobalRef(self);
    std::lock_guard<std::mutex> lock(JavaStateMutex());
    JavaStateMap()[engine] = state;
}

// rs.whisker.runtime.WhiskerView.nativeRequestFrameCallback
//
// Returns the C function pointer + user_data pair that Rust should call
// when signals dirty. Bundled into a small (fn, data) tuple via two
// jlong returns isn't great, so we use a dedicated init entry point —
// the caller (Kotlin) just hands us the engine and we wire it up.
//
// Used by WhiskerView right after nativeEngineAttach.
extern "C" JNIEXPORT void JNICALL
Java_rs_whisker_runtime_WhiskerView_nativeEngineRelease(
    JNIEnv* env, jobject /*self*/, jlong engine_raw) {
    auto* engine = reinterpret_cast<WhiskerEngine*>(engine_raw);
    if (engine == nullptr) return;
    {
        std::lock_guard<std::mutex> lock(JavaStateMutex());
        EngineJavaState* state = LookupJavaState(engine);
        if (state != nullptr) {
            if (state->whisker_view_weak != nullptr) {
                env->DeleteWeakGlobalRef(state->whisker_view_weak);
            }
            delete state;
            JavaStateMap().erase(engine);
        }
    }
    whisker_bridge_engine_release(engine);
}

// Exposed to Kotlin so it can hand the trampoline + engine pair to the
// Rust runtime's `whisker_app_main`. Returning the function pointer
// directly as jlong keeps the bridge ABI tidy on the Kotlin side.
extern "C" JNIEXPORT jlong JNICALL
Java_rs_whisker_runtime_WhiskerView_nativeRequestFrameFnPtr(
    JNIEnv* /*env*/, jclass /*clazz*/) {
    return reinterpret_cast<jlong>(&RequestFrameTrampoline);
}

// Rust runtime entry points. Defined by the user's `#[whisker::main]` crate
// (e.g. examples/hello-world); on Android both these symbols and the
// bridge code below land in the same .so (build.rs compiles the bridge
// straight into the cdylib), so we can just call them directly — no
// dlsym dance needed.
extern "C" void whisker_app_main(void* engine,
                              void (*request_frame)(void*),
                              void* request_frame_data);
extern "C" bool whisker_tick(void* engine);

extern "C" JNIEXPORT void JNICALL
Java_rs_whisker_runtime_WhiskerView_nativeAppMain(
    JNIEnv* /*env*/, jobject /*self*/, jlong engine_raw) {
    whisker_app_main(reinterpret_cast<void*>(engine_raw),
                  &RequestFrameTrampoline,
                  reinterpret_cast<void*>(engine_raw));
}

extern "C" JNIEXPORT jboolean JNICALL
Java_rs_whisker_runtime_WhiskerView_nativeTick(
    JNIEnv* /*env*/, jobject /*self*/, jlong engine_raw) {
    return whisker_tick(reinterpret_cast<void*>(engine_raw))
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
Java_rs_whisker_runtime_WhiskerView_nativeOnLynxEvent(
    JNIEnv* env, jobject /*self*/, jlong /*engine_raw*/,
    jint tag, jstring name_jstr, jstring payload_jstr) {
    if (name_jstr == nullptr) return JNI_FALSE;
    const char* name = env->GetStringUTFChars(name_jstr, nullptr);
    if (name == nullptr) return JNI_FALSE;
    // Phase 7-Φ.C: the Kotlin side now serialises event.params via
    // org.json.JSONObject and hands the result here. Mirrors the iOS
    // event reporter that goes through `[event generateEventBody]` +
    // `NSJSONSerialization`. Empty string means "no detail" — same
    // contract as iOS.
    const char* payload = nullptr;
    if (payload_jstr != nullptr) {
        payload = env->GetStringUTFChars(payload_jstr, nullptr);
    }
    bool handled = whisker_bridge_internal_dispatch_event(
        static_cast<int32_t>(tag), name, payload);
    if (payload != nullptr) {
        env->ReleaseStringUTFChars(payload_jstr, payload);
    }
    env->ReleaseStringUTFChars(name_jstr, name);
    return handled ? JNI_TRUE : JNI_FALSE;
}

// ---- Native module invocation (Phase 7-Φ.F, Android) ---------------------
//
// `whisker_bridge_invoke_module` routes into Kotlin's
// `WhiskerModuleRegistry.invokeDispatch(...)` via JNI. The KSP
// processor generates per-module dispatch lambdas that the
// registry maps by name. Args/returns are typed `WhiskerValue`
// (sealed class hierarchy in Kotlin) — no Foundation / boxed-Java
// type marshalling.
//
// JNI handles for `WhiskerValue` subclasses + the registry's
// dispatch method are cached once per process on first call.

#include <cstring>
#include <cstdlib>
#include <string>
#include <utility>
#include <vector>

namespace {

WhiskerValue MakeAndroidBridgeError(const char* message) {
    WhiskerValue v;
    std::memset(&v, 0, sizeof(v));
    v.type = WHISKER_VALUE_ERROR;
    if (message != nullptr) {
        size_t len = std::strlen(message);
        char* buf = static_cast<char*>(std::malloc(len + 1));
        std::memcpy(buf, message, len + 1);
        v.v.s.ptr = buf;
        v.v.s.len = len;
    }
    return v;
}

struct WhiskerValueJni {
    bool ready = false;
    jclass base = nullptr;
    jclass null_cls = nullptr;
    jobject null_obj = nullptr;
    jclass bool_cls = nullptr;
    jmethodID bool_ctor = nullptr, bool_get = nullptr;
    jclass int_cls = nullptr;
    jmethodID int_ctor = nullptr, int_get = nullptr;
    jclass float_cls = nullptr;
    jmethodID float_ctor = nullptr, float_get = nullptr;
    jclass str_cls = nullptr;
    jmethodID str_ctor = nullptr, str_get = nullptr;
    jclass bytes_cls = nullptr;
    jmethodID bytes_ctor = nullptr, bytes_get = nullptr;
    jclass array_cls = nullptr;
    jmethodID array_ctor = nullptr, array_get = nullptr;
    jclass map_cls = nullptr;
    jmethodID map_ctor = nullptr, map_get = nullptr;
    jclass err_cls = nullptr;
    jmethodID err_ctor = nullptr, err_get_msg = nullptr;

    jclass arraylist_cls = nullptr;
    jmethodID arraylist_ctor = nullptr, arraylist_add = nullptr;
    jclass list_cls = nullptr;
    jmethodID list_size = nullptr, list_get = nullptr;
    jclass hashmap_cls = nullptr;
    jmethodID hashmap_ctor = nullptr, hashmap_put = nullptr;
    jclass map_iface = nullptr;
    jmethodID map_entry_set = nullptr;
    jclass set_cls = nullptr;
    jmethodID set_iterator = nullptr;
    jclass iter_cls = nullptr;
    jmethodID iter_has_next = nullptr, iter_next = nullptr;
    jclass map_entry_cls = nullptr;
    jmethodID map_entry_key = nullptr, map_entry_val = nullptr;

    jclass registry_cls = nullptr;
    jmethodID registry_dispatch = nullptr;
};

WhiskerValueJni& wvjni() {
    static WhiskerValueJni h;
    return h;
}

jclass make_global(JNIEnv* env, const char* path) {
    jclass local = env->FindClass(path);
    if (local == nullptr) return nullptr;
    auto g = reinterpret_cast<jclass>(env->NewGlobalRef(local));
    env->DeleteLocalRef(local);
    return g;
}

bool init_wvjni(JNIEnv* env) {
    auto& h = wvjni();
    if (h.ready) return true;
    h.base       = make_global(env, "rs/whisker/runtime/WhiskerValue");
    h.null_cls   = make_global(env, "rs/whisker/runtime/WhiskerValue$Null");
    h.bool_cls   = make_global(env, "rs/whisker/runtime/WhiskerValue$Bool");
    h.int_cls    = make_global(env, "rs/whisker/runtime/WhiskerValue$Int");
    h.float_cls  = make_global(env, "rs/whisker/runtime/WhiskerValue$Float");
    h.str_cls    = make_global(env, "rs/whisker/runtime/WhiskerValue$Str");
    h.bytes_cls  = make_global(env, "rs/whisker/runtime/WhiskerValue$Bytes");
    h.array_cls  = make_global(env, "rs/whisker/runtime/WhiskerValue$Array");
    h.map_cls    = make_global(env, "rs/whisker/runtime/WhiskerValue$Map");
    h.err_cls    = make_global(env, "rs/whisker/runtime/WhiskerValue$Err");
    h.arraylist_cls = make_global(env, "java/util/ArrayList");
    h.list_cls      = make_global(env, "java/util/List");
    h.hashmap_cls   = make_global(env, "java/util/HashMap");
    h.map_iface     = make_global(env, "java/util/Map");
    h.set_cls       = make_global(env, "java/util/Set");
    h.iter_cls      = make_global(env, "java/util/Iterator");
    h.map_entry_cls = make_global(env, "java/util/Map$Entry");
    h.registry_cls  = make_global(env, "rs/whisker/runtime/WhiskerModuleRegistry");
    if (h.base == nullptr || h.registry_cls == nullptr) return false;

    jfieldID null_field = env->GetStaticFieldID(
        h.null_cls, "INSTANCE", "Lrs/whisker/runtime/WhiskerValue$Null;");
    if (null_field != nullptr) {
        jobject local = env->GetStaticObjectField(h.null_cls, null_field);
        h.null_obj = env->NewGlobalRef(local);
        env->DeleteLocalRef(local);
    }

    h.bool_ctor = env->GetMethodID(h.bool_cls, "<init>", "(Z)V");
    h.bool_get  = env->GetMethodID(h.bool_cls, "getValue", "()Z");
    h.int_ctor  = env->GetMethodID(h.int_cls, "<init>", "(J)V");
    h.int_get   = env->GetMethodID(h.int_cls, "getValue", "()J");
    h.float_ctor = env->GetMethodID(h.float_cls, "<init>", "(D)V");
    h.float_get  = env->GetMethodID(h.float_cls, "getValue", "()D");
    h.str_ctor = env->GetMethodID(h.str_cls, "<init>", "(Ljava/lang/String;)V");
    h.str_get  = env->GetMethodID(h.str_cls, "getValue", "()Ljava/lang/String;");
    h.bytes_ctor = env->GetMethodID(h.bytes_cls, "<init>", "([B)V");
    h.bytes_get  = env->GetMethodID(h.bytes_cls, "getValue", "()[B");
    h.array_ctor = env->GetMethodID(h.array_cls, "<init>", "(Ljava/util/List;)V");
    h.array_get  = env->GetMethodID(h.array_cls, "getValue", "()Ljava/util/List;");
    h.map_ctor = env->GetMethodID(h.map_cls, "<init>", "(Ljava/util/Map;)V");
    h.map_get  = env->GetMethodID(h.map_cls, "getValue", "()Ljava/util/Map;");
    h.err_ctor = env->GetMethodID(h.err_cls, "<init>", "(Ljava/lang/String;)V");
    h.err_get_msg = env->GetMethodID(h.err_cls, "getMessage", "()Ljava/lang/String;");

    h.arraylist_ctor = env->GetMethodID(h.arraylist_cls, "<init>", "()V");
    h.arraylist_add  = env->GetMethodID(h.arraylist_cls, "add", "(Ljava/lang/Object;)Z");
    h.list_size = env->GetMethodID(h.list_cls, "size", "()I");
    h.list_get  = env->GetMethodID(h.list_cls, "get", "(I)Ljava/lang/Object;");
    h.hashmap_ctor = env->GetMethodID(h.hashmap_cls, "<init>", "()V");
    h.hashmap_put  = env->GetMethodID(h.hashmap_cls, "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;");
    h.map_entry_set = env->GetMethodID(h.map_iface, "entrySet", "()Ljava/util/Set;");
    h.set_iterator  = env->GetMethodID(h.set_cls, "iterator", "()Ljava/util/Iterator;");
    h.iter_has_next = env->GetMethodID(h.iter_cls, "hasNext", "()Z");
    h.iter_next     = env->GetMethodID(h.iter_cls, "next", "()Ljava/lang/Object;");
    h.map_entry_key = env->GetMethodID(h.map_entry_cls, "getKey", "()Ljava/lang/Object;");
    h.map_entry_val = env->GetMethodID(h.map_entry_cls, "getValue", "()Ljava/lang/Object;");

    h.registry_dispatch = env->GetStaticMethodID(
        h.registry_cls, "invokeDispatch",
        "(Ljava/lang/String;Ljava/lang/String;[Lrs/whisker/runtime/WhiskerValue;)"
        "Lrs/whisker/runtime/WhiskerValue;");
    if (h.registry_dispatch == nullptr) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }
    h.ready = true;
    return true;
}

struct ScopedJNIEnv_M {
    JNIEnv* env = nullptr;
    bool attached = false;
    ScopedJNIEnv_M() {
        JavaVM* jvm = Handles().jvm;
        if (jvm == nullptr) return;
        int rc = jvm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6);
        if (rc == JNI_EDETACHED) {
            if (jvm->AttachCurrentThread(&env, nullptr) == JNI_OK) {
                attached = true;
            } else {
                env = nullptr;
            }
        }
    }
    ~ScopedJNIEnv_M() {
        if (attached && env != nullptr) {
            JavaVM* jvm = Handles().jvm;
            if (jvm != nullptr) jvm->DetachCurrentThread();
        }
    }
    JNIEnv* get() { return env; }
};

jobject value_to_jvalue(JNIEnv* env, const WhiskerValue* v);
WhiskerValue jvalue_to_value(JNIEnv* env, jobject obj);

jobject value_to_jvalue(JNIEnv* env, const WhiskerValue* v) {
    auto& h = wvjni();
    if (v == nullptr) return env->NewLocalRef(h.null_obj);
    switch (v->type) {
        case WHISKER_VALUE_NULL:
            return env->NewLocalRef(h.null_obj);
        case WHISKER_VALUE_BOOL:
            return env->NewObject(h.bool_cls, h.bool_ctor, (jboolean)v->v.b);
        case WHISKER_VALUE_INT:
            return env->NewObject(h.int_cls, h.int_ctor, (jlong)v->v.i);
        case WHISKER_VALUE_FLOAT:
            return env->NewObject(h.float_cls, h.float_ctor, (jdouble)v->v.f);
        case WHISKER_VALUE_STRING: {
            std::string s = (v->v.s.ptr != nullptr)
                ? std::string(v->v.s.ptr, v->v.s.len) : std::string();
            jstring js = env->NewStringUTF(s.c_str());
            jobject out = env->NewObject(h.str_cls, h.str_ctor, js);
            env->DeleteLocalRef(js);
            return out;
        }
        case WHISKER_VALUE_BYTES: {
            jbyteArray arr = env->NewByteArray((jsize)v->v.bytes.len);
            if (v->v.bytes.len > 0 && v->v.bytes.ptr != nullptr) {
                env->SetByteArrayRegion(arr, 0, (jsize)v->v.bytes.len,
                    reinterpret_cast<const jbyte*>(v->v.bytes.ptr));
            }
            jobject out = env->NewObject(h.bytes_cls, h.bytes_ctor, arr);
            env->DeleteLocalRef(arr);
            return out;
        }
        case WHISKER_VALUE_ARRAY: {
            jobject list = env->NewObject(h.arraylist_cls, h.arraylist_ctor);
            for (size_t i = 0; i < v->v.array.count; i++) {
                jobject item = value_to_jvalue(env, &v->v.array.items[i]);
                env->CallBooleanMethod(list, h.arraylist_add, item);
                if (item != nullptr) env->DeleteLocalRef(item);
            }
            jobject out = env->NewObject(h.array_cls, h.array_ctor, list);
            env->DeleteLocalRef(list);
            return out;
        }
        case WHISKER_VALUE_MAP: {
            jobject map = env->NewObject(h.hashmap_cls, h.hashmap_ctor);
            for (size_t i = 0; i < v->v.map.count; i++) {
                std::string k(v->v.map.entries[i].key.ptr,
                              v->v.map.entries[i].key.len);
                jstring kj = env->NewStringUTF(k.c_str());
                jobject vj = value_to_jvalue(env, &v->v.map.entries[i].value);
                jobject prev = env->CallObjectMethod(map, h.hashmap_put, kj, vj);
                if (prev != nullptr) env->DeleteLocalRef(prev);
                env->DeleteLocalRef(kj);
                if (vj != nullptr) env->DeleteLocalRef(vj);
            }
            jobject out = env->NewObject(h.map_cls, h.map_ctor, map);
            env->DeleteLocalRef(map);
            return out;
        }
        case WHISKER_VALUE_ERROR: {
            std::string s = (v->v.s.ptr != nullptr)
                ? std::string(v->v.s.ptr, v->v.s.len) : std::string();
            jstring js = env->NewStringUTF(s.c_str());
            jobject out = env->NewObject(h.err_cls, h.err_ctor, js);
            env->DeleteLocalRef(js);
            return out;
        }
        default:
            return env->NewLocalRef(h.null_obj);
    }
}

std::string jstr_to_str(JNIEnv* env, jstring js) {
    if (js == nullptr) return std::string();
    const char* p = env->GetStringUTFChars(js, nullptr);
    std::string s = p != nullptr ? std::string(p) : std::string();
    if (p != nullptr) env->ReleaseStringUTFChars(js, p);
    return s;
}

char* dup_malloc(const std::string& s) {
    char* buf = static_cast<char*>(std::malloc(s.size() + 1));
    std::memcpy(buf, s.c_str(), s.size() + 1);
    return buf;
}

WhiskerValue jvalue_to_value(JNIEnv* env, jobject obj) {
    WhiskerValue v;
    std::memset(&v, 0, sizeof(v));
    auto& h = wvjni();
    if (obj == nullptr) { v.type = WHISKER_VALUE_NULL; return v; }
    if (env->IsInstanceOf(obj, h.null_cls))  { v.type = WHISKER_VALUE_NULL; return v; }
    if (env->IsInstanceOf(obj, h.bool_cls))  { v.type = WHISKER_VALUE_BOOL;
        v.v.b = env->CallBooleanMethod(obj, h.bool_get); return v; }
    if (env->IsInstanceOf(obj, h.int_cls))   { v.type = WHISKER_VALUE_INT;
        v.v.i = env->CallLongMethod(obj, h.int_get); return v; }
    if (env->IsInstanceOf(obj, h.float_cls)) { v.type = WHISKER_VALUE_FLOAT;
        v.v.f = env->CallDoubleMethod(obj, h.float_get); return v; }
    if (env->IsInstanceOf(obj, h.str_cls)) {
        jstring js = (jstring)env->CallObjectMethod(obj, h.str_get);
        std::string s = jstr_to_str(env, js);
        if (js != nullptr) env->DeleteLocalRef(js);
        v.type = WHISKER_VALUE_STRING;
        v.v.s.ptr = dup_malloc(s); v.v.s.len = s.size(); return v;
    }
    if (env->IsInstanceOf(obj, h.bytes_cls)) {
        jbyteArray arr = (jbyteArray)env->CallObjectMethod(obj, h.bytes_get);
        jsize len = (arr != nullptr) ? env->GetArrayLength(arr) : 0;
        uint8_t* buf = static_cast<uint8_t*>(std::malloc(len > 0 ? (size_t)len : 1));
        if (len > 0 && arr != nullptr) {
            env->GetByteArrayRegion(arr, 0, len, reinterpret_cast<jbyte*>(buf));
        }
        if (arr != nullptr) env->DeleteLocalRef(arr);
        v.type = WHISKER_VALUE_BYTES;
        v.v.bytes.ptr = buf; v.v.bytes.len = (size_t)len; return v;
    }
    if (env->IsInstanceOf(obj, h.array_cls)) {
        jobject list = env->CallObjectMethod(obj, h.array_get);
        jint sz = env->CallIntMethod(list, h.list_size);
        WhiskerValue* items = static_cast<WhiskerValue*>(
            std::malloc(((size_t)sz > 0 ? (size_t)sz : 1) * sizeof(WhiskerValue)));
        for (jint i = 0; i < sz; i++) {
            jobject elem = env->CallObjectMethod(list, h.list_get, i);
            items[i] = jvalue_to_value(env, elem);
            if (elem != nullptr) env->DeleteLocalRef(elem);
        }
        env->DeleteLocalRef(list);
        v.type = WHISKER_VALUE_ARRAY;
        v.v.array.items = items; v.v.array.count = (size_t)sz; return v;
    }
    if (env->IsInstanceOf(obj, h.map_cls)) {
        jobject map = env->CallObjectMethod(obj, h.map_get);
        jobject set = env->CallObjectMethod(map, h.map_entry_set);
        jobject it  = env->CallObjectMethod(set, h.set_iterator);
        std::vector<std::pair<std::string, WhiskerValue>> tmp;
        while (env->CallBooleanMethod(it, h.iter_has_next)) {
            jobject entry = env->CallObjectMethod(it, h.iter_next);
            jstring ks = (jstring)env->CallObjectMethod(entry, h.map_entry_key);
            jobject vo = env->CallObjectMethod(entry, h.map_entry_val);
            std::string k = jstr_to_str(env, ks);
            WhiskerValue val = jvalue_to_value(env, vo);
            tmp.emplace_back(std::move(k), val);
            if (ks != nullptr) env->DeleteLocalRef(ks);
            if (vo != nullptr) env->DeleteLocalRef(vo);
            env->DeleteLocalRef(entry);
        }
        env->DeleteLocalRef(it); env->DeleteLocalRef(set); env->DeleteLocalRef(map);
        size_t n = tmp.size();
        WhiskerKeyValue* entries = static_cast<WhiskerKeyValue*>(
            std::malloc((n > 0 ? n : 1) * sizeof(WhiskerKeyValue)));
        for (size_t i = 0; i < n; i++) {
            entries[i].key.ptr = dup_malloc(tmp[i].first);
            entries[i].key.len = tmp[i].first.size();
            entries[i].value = tmp[i].second;
        }
        v.type = WHISKER_VALUE_MAP;
        v.v.map.entries = entries; v.v.map.count = n; return v;
    }
    if (env->IsInstanceOf(obj, h.err_cls)) {
        jstring js = (jstring)env->CallObjectMethod(obj, h.err_get_msg);
        std::string s = jstr_to_str(env, js);
        if (js != nullptr) env->DeleteLocalRef(js);
        v.type = WHISKER_VALUE_ERROR;
        v.v.s.ptr = dup_malloc(s); v.v.s.len = s.size(); return v;
    }
    v.type = WHISKER_VALUE_ERROR;
    const char* msg = "unknown WhiskerValue subtype";
    v.v.s.ptr = dup_malloc(msg); v.v.s.len = std::strlen(msg); return v;
}

}  // namespace

extern "C" WhiskerValue whisker_bridge_invoke_module(
    const char* module_name, const char* method_name,
    const WhiskerValue* args, size_t arg_count
) {
    if (module_name == nullptr || method_name == nullptr) {
        return MakeAndroidBridgeError("module/method name NULL");
    }
    ScopedJNIEnv_M guard;
    JNIEnv* env = guard.get();
    if (env == nullptr) {
        return MakeAndroidBridgeError("JVM not initialised");
    }
    if (!init_wvjni(env)) {
        return MakeAndroidBridgeError("WhiskerValue JNI init failed");
    }
    auto& h = wvjni();
    jobjectArray jargs = env->NewObjectArray((jsize)arg_count, h.base, nullptr);
    for (size_t i = 0; i < arg_count; i++) {
        jobject jv = value_to_jvalue(env, &args[i]);
        env->SetObjectArrayElement(jargs, (jsize)i, jv);
        if (jv != nullptr) env->DeleteLocalRef(jv);
    }
    jstring jmod = env->NewStringUTF(module_name);
    jstring jmtd = env->NewStringUTF(method_name);
    jobject jres = env->CallStaticObjectMethod(
        h.registry_cls, h.registry_dispatch, jmod, jmtd, jargs);
    env->DeleteLocalRef(jmod); env->DeleteLocalRef(jmtd); env->DeleteLocalRef(jargs);
    if (env->ExceptionCheck()) { env->ExceptionDescribe(); env->ExceptionClear();
        return MakeAndroidBridgeError("dispatch threw"); }
    WhiskerValue out = jvalue_to_value(env, jres);
    if (jres != nullptr) env->DeleteLocalRef(jres);
    return out;
}

extern "C" bool whisker_bridge_invoke_module_async(
    const char* module_name, const char* method_name,
    const WhiskerValue* args, size_t arg_count,
    WhiskerModuleCallback callback, void* user_data
) {
    if (callback == nullptr) return false;
    WhiskerValue r = whisker_bridge_invoke_module(module_name, method_name, args, arg_count);
    callback(user_data, &r);
    whisker_bridge_value_release(&r);
    return true;
}

#endif  // __ANDROID__
