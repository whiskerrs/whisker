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

// ---- Native module invocation (Phase 7-Φ.E.3) ----------------------------
//
// Real Android dispatch — mirrors the iOS path from E.2 but goes
// through JNI: look up the class via the `WhiskerModuleRegistry`
// Kotlin object, find the method by name (signature is the wire
// shape both platforms share — single `Array<Any?>` / `Object[]`
// arg, returns `Any?` / `Object`), cache the resolved `jmethodID`
// per (class, method) so subsequent calls skip the
// `GetMethodID` reflection cost, build a `jobjectArray` of args
// from `WhiskerValue[]`, invoke via `CallObjectMethodA`, convert
// the returned `jobject` back to `WhiskerValue`.

#include <cstring>
#include <cstdlib>
#include <string>
#include <unordered_map>

namespace {

WhiskerValue WhiskerMakeErrorValueAndroid(const char* message) {
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

// Thread attachment helper. Bridge calls can arrive from any
// thread (Rust runtime, TASM thread, async pool, …) — we need a
// JNIEnv each time. `GetEnv` reports JNI_EDETACHED for threads
// the JVM doesn't already own, in which case we attach and
// remember to detach on scope exit. Pre-attached threads (the
// app's main thread, JVM thread pool workers) keep their
// attachment.
struct ScopedJNIEnv {
    JNIEnv* env = nullptr;
    bool attached = false;

    ScopedJNIEnv() {
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

    ~ScopedJNIEnv() {
        if (attached && env != nullptr) {
            JavaVM* jvm = Handles().jvm;
            if (jvm != nullptr) jvm->DetachCurrentThread();
        }
    }

    JNIEnv* get() { return env; }
};

// jmethodID cache keyed by `<class-pointer-as-string>::<method-name>`.
// `jclass` is a `jobject` (pointer to JVM class object); we promote
// it to a global ref before storing so the cached identifier
// stays live across GCs. Keyed by string for simplicity over a
// custom hasher.
struct ModuleMethodCacheEntry {
    jclass cls_global_ref = nullptr;
    jmethodID method_id = nullptr;
};

std::mutex& ModuleMethodCacheMutex() {
    static std::mutex m;
    return m;
}
std::unordered_map<std::string, ModuleMethodCacheEntry>& ModuleMethodCache() {
    static std::unordered_map<std::string, ModuleMethodCacheEntry> c;
    return c;
}

// Resolve the cached `jmethodID` for `(cls, method_name)`,
// inserting via `GetMethodID` on first miss. Both args expected
// to be non-null. Returns null `method_id` if the method isn't
// found.
ModuleMethodCacheEntry GetCachedMethod(JNIEnv* env, jclass cls,
                                       const char* method_name) {
    // Use the class's hashCode as a stable identifier across
    // local refs.
    jclass class_class = env->FindClass("java/lang/Class");
    jmethodID hash_method = env->GetMethodID(class_class, "hashCode", "()I");
    jint class_hash = env->CallIntMethod(cls, hash_method);
    env->DeleteLocalRef(class_class);

    std::string key = std::to_string(class_hash) + "::" + method_name;
    auto& mutex = ModuleMethodCacheMutex();
    {
        std::lock_guard<std::mutex> g(mutex);
        auto it = ModuleMethodCache().find(key);
        if (it != ModuleMethodCache().end()) return it->second;
    }

    ModuleMethodCacheEntry entry;
    // Whisker module methods all share the same signature:
    // `Object method(Object[] args)`. See the convention note in
    // WhiskerModuleRegistry.kt.
    entry.method_id = env->GetMethodID(cls, method_name,
                                       "([Ljava/lang/Object;)Ljava/lang/Object;");
    if (entry.method_id == nullptr) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return entry;
    }
    entry.cls_global_ref = reinterpret_cast<jclass>(env->NewGlobalRef(cls));

    {
        std::lock_guard<std::mutex> g(mutex);
        auto [it, inserted] = ModuleMethodCache().emplace(key, entry);
        if (!inserted) {
            // Lost the race — release our just-allocated global
            // ref and use the existing entry instead.
            env->DeleteGlobalRef(entry.cls_global_ref);
            return it->second;
        }
    }
    return entry;
}

// Forward declarations — converters call each other for the
// array/map variants.
jobject WhiskerValueToJObject(JNIEnv* env, const WhiskerValue* v);
WhiskerValue JObjectToWhiskerValue(JNIEnv* env, jobject obj);

jobject WhiskerValueToJObject(JNIEnv* env, const WhiskerValue* v) {
    if (v == nullptr) return nullptr;
    switch (v->type) {
        case WHISKER_VALUE_NULL:
            return nullptr;
        case WHISKER_VALUE_BOOL: {
            jclass cls = env->FindClass("java/lang/Boolean");
            jmethodID ctor = env->GetMethodID(cls, "<init>", "(Z)V");
            jobject o = env->NewObject(cls, ctor, (jboolean)v->v.b);
            env->DeleteLocalRef(cls);
            return o;
        }
        case WHISKER_VALUE_INT: {
            jclass cls = env->FindClass("java/lang/Long");
            jmethodID ctor = env->GetMethodID(cls, "<init>", "(J)V");
            jobject o = env->NewObject(cls, ctor, (jlong)v->v.i);
            env->DeleteLocalRef(cls);
            return o;
        }
        case WHISKER_VALUE_FLOAT: {
            jclass cls = env->FindClass("java/lang/Double");
            jmethodID ctor = env->GetMethodID(cls, "<init>", "(D)V");
            jobject o = env->NewObject(cls, ctor, (jdouble)v->v.f);
            env->DeleteLocalRef(cls);
            return o;
        }
        case WHISKER_VALUE_STRING: {
            if (v->v.s.ptr == nullptr) {
                return env->NewStringUTF("");
            }
            std::string s(v->v.s.ptr, v->v.s.len);
            return env->NewStringUTF(s.c_str());
        }
        case WHISKER_VALUE_BYTES: {
            jbyteArray arr = env->NewByteArray((jsize)v->v.bytes.len);
            if (v->v.bytes.len > 0 && v->v.bytes.ptr != nullptr) {
                env->SetByteArrayRegion(
                    arr, 0, (jsize)v->v.bytes.len,
                    reinterpret_cast<const jbyte*>(v->v.bytes.ptr));
            }
            return arr;
        }
        case WHISKER_VALUE_ARRAY: {
            jclass object_cls = env->FindClass("java/lang/Object");
            jobjectArray arr =
                env->NewObjectArray((jsize)v->v.array.count, object_cls, nullptr);
            for (size_t i = 0; i < v->v.array.count; i++) {
                jobject item = WhiskerValueToJObject(env, &v->v.array.items[i]);
                env->SetObjectArrayElement(arr, (jsize)i, item);
                if (item != nullptr) env->DeleteLocalRef(item);
            }
            env->DeleteLocalRef(object_cls);
            return arr;
        }
        case WHISKER_VALUE_MAP: {
            jclass hashmap_cls = env->FindClass("java/util/HashMap");
            jmethodID ctor = env->GetMethodID(hashmap_cls, "<init>", "()V");
            jobject map = env->NewObject(hashmap_cls, ctor);
            jmethodID put = env->GetMethodID(
                hashmap_cls, "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;");
            for (size_t i = 0; i < v->v.map.count; i++) {
                std::string key_s(v->v.map.entries[i].key.ptr,
                                  v->v.map.entries[i].key.len);
                jstring key_j = env->NewStringUTF(key_s.c_str());
                jobject val_j =
                    WhiskerValueToJObject(env, &v->v.map.entries[i].value);
                jobject prev = env->CallObjectMethod(map, put, key_j, val_j);
                if (prev != nullptr) env->DeleteLocalRef(prev);
                env->DeleteLocalRef(key_j);
                if (val_j != nullptr) env->DeleteLocalRef(val_j);
            }
            env->DeleteLocalRef(hashmap_cls);
            return map;
        }
        case WHISKER_VALUE_ERROR:
        default:
            return nullptr;
    }
}

WhiskerValue JObjectToWhiskerValue(JNIEnv* env, jobject obj) {
    WhiskerValue v;
    std::memset(&v, 0, sizeof(v));
    if (obj == nullptr) {
        v.type = WHISKER_VALUE_NULL;
        return v;
    }
    // Type discovery via `isInstance` checks. Order matters —
    // Boolean before Number because Boolean is NOT a subclass of
    // Number on the JVM but both wrap primitives.
    jclass boolean_cls = env->FindClass("java/lang/Boolean");
    if (env->IsInstanceOf(obj, boolean_cls)) {
        jmethodID m = env->GetMethodID(boolean_cls, "booleanValue", "()Z");
        v.type = WHISKER_VALUE_BOOL;
        v.v.b = env->CallBooleanMethod(obj, m);
        env->DeleteLocalRef(boolean_cls);
        return v;
    }
    env->DeleteLocalRef(boolean_cls);

    jclass float_cls = env->FindClass("java/lang/Float");
    jclass double_cls = env->FindClass("java/lang/Double");
    if (env->IsInstanceOf(obj, float_cls) || env->IsInstanceOf(obj, double_cls)) {
        jclass number_cls = env->FindClass("java/lang/Number");
        jmethodID m = env->GetMethodID(number_cls, "doubleValue", "()D");
        v.type = WHISKER_VALUE_FLOAT;
        v.v.f = env->CallDoubleMethod(obj, m);
        env->DeleteLocalRef(float_cls);
        env->DeleteLocalRef(double_cls);
        env->DeleteLocalRef(number_cls);
        return v;
    }
    env->DeleteLocalRef(float_cls);
    env->DeleteLocalRef(double_cls);

    jclass number_cls = env->FindClass("java/lang/Number");
    if (env->IsInstanceOf(obj, number_cls)) {
        jmethodID m = env->GetMethodID(number_cls, "longValue", "()J");
        v.type = WHISKER_VALUE_INT;
        v.v.i = env->CallLongMethod(obj, m);
        env->DeleteLocalRef(number_cls);
        return v;
    }
    env->DeleteLocalRef(number_cls);

    jclass string_cls = env->FindClass("java/lang/String");
    if (env->IsInstanceOf(obj, string_cls)) {
        jstring js = static_cast<jstring>(obj);
        const char* utf = env->GetStringUTFChars(js, nullptr);
        size_t len = (utf != nullptr) ? std::strlen(utf) : 0;
        char* buf = static_cast<char*>(std::malloc(len + 1));
        if (utf != nullptr) {
            std::memcpy(buf, utf, len + 1);
            env->ReleaseStringUTFChars(js, utf);
        } else {
            buf[0] = '\0';
        }
        v.type = WHISKER_VALUE_STRING;
        v.v.s.ptr = buf;
        v.v.s.len = len;
        env->DeleteLocalRef(string_cls);
        return v;
    }
    env->DeleteLocalRef(string_cls);

    jclass byte_array_cls = env->FindClass("[B");
    if (env->IsInstanceOf(obj, byte_array_cls)) {
        jbyteArray jarr = static_cast<jbyteArray>(obj);
        jsize len = env->GetArrayLength(jarr);
        uint8_t* buf = static_cast<uint8_t*>(std::malloc((size_t)len));
        if (len > 0) {
            env->GetByteArrayRegion(jarr, 0, len,
                                    reinterpret_cast<jbyte*>(buf));
        }
        v.type = WHISKER_VALUE_BYTES;
        v.v.bytes.ptr = buf;
        v.v.bytes.len = (size_t)len;
        env->DeleteLocalRef(byte_array_cls);
        return v;
    }
    env->DeleteLocalRef(byte_array_cls);

    jclass object_array_cls = env->FindClass("[Ljava/lang/Object;");
    if (env->IsInstanceOf(obj, object_array_cls)) {
        jobjectArray jarr = static_cast<jobjectArray>(obj);
        jsize len = env->GetArrayLength(jarr);
        WhiskerValue* items = static_cast<WhiskerValue*>(
            std::malloc((size_t)len * sizeof(WhiskerValue)));
        for (jsize i = 0; i < len; i++) {
            jobject elem = env->GetObjectArrayElement(jarr, i);
            items[i] = JObjectToWhiskerValue(env, elem);
            if (elem != nullptr) env->DeleteLocalRef(elem);
        }
        v.type = WHISKER_VALUE_ARRAY;
        v.v.array.items = items;
        v.v.array.count = (size_t)len;
        env->DeleteLocalRef(object_array_cls);
        return v;
    }
    env->DeleteLocalRef(object_array_cls);

    jclass map_cls = env->FindClass("java/util/Map");
    if (env->IsInstanceOf(obj, map_cls)) {
        jmethodID size_m = env->GetMethodID(map_cls, "size", "()I");
        jmethodID entrySet_m =
            env->GetMethodID(map_cls, "entrySet", "()Ljava/util/Set;");
        jint size = env->CallIntMethod(obj, size_m);
        jobject entry_set = env->CallObjectMethod(obj, entrySet_m);

        jclass set_cls = env->FindClass("java/util/Set");
        jmethodID iter_m =
            env->GetMethodID(set_cls, "iterator", "()Ljava/util/Iterator;");
        jobject iter = env->CallObjectMethod(entry_set, iter_m);

        jclass iter_cls = env->FindClass("java/util/Iterator");
        jmethodID hasnext_m = env->GetMethodID(iter_cls, "hasNext", "()Z");
        jmethodID next_m = env->GetMethodID(iter_cls, "next", "()Ljava/lang/Object;");
        jclass entry_cls = env->FindClass("java/util/Map$Entry");
        jmethodID getkey_m =
            env->GetMethodID(entry_cls, "getKey", "()Ljava/lang/Object;");
        jmethodID getvalue_m =
            env->GetMethodID(entry_cls, "getValue", "()Ljava/lang/Object;");

        WhiskerKeyValue* entries = static_cast<WhiskerKeyValue*>(
            std::malloc((size_t)size * sizeof(WhiskerKeyValue)));
        size_t actual = 0;
        while (env->CallBooleanMethod(iter, hasnext_m)) {
            jobject entry = env->CallObjectMethod(iter, next_m);
            jobject key_obj = env->CallObjectMethod(entry, getkey_m);
            jobject value_obj = env->CallObjectMethod(entry, getvalue_m);
            // Stringify the key — Whisker maps are
            // string-keyed by convention.
            jstring key_str = nullptr;
            if (key_obj != nullptr && env->IsInstanceOf(key_obj, string_cls)) {
                key_str = static_cast<jstring>(key_obj);
            } else if (key_obj != nullptr) {
                jclass kcls = env->GetObjectClass(key_obj);
                jmethodID toString = env->GetMethodID(
                    kcls, "toString", "()Ljava/lang/String;");
                key_str = static_cast<jstring>(
                    env->CallObjectMethod(key_obj, toString));
                env->DeleteLocalRef(kcls);
            }
            if (key_str != nullptr) {
                const char* utf = env->GetStringUTFChars(key_str, nullptr);
                size_t len = (utf != nullptr) ? std::strlen(utf) : 0;
                char* buf = static_cast<char*>(std::malloc(len + 1));
                if (utf != nullptr) {
                    std::memcpy(buf, utf, len + 1);
                    env->ReleaseStringUTFChars(key_str, utf);
                } else {
                    buf[0] = '\0';
                }
                entries[actual].key.ptr = buf;
                entries[actual].key.len = len;
                entries[actual].value = JObjectToWhiskerValue(env, value_obj);
                actual++;
            }
            if (entry != nullptr) env->DeleteLocalRef(entry);
            if (key_obj != nullptr) env->DeleteLocalRef(key_obj);
            if (value_obj != nullptr) env->DeleteLocalRef(value_obj);
        }
        v.type = WHISKER_VALUE_MAP;
        v.v.map.entries = entries;
        v.v.map.count = actual;
        env->DeleteLocalRef(iter);
        env->DeleteLocalRef(entry_set);
        env->DeleteLocalRef(set_cls);
        env->DeleteLocalRef(iter_cls);
        env->DeleteLocalRef(entry_cls);
        env->DeleteLocalRef(map_cls);
        return v;
    }
    env->DeleteLocalRef(map_cls);

    // Unknown type — fall through to null. The Rust proxy reports
    // it as a type mismatch.
    v.type = WHISKER_VALUE_NULL;
    return v;
}

}  // namespace

extern "C" WhiskerValue whisker_bridge_invoke_module(
    const char* module_name,
    const char* method_name,
    const WhiskerValue* args,
    size_t arg_count) {
    if (module_name == nullptr || method_name == nullptr) {
        return WhiskerMakeErrorValueAndroid("module/method name is NULL");
    }
    ScopedJNIEnv env_guard;
    JNIEnv* env = env_guard.get();
    if (env == nullptr) {
        return WhiskerMakeErrorValueAndroid(
            "whisker_bridge_invoke_module: JVM not initialised");
    }

    // Resolve the Kotlin registry's instance lookup once.
    jclass registry_cls =
        env->FindClass("rs/whisker/runtime/WhiskerModuleRegistry");
    if (registry_cls == nullptr) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return WhiskerMakeErrorValueAndroid(
            "WhiskerModuleRegistry class not found");
    }
    jmethodID instance_for_name_m = env->GetStaticMethodID(
        registry_cls, "instanceForName",
        "(Ljava/lang/String;)Ljava/lang/Object;");
    if (instance_for_name_m == nullptr) {
        env->DeleteLocalRef(registry_cls);
        if (env->ExceptionCheck()) env->ExceptionClear();
        return WhiskerMakeErrorValueAndroid(
            "WhiskerModuleRegistry.instanceForName not found");
    }

    jstring jname = env->NewStringUTF(module_name);
    jobject instance =
        env->CallStaticObjectMethod(registry_cls, instance_for_name_m, jname);
    env->DeleteLocalRef(jname);
    env->DeleteLocalRef(registry_cls);

    if (instance == nullptr) {
        return WhiskerMakeErrorValueAndroid("module not registered");
    }

    jclass instance_cls = env->GetObjectClass(instance);
    ModuleMethodCacheEntry cache = GetCachedMethod(env, instance_cls, method_name);
    env->DeleteLocalRef(instance_cls);
    if (cache.method_id == nullptr) {
        env->DeleteLocalRef(instance);
        return WhiskerMakeErrorValueAndroid("module method not found");
    }

    // Build Object[] arg array.
    jclass object_cls = env->FindClass("java/lang/Object");
    jobjectArray args_arr =
        env->NewObjectArray((jsize)arg_count, object_cls, nullptr);
    env->DeleteLocalRef(object_cls);
    for (size_t i = 0; i < arg_count; i++) {
        jobject arg = WhiskerValueToJObject(env, &args[i]);
        env->SetObjectArrayElement(args_arr, (jsize)i, arg);
        if (arg != nullptr) env->DeleteLocalRef(arg);
    }

    // CallObjectMethod (varargs-style) is fine for our single-arg
    // shape — `CallObjectMethodA(env, instance, method_id,
    // jvalueArray)` would be marginally faster for fully-typed
    // args, but with one Object[] arg the varargs overhead is the
    // same.
    jobject result = env->CallObjectMethod(instance, cache.method_id, args_arr);
    env->DeleteLocalRef(args_arr);

    if (env->ExceptionCheck()) {
        env->ExceptionDescribe();
        env->ExceptionClear();
        env->DeleteLocalRef(instance);
        return WhiskerMakeErrorValueAndroid("module method threw an exception");
    }

    WhiskerValue value = JObjectToWhiskerValue(env, result);
    if (result != nullptr) env->DeleteLocalRef(result);
    env->DeleteLocalRef(instance);
    return value;
}

extern "C" bool whisker_bridge_invoke_module_async(
    const char* module_name,
    const char* method_name,
    const WhiskerValue* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (callback == nullptr) return false;
    // Foundation impl — dispatch on the same thread for now.
    // Real off-main-thread async dispatch lives in a follow-up
    // alongside the storage module's async API. The iOS path uses
    // a global-queue dispatch_async; Android will get an
    // equivalent via a thread pool once async semantics are
    // pinned (cancel handles, drop behavior on detached
    // futures).
    WhiskerValue result = whisker_bridge_invoke_module(
        module_name, method_name, args, arg_count);
    callback(user_data, &result);
    whisker_bridge_value_release(&result);
    return true;
}

extern "C" void whisker_bridge_value_release(WhiskerValue* value) {
    if (value == nullptr) return;
    switch (value->type) {
        case WHISKER_VALUE_STRING:
        case WHISKER_VALUE_ERROR:
            std::free(const_cast<char*>(value->v.s.ptr));
            value->v.s.ptr = nullptr;
            value->v.s.len = 0;
            break;
        case WHISKER_VALUE_BYTES:
            std::free(const_cast<uint8_t*>(value->v.bytes.ptr));
            value->v.bytes.ptr = nullptr;
            value->v.bytes.len = 0;
            break;
        case WHISKER_VALUE_ARRAY:
            for (size_t i = 0; i < value->v.array.count; i++) {
                whisker_bridge_value_release(&value->v.array.items[i]);
            }
            std::free(value->v.array.items);
            value->v.array.items = nullptr;
            value->v.array.count = 0;
            break;
        case WHISKER_VALUE_MAP:
            for (size_t i = 0; i < value->v.map.count; i++) {
                std::free(const_cast<char*>(value->v.map.entries[i].key.ptr));
                whisker_bridge_value_release(&value->v.map.entries[i].value);
            }
            std::free(value->v.map.entries);
            value->v.map.entries = nullptr;
            value->v.map.count = 0;
            break;
        default:
            break;
    }
    value->type = WHISKER_VALUE_NULL;
}

#endif  // __ANDROID__
