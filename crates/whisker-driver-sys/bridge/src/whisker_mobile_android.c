#if defined(__ANDROID__)

#include <jni.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "whisker_mobile.h"

typedef struct { jobject surface; void* runtime; } WhiskerAndroidView;

static JavaVM* g_vm;
static jclass g_view_class;
static jmethodID g_request_frame, g_begin_bootstrap, g_register_element, g_finish_bootstrap;
static jmethodID g_present_frame, g_current_revision, g_measure;
static jmethodID g_resource_command, g_invoke_module, g_observe_module;

enum { WHISKER_ANDROID_OPERATION_STRIDE = 10 };

void whisker_mobile_bridge_anchor(void) {}

static JNIEnv* whisker_env(bool* attached) {
    JNIEnv* env = NULL;
    *attached = false;
    if (g_vm == NULL) return NULL;
    if ((*g_vm)->GetEnv(g_vm, (void**)&env, JNI_VERSION_1_6) == JNI_OK) return env;
    if ((*g_vm)->AttachCurrentThread(g_vm, &env, NULL) != JNI_OK) return NULL;
    *attached = true;
    return env;
}

static bool clear_exception(JNIEnv* env) {
    if (!(*env)->ExceptionCheck(env)) return false;
    (*env)->ExceptionDescribe(env);
    (*env)->ExceptionClear(env);
    return true;
}

static jobject local_view(JNIEnv* env, WhiskerAndroidView* view) {
    return view != NULL && view->surface != NULL ? (*env)->NewLocalRef(env, view->surface) : NULL;
}

static jobject utf8_charset(JNIEnv* env) {
    jclass charsets_class = (*env)->FindClass(env, "java/nio/charset/StandardCharsets");
    if (charsets_class == NULL) return NULL;
    jfieldID utf8_field = (*env)->GetStaticFieldID(
        env, charsets_class, "UTF_8", "Ljava/nio/charset/Charset;");
    jobject utf8 = utf8_field == NULL ? NULL : (*env)->GetStaticObjectField(env, charsets_class, utf8_field);
    (*env)->DeleteLocalRef(env, charsets_class);
    return utf8;
}

static jstring new_string(JNIEnv* env, const char* bytes, size_t length) {
    if (length == 0) return (*env)->NewString(env, NULL, 0);
    if (bytes == NULL || length > INT32_MAX) return NULL;
    jbyteArray encoded = (*env)->NewByteArray(env, (jsize)length);
    if (encoded == NULL) return NULL;
    (*env)->SetByteArrayRegion(env, encoded, 0, (jsize)length, (const jbyte*)bytes);
    jclass string_class = (*env)->FindClass(env, "java/lang/String");
    jobject utf8 = utf8_charset(env);
    jmethodID init = string_class == NULL ? NULL : (*env)->GetMethodID(
        env, string_class, "<init>", "([BLjava/nio/charset/Charset;)V");
    jstring result = init == NULL || utf8 == NULL ? NULL
        : (*env)->NewObject(env, string_class, init, encoded, utf8);
    if (utf8) (*env)->DeleteLocalRef(env, utf8);
    if (string_class) (*env)->DeleteLocalRef(env, string_class);
    (*env)->DeleteLocalRef(env, encoded);
    return result;
}

static bool valid_nonempty_string_ref(WhiskerStringRef value) {
    return value.ptr != NULL && value.len > 0 && value.len <= INT32_MAX;
}

static jobjectArray string_refs(JNIEnv* env, const WhiskerStringRef* values, size_t count) {
    if (count > 4096 || (values == NULL) != (count == 0)) return NULL;
    jclass cls = (*env)->FindClass(env, "java/lang/String");
    jobjectArray result = cls == NULL ? NULL : (*env)->NewObjectArray(env, (jsize)count, cls, NULL);
    for (size_t i = 0; result != NULL && i < count; ++i) {
        if (!valid_nonempty_string_ref(values[i])) {
            (*env)->DeleteLocalRef(env, result);
            result = NULL;
            break;
        }
        jstring value = new_string(env, values[i].ptr, values[i].len);
        if (value == NULL) {
            (*env)->DeleteLocalRef(env, result);
            result = NULL;
            break;
        }
        (*env)->SetObjectArrayElement(env, result, (jsize)i, value);
        (*env)->DeleteLocalRef(env, value);
    }
    if (cls) (*env)->DeleteLocalRef(env, cls);
    return result;
}

static jintArray member_ints(JNIEnv* env, const WhiskerMobileMemberRegistration* values,
                             size_t count, bool kinds, bool optional) {
    jintArray result = (*env)->NewIntArray(env, (jsize)count);
    if (result == NULL) return NULL;
    jint* items = malloc((count > 0 ? count : 1) * sizeof(jint));
    if (items == NULL) return result;
    for (size_t i = 0; i < count; ++i) {
        items[i] = kinds ? (optional && !values[i].optional_kind ? -1 : (jint)values[i].value_kind)
                         : (jint)values[i].id;
    }
    if (count > 0) (*env)->SetIntArrayRegion(env, result, 0, (jsize)count, items);
    free(items);
    return result;
}

static jobjectArray member_names(JNIEnv* env, const WhiskerMobileMemberRegistration* values,
                                 size_t count) {
    jclass string_class = (*env)->FindClass(env, "java/lang/String");
    jobjectArray result = (*env)->NewObjectArray(env, (jsize)count, string_class, NULL);
    for (size_t i = 0; i < count; ++i) {
        jstring value = new_string(env, values[i].name.ptr, values[i].name.len);
        (*env)->SetObjectArrayElement(env, result, (jsize)i, value);
        (*env)->DeleteLocalRef(env, value);
    }
    (*env)->DeleteLocalRef(env, string_class);
    return result;
}

static bool bootstrap_host(void* data, const WhiskerMobileBootstrap* bootstrap) {
    if (bootstrap == NULL || bootstrap->abi_major != WHISKER_MOBILE_ABI_MAJOR) return false;
    bool attached; JNIEnv* env = whisker_env(&attached);
    jobject view = env != NULL ? local_view(env, data) : NULL;
    if (view == NULL) return false;
    (*env)->CallVoidMethod(env, view, g_begin_bootstrap);
    for (size_t i = 0; i < bootstrap->registration_count && !clear_exception(env); ++i) {
        const WhiskerMobileElementRegistration* item = &bootstrap->registrations[i];
        jstring name = new_string(env, item->name.ptr, item->name.len);
        jintArray pi = member_ints(env, item->properties, item->property_count, false, false);
        jintArray pk = member_ints(env, item->properties, item->property_count, true, false);
        jobjectArray pn = member_names(env, item->properties, item->property_count);
        jintArray ei = member_ints(env, item->events, item->event_count, false, false);
        jintArray ek = member_ints(env, item->events, item->event_count, true, true);
        jobjectArray en = member_names(env, item->events, item->event_count);
        jintArray ci = member_ints(env, item->commands, item->command_count, false, false);
        jintArray ck = member_ints(env, item->commands, item->command_count, true, false);
        jobjectArray cn = member_names(env, item->commands, item->command_count);
        (*env)->CallVoidMethod(env, view, g_register_element, (jint)item->element_type, name,
            (jint)item->child_policy, (jint)item->measurement, (jint)item->text_style,
            pi, pk, pn, ei, ek, en, ci, ck, cn);
        jobject refs[] = {name, pi, pk, pn, ei, ek, en, ci, ck, cn};
        for (size_t j = 0; j < sizeof(refs) / sizeof(refs[0]); ++j) if (refs[j]) (*env)->DeleteLocalRef(env, refs[j]);
    }
    bool accepted = !clear_exception(env) && (*env)->CallBooleanMethod(env, view, g_finish_bootstrap) == JNI_TRUE;
    if (clear_exception(env)) accepted = false;
    (*env)->DeleteLocalRef(env, view);
    if (attached) (*g_vm)->DetachCurrentThread(g_vm);
    return accepted;
}

static jobject raw_to_value(JNIEnv* env, const WhiskerValueRaw* raw);

static jobject new_value(JNIEnv* env, const char* nested, const char* signature, jvalue* args) {
    char name[128]; snprintf(name, sizeof(name), "rs/whisker/runtime/WhiskerValue$%s", nested);
    jclass cls = (*env)->FindClass(env, name);
    jmethodID init = cls != NULL ? (*env)->GetMethodID(env, cls, "<init>", signature) : NULL;
    jobject value = init != NULL ? (*env)->NewObjectA(env, cls, init, args) : NULL;
    if (cls) (*env)->DeleteLocalRef(env, cls);
    return value;
}

static jobject raw_to_value(JNIEnv* env, const WhiskerValueRaw* raw) {
    if (raw == NULL || raw->type == WHISKER_VALUE_NULL) {
        jclass cls = (*env)->FindClass(env, "rs/whisker/runtime/WhiskerValue$Null");
        jfieldID field = (*env)->GetStaticFieldID(env, cls, "INSTANCE", "Lrs/whisker/runtime/WhiskerValue$Null;");
        jobject value = (*env)->GetStaticObjectField(env, cls, field); (*env)->DeleteLocalRef(env, cls); return value;
    }
    jvalue arg = {0};
    switch (raw->type) {
        case WHISKER_VALUE_BOOL: arg.z = raw->v.b; return new_value(env, "Bool", "(Z)V", &arg);
        case WHISKER_VALUE_INT: arg.j = raw->v.i; return new_value(env, "Int", "(J)V", &arg);
        case WHISKER_VALUE_FLOAT: arg.d = raw->v.f; return new_value(env, "Float", "(D)V", &arg);
        case WHISKER_VALUE_STRING: case WHISKER_VALUE_ERROR: {
            arg.l = new_string(env, raw->v.s.ptr, raw->v.s.len);
            jobject value = new_value(env, raw->type == WHISKER_VALUE_ERROR ? "Err" : "Str", "(Ljava/lang/String;)V", &arg);
            (*env)->DeleteLocalRef(env, arg.l); return value;
        }
        case WHISKER_VALUE_BYTES: {
            jbyteArray bytes = (*env)->NewByteArray(env, (jsize)raw->v.bytes.len);
            if (raw->v.bytes.len) (*env)->SetByteArrayRegion(env, bytes, 0, (jsize)raw->v.bytes.len, (const jbyte*)raw->v.bytes.ptr);
            arg.l = bytes; jobject value = new_value(env, "Bytes", "([B)V", &arg); (*env)->DeleteLocalRef(env, bytes); return value;
        }
        case WHISKER_VALUE_ARRAY: {
            jclass list_cls = (*env)->FindClass(env, "java/util/ArrayList");
            jmethodID init = (*env)->GetMethodID(env, list_cls, "<init>", "(I)V");
            jmethodID add = (*env)->GetMethodID(env, list_cls, "add", "(Ljava/lang/Object;)Z");
            jobject list = (*env)->NewObject(env, list_cls, init, (jint)raw->v.array.count);
            for (size_t i = 0; i < raw->v.array.count; ++i) { jobject item = raw_to_value(env, &raw->v.array.items[i]); (*env)->CallBooleanMethod(env, list, add, item); (*env)->DeleteLocalRef(env, item); }
            arg.l = list; jobject value = new_value(env, "Array", "(Ljava/util/List;)V", &arg);
            (*env)->DeleteLocalRef(env, list); (*env)->DeleteLocalRef(env, list_cls); return value;
        }
        case WHISKER_VALUE_MAP: {
            jclass map_cls = (*env)->FindClass(env, "java/util/LinkedHashMap");
            jmethodID init = (*env)->GetMethodID(env, map_cls, "<init>", "()V");
            jmethodID put = (*env)->GetMethodID(env, map_cls, "put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;");
            jobject map = (*env)->NewObject(env, map_cls, init);
            for (size_t i = 0; i < raw->v.map.count; ++i) { const WhiskerKeyValueRaw* entry = &raw->v.map.entries[i]; jstring key = new_string(env, entry->key.ptr, entry->key.len); jobject item = raw_to_value(env, &entry->value); (*env)->CallObjectMethod(env, map, put, key, item); (*env)->DeleteLocalRef(env, key); (*env)->DeleteLocalRef(env, item); }
            arg.l = map; jobject value = new_value(env, "Map", "(Ljava/util/Map;)V", &arg);
            (*env)->DeleteLocalRef(env, map); (*env)->DeleteLocalRef(env, map_cls); return value;
        }
        default: return raw_to_value(env, NULL);
    }
}

static jfloatArray floats(JNIEnv* env, const float* values, size_t count) {
    if (values == NULL || count == 0) return NULL;
    jfloatArray result = (*env)->NewFloatArray(env, (jsize)count);
    if (result) (*env)->SetFloatArrayRegion(env, result, 0, (jsize)count, values);
    return result;
}

static void append_color(float* out, size_t* cursor, const WhiskerMobileColor* color) {
    out[(*cursor)++] = (float)color->kind; out[(*cursor)++] = (float)color->red;
    out[(*cursor)++] = (float)color->green; out[(*cursor)++] = (float)color->blue;
    out[(*cursor)++] = color->alpha;
}

static jobjectArray color_names(JNIEnv* env, const WhiskerMobileBoxPaint* paint) {
    jclass cls = (*env)->FindClass(env, "java/lang/String");
    jobjectArray result = (*env)->NewObjectArray(env, 5, cls, NULL);
    const WhiskerMobileColor* colors[] = {&paint->background, &paint->colors[0], &paint->colors[1], &paint->colors[2], &paint->colors[3]};
    for (int i = 0; i < 5; ++i) { jstring name = new_string(env, colors[i]->name.ptr, colors[i]->name.len); (*env)->SetObjectArrayElement(env, result, i, name); if (name) (*env)->DeleteLocalRef(env, name); }
    (*env)->DeleteLocalRef(env, cls); return result;
}

static jstring font_setting(JNIEnv* env, const uint8_t tag[4], double value, bool integer) {
    char buffer[64];
    int length = integer
        ? snprintf(buffer, sizeof(buffer), "%.4s=%u", (const char*)tag, (uint32_t)value)
        : snprintf(buffer, sizeof(buffer), "%.4s=%.9g", (const char*)tag, value);
    return length < 0 ? NULL : new_string(env, buffer, (size_t)length);
}

static bool valid_font_tag(const uint8_t tag[4]) {
    for (size_t i = 0; i < 4; ++i) {
        if (tag[i] < 0x20 || tag[i] > 0x7e) return false;
    }
    return true;
}

static bool valid_font_settings(const WhiskerMobileFontFeature* features, size_t feature_count,
                                const WhiskerMobileFontVariation* variations, size_t variation_count) {
    if (feature_count > 4096 || variation_count > 4096 ||
        (features == NULL) != (feature_count == 0) ||
        (variations == NULL) != (variation_count == 0) ||
        feature_count + variation_count > 4096) return false;
    for (size_t i = 0; i < feature_count; ++i) {
        if (!valid_font_tag(features[i].tag)) return false;
    }
    for (size_t i = 0; i < variation_count; ++i) {
        if (!valid_font_tag(variations[i].tag) || !isfinite(variations[i].value)) return false;
    }
    return true;
}

static jobjectArray font_settings(JNIEnv* env,
                                  const WhiskerMobileFontFeature* features, size_t feature_count,
                                  const WhiskerMobileFontVariation* variations, size_t variation_count) {
    if (!valid_font_settings(features, feature_count, variations, variation_count)) return NULL;
    jclass cls = (*env)->FindClass(env, "java/lang/String");
    jobjectArray result = cls == NULL ? NULL : (*env)->NewObjectArray(
        env, (jsize)(feature_count + variation_count), cls, NULL);
    for (size_t i = 0; result != NULL && i < feature_count; ++i) {
        jstring value = font_setting(env, features[i].tag, features[i].value, true);
        (*env)->SetObjectArrayElement(env, result, (jsize)i, value);
        if (value) (*env)->DeleteLocalRef(env, value);
    }
    for (size_t i = 0; result != NULL && i < variation_count; ++i) {
        jstring value = font_setting(env, variations[i].tag, variations[i].value, false);
        (*env)->SetObjectArrayElement(env, result, (jsize)(feature_count + i), value);
        if (value) (*env)->DeleteLocalRef(env, value);
    }
    if (cls) (*env)->DeleteLocalRef(env, cls);
    return result;
}

static bool present_frame(void* data, const WhiskerMobileFrame* frame, WhiskerMobileApplyResponse* response) {
    if (frame == NULL || response == NULL || frame->abi_major != WHISKER_MOBILE_ABI_MAJOR ||
        frame->operation_count > INT32_MAX / WHISKER_ANDROID_OPERATION_STRIDE ||
        (frame->operations == NULL) != (frame->operation_count == 0)) return false;
    bool attached; JNIEnv* env = whisker_env(&attached); jobject view = env != NULL ? local_view(env, data) : NULL;
    if (view == NULL) return false;

    const jsize operation_count = (jsize)frame->operation_count;
    const jsize metadata_count = operation_count * WHISKER_ANDROID_OPERATION_STRIDE;
    jlongArray metadata = (*env)->NewLongArray(env, metadata_count);
    jclass float_array_class = (*env)->FindClass(env, "[F");
    jclass string_class = (*env)->FindClass(env, "java/lang/String");
    jclass string_array_class = (*env)->FindClass(env, "[Ljava/lang/String;");
    jclass value_class = (*env)->FindClass(env, "rs/whisker/runtime/WhiskerValue");
    jobjectArray number_batches = float_array_class == NULL ? NULL
        : (*env)->NewObjectArray(env, operation_count, float_array_class, NULL);
    jobjectArray texts = string_class == NULL ? NULL
        : (*env)->NewObjectArray(env, operation_count, string_class, NULL);
    jobjectArray name_batches = string_array_class == NULL ? NULL
        : (*env)->NewObjectArray(env, operation_count, string_array_class, NULL);
    jobjectArray values = value_class == NULL ? NULL
        : (*env)->NewObjectArray(env, operation_count, value_class, NULL);
    jlongArray result = (*env)->NewLongArray(env, 2);
    jlong* metadata_values = malloc(
        (metadata_count > 0 ? (size_t)metadata_count : 1) * sizeof(jlong));
    bool ok = true;
    if (metadata == NULL || number_batches == NULL || texts == NULL || name_batches == NULL ||
        values == NULL || result == NULL || metadata_values == NULL || clear_exception(env)) ok = false;
    for (size_t i = 0; i < frame->operation_count && ok; ++i) {
        const WhiskerMobileOperation* op = &frame->operations[i];
        jfloatArray numbers = NULL; jstring text = NULL; jobjectArray names = NULL; jobject value = NULL;
        uint32_t staged_flags = op->flags;
        float staged_scalar = op->scalar;
        float storage[64]; size_t count = 0;
        switch (op->tag) {
            case WHISKER_OP_LAYOUT: {
                const WhiskerMobileLayoutGeometry* p = op->payload;
                if (!p) { ok = false; break; }
                float v[] = {p->border.x,p->border.y,p->border.width,p->border.height,p->content.x,p->content.y,p->content.width,p->content.height};
                numbers = floats(env, v, 8); break;
            }
            case WHISKER_OP_PAINT: {
                const WhiskerMobileBoxPaint* p = op->payload; if (!p) { ok = false; break; }
                append_color(storage, &count, &p->background);
                for (int j=0;j<4;++j) { storage[count++]=p->widths[j].length; storage[count++]=p->widths[j].fraction; }
                for (int j=0;j<4;++j) append_color(storage, &count, &p->colors[j]);
                for (int j=0;j<4;++j) { storage[count++]=p->radii_horizontal[j].length; storage[count++]=p->radii_horizontal[j].fraction; }
                for (int j=0;j<4;++j) { storage[count++]=p->radii_vertical[j].length; storage[count++]=p->radii_vertical[j].fraction; }
                for (int j=0;j<4;++j) storage[count++]=(float)p->styles[j];
                numbers = floats(env, storage, count); names = color_names(env, p); break;
            }
            case WHISKER_OP_BOX_SHADOWS: {
                if ((op->payload == NULL) != (op->payload_count == 0) ||
                    op->payload_count > 256) { ok = false; break; }
                const WhiskerMobileBoxShadow* shadows = op->payload;
                const size_t value_count = op->payload_count * 10;
                float* values = malloc((value_count > 0 ? value_count : 1) * sizeof(float));
                jclass cls = (*env)->FindClass(env, "java/lang/String");
                names = cls == NULL ? NULL
                    : (*env)->NewObjectArray(env, (jsize)op->payload_count, cls, NULL);
                if (cls) (*env)->DeleteLocalRef(env, cls);
                if (values == NULL || names == NULL) { free(values); ok = false; break; }
                size_t cursor = 0;
                for (size_t shadow_index = 0; shadow_index < op->payload_count; ++shadow_index) {
                    const WhiskerMobileBoxShadow* shadow = &shadows[shadow_index];
                    values[cursor++] = shadow->offset_x;
                    values[cursor++] = shadow->offset_y;
                    values[cursor++] = shadow->blur_radius;
                    values[cursor++] = shadow->spread_radius;
                    values[cursor++] = (float)shadow->inset;
                    append_color(values, &cursor, &shadow->color);
                    jstring name = new_string(env, shadow->color.name.ptr, shadow->color.name.len);
                    (*env)->SetObjectArrayElement(env, names, (jsize)shadow_index, name);
                    if (name) (*env)->DeleteLocalRef(env, name);
                }
                numbers = floats(env, values, value_count);
                free(values);
                break;
            }
            case WHISKER_OP_CLIP_PATH: {
                if (op->payload == NULL) {
                    if (op->payload_count != 0) ok = false;
                    break;
                }
                if (op->payload_count != 1) { ok = false; break; }
                const WhiskerMobileClipPath* clip = op->payload;
                if (clip->payload == NULL || clip->payload_count != 1) {
                    ok = false; break;
                }
                float *clip_values = storage;
                size_t clip_capacity = 64;
                const WhiskerMobileClipPathCommands *path = NULL;
                if (clip->shape_kind == WHISKER_CLIP_SHAPE_PATH) {
                    path = clip->payload;
                    if (path->commands == NULL || path->command_count == 0 || path->command_count > 4096 ||
                        path->command_count > (SIZE_MAX - 4) / 13) { ok = false; break; }
                    clip_capacity = 4 + path->command_count * 13;
                    clip_values = malloc(clip_capacity * sizeof(float));
                    if (clip_values == NULL) { ok = false; break; }
                }
                clip_values[count++] = (float)clip->reference_box;
                clip_values[count++] = (float)clip->shape_kind;
                if (clip->shape_kind == WHISKER_CLIP_SHAPE_INSET) {
                    const WhiskerMobileClipInset* inset = clip->payload;
                    for (int j=0;j<4;++j) { clip_values[count++]=inset->edges[j].length; clip_values[count++]=inset->edges[j].fraction; }
                    for (int j=0;j<4;++j) { clip_values[count++]=inset->radii_horizontal[j].length; clip_values[count++]=inset->radii_horizontal[j].fraction; }
                    for (int j=0;j<4;++j) { clip_values[count++]=inset->radii_vertical[j].length; clip_values[count++]=inset->radii_vertical[j].fraction; }
                } else if (clip->shape_kind == WHISKER_CLIP_SHAPE_CIRCLE) {
                    const WhiskerMobileClipCircle* circle = clip->payload;
                    const WhiskerMobileLengthPercentage values[] = {circle->radius, circle->center_x, circle->center_y};
                    for (int j=0;j<3;++j) { clip_values[count++]=values[j].length; clip_values[count++]=values[j].fraction; }
                } else if (clip->shape_kind == WHISKER_CLIP_SHAPE_ELLIPSE) {
                    const WhiskerMobileClipEllipse* ellipse = clip->payload;
                    const WhiskerMobileLengthPercentage values[] = {ellipse->radius_x, ellipse->radius_y, ellipse->center_x, ellipse->center_y};
                    for (int j=0;j<4;++j) { clip_values[count++]=values[j].length; clip_values[count++]=values[j].fraction; }
                } else if (clip->shape_kind == WHISKER_CLIP_SHAPE_PATH) {
                    clip_values[count++] = (float)path->fill_rule;
                    clip_values[count++] = (float)path->command_count;
                    for (size_t command_index = 0; command_index < path->command_count; ++command_index) {
                        const WhiskerMobilePathCommand *command = &path->commands[command_index];
                        clip_values[count++] = (float)command->kind;
                        for (int j=0;j<6;++j) {
                            clip_values[count++]=command->points[j].length;
                            clip_values[count++]=command->points[j].fraction;
                        }
                    }
                } else {
                    ok = false; break;
                }
                numbers = floats(env, clip_values, count);
                if (clip_values != storage) free(clip_values);
                break;
            }
            case WHISKER_OP_BACKGROUND_LAYERS: {
                if (op->payload == NULL) {
                    if (op->payload_count != 0) ok = false;
                    staged_flags = WHISKER_BACKGROUND_LINEAR;
                    staged_scalar = 0.0f;
                    break;
                }
                if (op->payload_count == 0 || op->payload_count > 256) { ok = false; break; }
                const WhiskerMobileBackgroundLayer* layers = op->payload;
                const size_t geometry_count = 15;
                const size_t packed_header_count = 3;
                const size_t max_exact_float_integer = 1u << 24;
                size_t total_values = 1;
                size_t total_stops = 0;
                for (size_t layer_index = 0; layer_index < op->payload_count; ++layer_index) {
                    const WhiskerMobileBackgroundLayer* layer = &layers[layer_index];
                    const WhiskerMobileGradientStop* stops = layer->image.payload;
                    size_t stop_count = layer->image.payload_count;
                    size_t image_prefix_count = 0;
                    if (layer->image.kind == WHISKER_BACKGROUND_RADIAL) {
                        if (layer->image.payload == NULL || layer->image.payload_count != 1) {
                            ok = false; break;
                        }
                        const WhiskerMobileRadialGradient* radial = layer->image.payload;
                        stops = radial->stops;
                        stop_count = radial->stop_count;
                        image_prefix_count = 8;
                    } else if (layer->image.kind == WHISKER_BACKGROUND_CONIC) {
                        if (layer->image.payload == NULL || layer->image.payload_count != 1) {
                            ok = false; break;
                        }
                        const WhiskerMobileConicGradient* conic = layer->image.payload;
                        stops = conic->stops;
                        stop_count = conic->stop_count;
                        image_prefix_count = 4;
                    } else if (layer->image.kind == WHISKER_BACKGROUND_RESOURCE) {
                        if (layer->image.payload == NULL || layer->image.payload_count != 1) {
                            ok = false; break;
                        }
                        stops = NULL;
                        stop_count = 0;
                        image_prefix_count = 4;
                    } else if (layer->image.kind != WHISKER_BACKGROUND_LINEAR) {
                        ok = false; break;
                    }
                    size_t prefix_count = geometry_count + image_prefix_count;
                    if ((stops == NULL && stop_count != 0) ||
                        stop_count > (max_exact_float_integer - prefix_count) / 7) {
                        ok = false; break;
                    }
                    size_t value_count = prefix_count + stop_count * 7;
                    if (value_count > max_exact_float_integer ||
                        packed_header_count + value_count > max_exact_float_integer - total_values ||
                        stop_count > INT32_MAX - total_stops) {
                        ok = false; break;
                    }
                    total_values += packed_header_count + value_count;
                    total_stops += stop_count;
                }
                if (!ok) break;
                float* values = malloc(total_values * sizeof(float));
                if (values == NULL) { ok = false; break; }
                jclass string_class = (*env)->FindClass(env, "java/lang/String");
                names = string_class == NULL ? NULL
                    : (*env)->NewObjectArray(env, (jsize)total_stops, string_class, NULL);
                if (string_class) (*env)->DeleteLocalRef(env, string_class);
                if (names == NULL) { free(values); ok = false; break; }
                size_t cursor = 0;
                size_t name_cursor = 0;
                values[cursor++] = (float)op->payload_count;
                for (size_t layer_index = 0; layer_index < op->payload_count; ++layer_index) {
                    const WhiskerMobileBackgroundLayer* layer = &layers[layer_index];
                    const WhiskerMobileGradientStop* stops = layer->image.payload;
                    const WhiskerMobileRadialGradient* radial = NULL;
                    const WhiskerMobileConicGradient* conic = NULL;
                    const uint64_t* resource = NULL;
                    size_t stop_count = layer->image.payload_count;
                    size_t image_prefix_count = 0;
                    if (layer->image.kind == WHISKER_BACKGROUND_RADIAL) {
                        radial = layer->image.payload;
                        stops = radial->stops;
                        stop_count = radial->stop_count;
                        image_prefix_count = 8;
                    } else if (layer->image.kind == WHISKER_BACKGROUND_CONIC) {
                        conic = layer->image.payload;
                        stops = conic->stops;
                        stop_count = conic->stop_count;
                        image_prefix_count = 4;
                    } else if (layer->image.kind == WHISKER_BACKGROUND_RESOURCE) {
                        resource = layer->image.payload;
                        stops = NULL;
                        stop_count = 0;
                        image_prefix_count = 4;
                    }
                    size_t value_count = geometry_count + image_prefix_count + stop_count * 7;
                    values[cursor++] = (float)layer->image.kind;
                    values[cursor++] = layer->image.scalar;
                    values[cursor++] = (float)value_count;
                    const WhiskerMobileLengthPercentage* geometry[] = {
                        &layer->position_x, &layer->position_y,
                        &layer->size_width, &layer->size_height
                    };
                    for (size_t j = 0; j < 4; ++j) {
                        values[cursor++] = geometry[j]->length;
                        values[cursor++] = geometry[j]->fraction;
                    }
                    values[cursor++] = (float)layer->size_kind;
                    values[cursor++] = (float)layer->repeat_x;
                    values[cursor++] = (float)layer->repeat_y;
                    values[cursor++] = (float)layer->origin;
                    values[cursor++] = (float)layer->clip;
                    values[cursor++] = (float)layer->attachment;
                    values[cursor++] = (float)layer->blend_mode;
                    if (radial != NULL) {
                        const WhiskerMobileLengthPercentage* coordinates[] = {
                            &radial->center_x, &radial->center_y,
                            &radial->radius_x, &radial->radius_y
                        };
                        for (size_t j = 0; j < 4; ++j) {
                            values[cursor++] = coordinates[j]->length;
                            values[cursor++] = coordinates[j]->fraction;
                        }
                    } else if (conic != NULL) {
                        const WhiskerMobileLengthPercentage* coordinates[] = {
                            &conic->center_x, &conic->center_y
                        };
                        for (size_t j = 0; j < 2; ++j) {
                            values[cursor++] = coordinates[j]->length;
                            values[cursor++] = coordinates[j]->fraction;
                        }
                    } else if (resource != NULL) {
                        for (size_t j = 0; j < 4; ++j) {
                            values[cursor++] = (float)((*resource >> (j * 16)) & UINT64_C(0xffff));
                        }
                    }
                    for (size_t j = 0; j < stop_count; ++j) {
                        append_color(values, &cursor, &stops[j].color);
                        values[cursor++] = stops[j].position.length;
                        values[cursor++] = stops[j].position.fraction;
                        jstring name = new_string(
                            env, stops[j].color.name.ptr, stops[j].color.name.len);
                        (*env)->SetObjectArrayElement(env, names, (jsize)name_cursor++, name);
                        if (name) (*env)->DeleteLocalRef(env, name);
                    }
                }
                numbers = floats(env, values, total_values);
                staged_flags = 256;
                staged_scalar = 0.0f;
                free(values);
                break;
            }
            case WHISKER_OP_TRANSFORM: numbers = floats(env, op->payload, op->payload_count); break;
            case WHISKER_OP_TEXT: case WHISKER_OP_TEXT_STYLE: {
                const WhiskerMobileText* p = op->payload; if (!p) { ok = false; break; }
                if (p->font_optical_sizing > 1 ||
                    p->direction > 2 || p->alignment > 4 ||
                    p->font_family_count == 0 ||
                    (p->font_families == NULL) != (p->font_family_count == 0) ||
                    (p->font_features == NULL) != (p->font_feature_count == 0) ||
                    (p->font_variations == NULL) != (p->font_variation_count == 0) ||
                    p->font_family_count > 4096 ||
                    p->font_feature_count > 4096 ||
                    p->font_variation_count > 4096 ||
                    p->font_family_count + p->font_feature_count + p->font_variation_count > 4096 ||
                    !valid_font_settings(p->font_features, p->font_feature_count,
                                         p->font_variations, p->font_variation_count)) { ok = false; break; }
                for (size_t j=0;j<p->font_family_count;++j) {
                    if (!valid_nonempty_string_ref(p->font_families[j])) { ok = false; break; }
                }
                if (!ok) break;
                text = new_string(env, p->text.ptr, p->text.len);
                storage[0]=p->font_size; storage[1]=(float)p->font_weight; storage[2]=(float)p->font_style;
                storage[3]=(float)p->color.red; storage[4]=(float)p->color.green; storage[5]=(float)p->color.blue;
                storage[6]=p->color.alpha; storage[7]=(float)p->color.kind;
                storage[8]=(float)p->shadow_flags; storage[9]=p->shadow_offset_x; storage[10]=p->shadow_offset_y;
                storage[11]=p->shadow_blur_radius; storage[12]=(float)p->shadow_color.red;
                storage[13]=(float)p->shadow_color.green; storage[14]=(float)p->shadow_color.blue;
                storage[15]=p->shadow_color.alpha; storage[16]=(float)p->shadow_color.kind;
                storage[17]=(float)p->decoration_flags; storage[18]=(float)p->decoration_style;
                storage[19]=(float)p->decoration_color.red; storage[20]=(float)p->decoration_color.green;
                storage[21]=(float)p->decoration_color.blue; storage[22]=p->decoration_color.alpha;
                storage[23]=(float)p->decoration_color.kind;
                storage[24]=(float)p->alignment;
                storage[25]=p->indent_logical_pixels; storage[26]=p->indent_percentage;
                storage[27]=(float)p->wrap; storage[28]=(float)p->word_break;
                storage[29]=(float)p->max_lines; storage[30]=(float)p->overflow;
                storage[31]=(float)p->font_optical_sizing; storage[32]=(float)p->font_feature_count;
                storage[33]=(float)p->font_family_count;
                storage[34]=p->line_height; storage[35]=p->letter_spacing;
                storage[36]=(float)p->direction;
                numbers = floats(env, storage, 37);
                jclass cls = (*env)->FindClass(env, "java/lang/String"); names = (*env)->NewObjectArray(env, (jsize)(3 + p->font_family_count + p->font_feature_count + p->font_variation_count), cls, NULL);
                jstring color_name = new_string(env, p->color.name.ptr, p->color.name.len); (*env)->SetObjectArrayElement(env, names, 0, color_name);
                jstring shadow_name = new_string(env, p->shadow_color.name.ptr, p->shadow_color.name.len); (*env)->SetObjectArrayElement(env, names, 1, shadow_name);
                jstring decoration_name = new_string(env, p->decoration_color.name.ptr, p->decoration_color.name.len); (*env)->SetObjectArrayElement(env, names, 2, decoration_name);
                for (size_t j=0;j<p->font_family_count;++j) { jstring value=new_string(env,p->font_families[j].ptr,p->font_families[j].len); (*env)->SetObjectArrayElement(env,names,(jsize)(3+j),value); if(value)(*env)->DeleteLocalRef(env,value); }
                for (size_t j=0;j<p->font_feature_count;++j) { jstring value=font_setting(env,p->font_features[j].tag,p->font_features[j].value,true); (*env)->SetObjectArrayElement(env,names,(jsize)(3+p->font_family_count+j),value); if(value)(*env)->DeleteLocalRef(env,value); }
                for (size_t j=0;j<p->font_variation_count;++j) { jstring value=font_setting(env,p->font_variations[j].tag,p->font_variations[j].value,false); (*env)->SetObjectArrayElement(env,names,(jsize)(3+p->font_family_count+p->font_feature_count+j),value); if(value)(*env)->DeleteLocalRef(env,value); }
                if (color_name) (*env)->DeleteLocalRef(env, color_name);
                if (shadow_name) (*env)->DeleteLocalRef(env, shadow_name);
                if (decoration_name) (*env)->DeleteLocalRef(env, decoration_name);
                (*env)->DeleteLocalRef(env, cls); break;
            }
            case WHISKER_OP_PROPERTY: case WHISKER_OP_COMMAND: value = raw_to_value(env, op->payload); break;
        }
        if (!ok) break;
        const size_t offset = i * WHISKER_ANDROID_OPERATION_STRIDE;
        uint32_t scalar_bits;
        memcpy(&scalar_bits, &staged_scalar, sizeof(scalar_bits));
        metadata_values[offset] = (jlong)op->tag;
        metadata_values[offset + 1] = (jlong)staged_flags;
        metadata_values[offset + 2] = (jlong)op->node;
        metadata_values[offset + 3] = (jlong)op->parent;
        metadata_values[offset + 4] = (jlong)op->child;
        metadata_values[offset + 5] = (jlong)op->index;
        metadata_values[offset + 6] = (jlong)op->member;
        metadata_values[offset + 7] = (jlong)op->integer;
        metadata_values[offset + 8] = (jlong)scalar_bits;
        metadata_values[offset + 9] = (jlong)op->wide;
        (*env)->SetObjectArrayElement(env, number_batches, (jsize)i, numbers);
        (*env)->SetObjectArrayElement(env, texts, (jsize)i, text);
        (*env)->SetObjectArrayElement(env, name_batches, (jsize)i, names);
        (*env)->SetObjectArrayElement(env, values, (jsize)i, value);
        if (clear_exception(env)) ok = false;
        if (numbers) (*env)->DeleteLocalRef(env, numbers); if (text) (*env)->DeleteLocalRef(env, text);
        if (names) (*env)->DeleteLocalRef(env, names); if (value) (*env)->DeleteLocalRef(env, value);
    }
    if (ok && metadata_count > 0) {
        (*env)->SetLongArrayRegion(env, metadata, 0, metadata_count, metadata_values);
        if (clear_exception(env)) ok = false;
    }
    if (ok) ok = (*env)->CallBooleanMethod(env, view, g_present_frame,
        (jint)frame->mode, (jint)frame->scene_epoch,
        (jlong)frame->base_revision, (jlong)frame->target_revision,
        metadata, number_batches, texts, name_batches, values, result) == JNI_TRUE &&
        !clear_exception(env);
    if (ok) {
        jlong result_values[2];
        (*env)->GetLongArrayRegion(env, result, 0, 2, result_values);
        ok = !clear_exception(env) && result_values[0] >= WHISKER_APPLY_ACCEPTED &&
            result_values[0] <= WHISKER_APPLY_REJECTED;
        if (ok) {
            response->status = (uint8_t)result_values[0];
            response->revision = (uint64_t)result_values[1];
        }
    }
    if (!ok) {
        response->status = WHISKER_APPLY_REJECTED;
        response->revision = (uint64_t)(*env)->CallLongMethod(env, view, g_current_revision);
        if (clear_exception(env)) {
            ok = false;
        } else {
            ok = true;
        }
    }
    free(metadata_values);
    if (result) (*env)->DeleteLocalRef(env, result);
    if (values) (*env)->DeleteLocalRef(env, values);
    if (name_batches) (*env)->DeleteLocalRef(env, name_batches);
    if (texts) (*env)->DeleteLocalRef(env, texts);
    if (number_batches) (*env)->DeleteLocalRef(env, number_batches);
    if (value_class) (*env)->DeleteLocalRef(env, value_class);
    if (string_array_class) (*env)->DeleteLocalRef(env, string_array_class);
    if (string_class) (*env)->DeleteLocalRef(env, string_class);
    if (float_array_class) (*env)->DeleteLocalRef(env, float_array_class);
    if (metadata) (*env)->DeleteLocalRef(env, metadata);
    (*env)->DeleteLocalRef(env, view); if (attached) (*g_vm)->DetachCurrentThread(g_vm); return ok;
}

static bool measure_host(void* data, const WhiskerMobileMeasureRequest* requests, size_t count,
                         WhiskerMobileMeasureResponse* responses) {
    if (count > 4096 || (requests == NULL) != (count == 0) ||
        (responses == NULL) != (count == 0)) return false;
    bool attached; JNIEnv* env = whisker_env(&attached); jobject view = env != NULL ? local_view(env, data) : NULL;
    if (view == NULL) { if (attached) (*g_vm)->DetachCurrentThread(g_vm); return false; }
    bool ok = true;
    for (size_t i=0;i<count && ok;++i) {
        const WhiskerMobileMeasureRequest* r = &requests[i];
        if (r->text.len > INT32_MAX || (r->text.ptr == NULL) != (r->text.len == 0) ||
            r->locale.len > INT32_MAX || (r->locale.ptr == NULL) != (r->locale.len == 0) ||
            r->payload.len > INT32_MAX || (r->payload.ptr == NULL) != (r->payload.len == 0) ||
            r->font_family_count > 4096 ||
            (r->font_families == NULL) != (r->font_family_count == 0) ||
            (r->kind == WHISKER_MEASURE_TEXT && r->font_family_count == 0) ||
            r->font_feature_count > 4096 ||
            r->font_variation_count > 4096 ||
            r->direction > 2 || r->alignment > 4 ||
            r->font_family_count + r->font_feature_count + r->font_variation_count > 4096) {
            ok = false;
            break;
        }
        jstring text = new_string(env, r->text.ptr, r->text.len);
        jobjectArray families = string_refs(env, r->font_families, r->font_family_count);
        jbyteArray payload = (*env)->NewByteArray(env, (jsize)r->payload.len);
        jobjectArray font_values = font_settings(env, r->font_features, r->font_feature_count,
                                                  r->font_variations, r->font_variation_count);
        if (text == NULL || families == NULL || payload == NULL || font_values == NULL) ok = false;
        if (payload && r->payload.len) (*env)->SetByteArrayRegion(env, payload, 0, (jsize)r->payload.len, (const jbyte*)r->payload.ptr);
        jfloatArray result = ok ? (*env)->CallObjectMethod(env, view, g_measure,
            (jint)r->element_type,(jint)r->kind,r->known_width,r->known_height,(jint)r->known_mask,
            r->available_width,r->available_height,(jint)r->available_width_kind,(jint)r->available_height_kind,
            text,families,r->font_size,(jint)r->font_weight,(jint)r->font_style,(jint)r->wrap,
            (jint)r->word_break,(jint)r->overflow,
            r->letter_spacing,r->line_height,r->indent_logical_pixels,r->indent_percentage,
            (jint)r->max_lines,font_values,(jint)r->font_feature_count,(jint)r->font_optical_sizing,
            (jint)r->payload_version,payload,r->intrinsic_width,r->intrinsic_height,
            (jint)r->intrinsic_mask,(jint)r->direction,(jint)r->alignment) : NULL;
        if (clear_exception(env) || result == NULL || (*env)->GetArrayLength(env, result) < 7) ok = false;
        if (ok) {
            jfloat values[7]; (*env)->GetFloatArrayRegion(env, result, 0, 7, values);
            responses[i].key=r->key; responses[i].environment_epoch=r->environment_epoch;
            responses[i].status=(uint32_t)values[0]; responses[i].reason=(uint32_t)values[1];
            responses[i].width=values[2]; responses[i].height=values[3]; responses[i].first_baseline=values[4];
            responses[i].last_baseline=values[5]; responses[i].metrics_mask=(uint32_t)values[6];
        }
        if (result) (*env)->DeleteLocalRef(env, result); if (payload) (*env)->DeleteLocalRef(env, payload);
        if (font_values) (*env)->DeleteLocalRef(env, font_values);
        if (families) (*env)->DeleteLocalRef(env, families); if (text) (*env)->DeleteLocalRef(env, text);
    }
    (*env)->DeleteLocalRef(env, view); if (attached) (*g_vm)->DetachCurrentThread(g_vm); return ok;
}

static char* copy_utf8(JNIEnv* env, jstring string, size_t* length) {
    *length = 0;
    if (string == NULL) return NULL;
    jclass string_class = (*env)->FindClass(env, "java/lang/String");
    jmethodID get_bytes = string_class == NULL ? NULL : (*env)->GetMethodID(
        env, string_class, "getBytes", "(Ljava/nio/charset/Charset;)[B");
    jobject utf8 = utf8_charset(env);
    jbyteArray encoded = get_bytes == NULL || utf8 == NULL ? NULL
        : (*env)->CallObjectMethod(env, string, get_bytes, utf8);
    jsize count = encoded == NULL ? 0 : (*env)->GetArrayLength(env, encoded);
    char* result = malloc((size_t)count + 1);
    if (result != NULL) {
        if (count > 0) (*env)->GetByteArrayRegion(env, encoded, 0, count, (jbyte*)result);
        result[count] = 0;
        *length = (size_t)count;
    }
    if (encoded) (*env)->DeleteLocalRef(env, encoded);
    if (utf8) (*env)->DeleteLocalRef(env, utf8);
    if (string_class) (*env)->DeleteLocalRef(env, string_class);
    return result;
}

static WhiskerValueRaw object_to_raw(JNIEnv* env, jobject value);
static void release_raw(WhiskerValueRaw* value) {
    if (!value) return;
    if (value->type == WHISKER_VALUE_STRING || value->type == WHISKER_VALUE_ERROR) free((void*)value->v.s.ptr);
    else if (value->type == WHISKER_VALUE_BYTES) free((void*)value->v.bytes.ptr);
    else if (value->type == WHISKER_VALUE_ARRAY) { for(size_t i=0;i<value->v.array.count;++i) release_raw(&value->v.array.items[i]); free(value->v.array.items); }
    else if (value->type == WHISKER_VALUE_MAP) { for(size_t i=0;i<value->v.map.count;++i) { free((void*)value->v.map.entries[i].key.ptr); release_raw(&value->v.map.entries[i].value); } free(value->v.map.entries); }
    memset(value, 0, sizeof(*value));
}

static WhiskerValueRaw object_to_raw(JNIEnv* env, jobject value) {
    WhiskerValueRaw out; memset(&out, 0, sizeof(out));
    if (value == NULL) return out;
    jclass value_cls = (*env)->FindClass(env, "rs/whisker/runtime/WhiskerValue");
    if ((*env)->IsInstanceOf(env, value, value_cls)) {
        jclass error_cls = (*env)->FindClass(env, "rs/whisker/runtime/WhiskerValue$Err");
        if ((*env)->IsInstanceOf(env, value, error_cls)) {
            jmethodID message = (*env)->GetMethodID(env, error_cls, "getMessage", "()Ljava/lang/String;");
            jstring string = (*env)->CallObjectMethod(env, value, message);
            out.type = WHISKER_VALUE_ERROR;
            out.v.s.ptr = copy_utf8(env, string, &out.v.s.len);
            (*env)->DeleteLocalRef(env, string); (*env)->DeleteLocalRef(env, error_cls);
            (*env)->DeleteLocalRef(env, value_cls); return out;
        }
        (*env)->DeleteLocalRef(env, error_cls);
        jclass helper = (*env)->FindClass(env, "rs/whisker/runtime/WhiskerValueKt");
        jmethodID convert = (*env)->GetStaticMethodID(env, helper, "toJavaObject", "(Lrs/whisker/runtime/WhiskerValue;)Ljava/lang/Object;");
        jobject converted = (*env)->CallStaticObjectMethod(env, helper, convert, value);
        (*env)->DeleteLocalRef(env, helper); (*env)->DeleteLocalRef(env, value_cls);
        out = object_to_raw(env, converted); if (converted) (*env)->DeleteLocalRef(env, converted); return out;
    }
    (*env)->DeleteLocalRef(env, value_cls);
    jclass string_cls=(*env)->FindClass(env,"java/lang/String");
    jclass bool_cls=(*env)->FindClass(env,"java/lang/Boolean");
    jclass long_cls=(*env)->FindClass(env,"java/lang/Long");
    jclass double_cls=(*env)->FindClass(env,"java/lang/Double");
    jclass bytes_cls=(*env)->FindClass(env,"[B");
    jclass list_cls=(*env)->FindClass(env,"java/util/List");
    jclass map_cls=(*env)->FindClass(env,"java/util/Map");
    if ((*env)->IsInstanceOf(env,value,string_cls)) {
        out.type=WHISKER_VALUE_STRING; out.v.s.ptr=copy_utf8(env,(jstring)value,&out.v.s.len);
    } else if ((*env)->IsInstanceOf(env,value,bool_cls)) {
        jmethodID method=(*env)->GetMethodID(env,bool_cls,"booleanValue","()Z"); out.type=WHISKER_VALUE_BOOL; out.v.b=(*env)->CallBooleanMethod(env,value,method);
    } else if ((*env)->IsInstanceOf(env,value,long_cls)) {
        jmethodID method=(*env)->GetMethodID(env,long_cls,"longValue","()J"); out.type=WHISKER_VALUE_INT; out.v.i=(*env)->CallLongMethod(env,value,method);
    } else if ((*env)->IsInstanceOf(env,value,double_cls)) {
        jmethodID method=(*env)->GetMethodID(env,double_cls,"doubleValue","()D"); out.type=WHISKER_VALUE_FLOAT; out.v.f=(*env)->CallDoubleMethod(env,value,method);
    } else if ((*env)->IsInstanceOf(env,value,bytes_cls)) {
        jsize count=(*env)->GetArrayLength(env,(jarray)value); uint8_t* bytes=malloc(count > 0 ? (size_t)count : 1);
        if(count) (*env)->GetByteArrayRegion(env,(jbyteArray)value,0,count,(jbyte*)bytes); out.type=WHISKER_VALUE_BYTES; out.v.bytes.ptr=bytes; out.v.bytes.len=(size_t)count;
    } else if ((*env)->IsInstanceOf(env,value,list_cls)) {
        jmethodID size=(*env)->GetMethodID(env,list_cls,"size","()I"), get=(*env)->GetMethodID(env,list_cls,"get","(I)Ljava/lang/Object;");
        jint count=(*env)->CallIntMethod(env,value,size); WhiskerValueRaw* items=calloc(count > 0 ? (size_t)count : 1,sizeof(*items));
        for(jint i=0;i<count;++i){ jobject item=(*env)->CallObjectMethod(env,value,get,i); items[i]=object_to_raw(env,item); if(item)(*env)->DeleteLocalRef(env,item); }
        out.type=WHISKER_VALUE_ARRAY; out.v.array.items=items; out.v.array.count=(size_t)count;
    } else if ((*env)->IsInstanceOf(env,value,map_cls)) {
        jmethodID entry_set=(*env)->GetMethodID(env,map_cls,"entrySet","()Ljava/util/Set;"); jobject set=(*env)->CallObjectMethod(env,value,entry_set);
        jclass collection_cls=(*env)->FindClass(env,"java/util/Collection"); jmethodID to_array=(*env)->GetMethodID(env,collection_cls,"toArray","()[Ljava/lang/Object;");
        jobjectArray array=(*env)->CallObjectMethod(env,set,to_array); jsize count=(*env)->GetArrayLength(env,array); WhiskerKeyValueRaw* entries=calloc(count > 0 ? (size_t)count : 1,sizeof(*entries));
        jclass entry_cls=(*env)->FindClass(env,"java/util/Map$Entry"); jmethodID get_key=(*env)->GetMethodID(env,entry_cls,"getKey","()Ljava/lang/Object;"),get_value=(*env)->GetMethodID(env,entry_cls,"getValue","()Ljava/lang/Object;");
        for(jsize i=0;i<count;++i){ jobject entry=(*env)->GetObjectArrayElement(env,array,i),key=(*env)->CallObjectMethod(env,entry,get_key),item=(*env)->CallObjectMethod(env,entry,get_value); entries[i].key.ptr=copy_utf8(env,(jstring)key,&entries[i].key.len); entries[i].value=object_to_raw(env,item); (*env)->DeleteLocalRef(env,item);(*env)->DeleteLocalRef(env,key);(*env)->DeleteLocalRef(env,entry); }
        out.type=WHISKER_VALUE_MAP; out.v.map.entries=entries; out.v.map.count=(size_t)count;
        (*env)->DeleteLocalRef(env,entry_cls);(*env)->DeleteLocalRef(env,array);(*env)->DeleteLocalRef(env,collection_cls);(*env)->DeleteLocalRef(env,set);
    }
    jobject refs[]={string_cls,bool_cls,long_cls,double_cls,bytes_cls,list_cls,map_cls}; for(size_t i=0;i<7;++i)(*env)->DeleteLocalRef(env,refs[i]); return out;
}

static void request_frame(void* data) {
    bool attached; JNIEnv* env=whisker_env(&attached); jobject view=env?local_view(env,data):NULL;
    if(view){(*env)->CallVoidMethod(env,view,g_request_frame);clear_exception(env);(*env)->DeleteLocalRef(env,view);} if(attached)(*g_vm)->DetachCurrentThread(g_vm);
}

static bool resource_command(void* data, const WhiskerMobileResourceCommand* command) {
    if (command == NULL || command->identifier.len > INT32_MAX || command->data.len > INT32_MAX ||
        (command->identifier.len > 0 && command->identifier.ptr == NULL) ||
        (command->data.len > 0 && command->data.ptr == NULL)) return false;
    bool attached; JNIEnv* env=whisker_env(&attached); jobject view=env?local_view(env,data):NULL;
    if(!view){if(attached)(*g_vm)->DetachCurrentThread(g_vm);return false;}
    jstring identifier=new_string(env,command->identifier.ptr,command->identifier.len);
    jbyteArray bytes=(*env)->NewByteArray(env,(jsize)command->data.len);
    if(bytes&&command->data.len)(*env)->SetByteArrayRegion(
        env,bytes,0,(jsize)command->data.len,(const jbyte*)command->data.ptr);
    bool accepted=identifier&&bytes&&(*env)->CallBooleanMethod(
        env,view,g_resource_command,(jint)command->command,(jint)command->kind,
        (jint)command->source,(jlong)command->resource,(jlong)command->generation,
        identifier,bytes)==JNI_TRUE&&!clear_exception(env);
    if(bytes)(*env)->DeleteLocalRef(env,bytes);if(identifier)(*env)->DeleteLocalRef(env,identifier);
    (*env)->DeleteLocalRef(env,view);if(attached)(*g_vm)->DetachCurrentThread(g_vm);return accepted;
}

static bool invoke_module(void* data,const uint8_t* module,size_t module_len,const uint8_t* method,size_t method_len,
                          const WhiskerValueRaw* args,size_t arg_count,bool async,
                          WhiskerMobileModuleResultCallback result,void* result_data) {
    bool attached; JNIEnv* env=whisker_env(&attached); jobject view=env?local_view(env,data):NULL; if(!view)return false;
    jstring m=new_string(env,(const char*)module,module_len),f=new_string(env,(const char*)method,method_len);
    jclass value_cls=(*env)->FindClass(env,"rs/whisker/runtime/WhiskerValue"); jobjectArray array=(*env)->NewObjectArray(env,(jsize)arg_count,value_cls,NULL);
    for(size_t i=0;i<arg_count;++i){jobject item=raw_to_value(env,&args[i]);(*env)->SetObjectArrayElement(env,array,(jsize)i,item);(*env)->DeleteLocalRef(env,item);}
    bool accepted=(*env)->CallBooleanMethod(env,view,g_invoke_module,m,f,array,async?JNI_TRUE:JNI_FALSE,(jlong)(uintptr_t)result,(jlong)(uintptr_t)result_data)==JNI_TRUE&&!clear_exception(env);
    (*env)->DeleteLocalRef(env,array);(*env)->DeleteLocalRef(env,value_cls);(*env)->DeleteLocalRef(env,f);(*env)->DeleteLocalRef(env,m);(*env)->DeleteLocalRef(env,view);if(attached)(*g_vm)->DetachCurrentThread(g_vm);return accepted;
}

static void observe_module(void* data,const uint8_t* module,size_t module_len,const uint8_t* event,size_t event_len,bool observing){
    bool attached;JNIEnv* env=whisker_env(&attached);jobject view=env?local_view(env,data):NULL;if(!view)return;jstring m=new_string(env,(const char*)module,module_len),e=new_string(env,(const char*)event,event_len);(*env)->CallVoidMethod(env,view,g_observe_module,m,e,observing?JNI_TRUE:JNI_FALSE);clear_exception(env);(*env)->DeleteLocalRef(env,e);(*env)->DeleteLocalRef(env,m);(*env)->DeleteLocalRef(env,view);if(attached)(*g_vm)->DetachCurrentThread(g_vm);
}

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM* vm, void* reserved) {
    (void)reserved; JNIEnv* env=NULL; g_vm=vm;
    if((*vm)->GetEnv(vm,(void**)&env,JNI_VERSION_1_6)!=JNI_OK)return JNI_ERR;
    jclass local=(*env)->FindClass(env,"rs/whisker/runtime/WhiskerView"); if(!local)return JNI_ERR;
    g_view_class=(*env)->NewGlobalRef(env,local);(*env)->DeleteLocalRef(env,local);
#define METHOD(slot,name,sig) slot=(*env)->GetMethodID(env,g_view_class,name,sig); if(!slot){clear_exception(env);return JNI_ERR;}
    METHOD(g_request_frame,"requestFrameFromNative","()V")
    METHOD(g_begin_bootstrap,"beginBootstrapFromNative","()V")
    METHOD(g_register_element,"registerElementFromNative","(ILjava/lang/String;III[I[I[Ljava/lang/String;[I[I[Ljava/lang/String;[I[I[Ljava/lang/String;)V")
    METHOD(g_finish_bootstrap,"finishBootstrapFromNative","()Z")
    METHOD(g_present_frame,"presentFrameFromNative","(IIJJ[J[[F[Ljava/lang/String;[[Ljava/lang/String;[Lrs/whisker/runtime/WhiskerValue;[J)Z")
    METHOD(g_current_revision,"currentRevisionFromNative","()J")
    METHOD(g_measure,"measureFromNative","(IIFFIFFIILjava/lang/String;[Ljava/lang/String;FIIIIIFFFFI[Ljava/lang/String;III[BFFIII)[F")
    METHOD(g_resource_command,"resourceCommandFromNative","(IIIJJLjava/lang/String;[B)Z")
    METHOD(g_invoke_module,"invokeModuleFromNative","(Ljava/lang/String;Ljava/lang/String;[Lrs/whisker/runtime/WhiskerValue;ZJJ)Z")
    METHOD(g_observe_module,"observeModuleFromNative","(Ljava/lang/String;Ljava/lang/String;Z)V")
#undef METHOD
    return JNI_VERSION_1_6;
}

JNIEXPORT jlong JNICALL Java_rs_whisker_runtime_WhiskerView_nativeCreate(JNIEnv* env,jobject self,jfloat width,jfloat height,jfloat scale){
    WhiskerAndroidView* view=calloc(1,sizeof(*view));if(!view)return 0;view->surface=(*env)->NewGlobalRef(env,self);if(!view->surface){free(view);return 0;}
    view->runtime=whisker_view_create(width,height,scale,request_frame,view,bootstrap_host,view,measure_host,view,present_frame,view,resource_command,view,invoke_module,observe_module,view);
    if(!view->runtime){(*env)->DeleteGlobalRef(env,view->surface);free(view);return 0;}return(jlong)(uintptr_t)view;
}
JNIEXPORT jboolean JNICALL Java_rs_whisker_runtime_WhiskerView_nativeTick(JNIEnv* env,jobject self,jlong handle,jdouble timestamp,jfloat width,jfloat height,jfloat scale){(void)env;(void)self;WhiskerAndroidView* view=(void*)(uintptr_t)handle;return view&&view->runtime&&whisker_view_tick(view->runtime,timestamp,width,height,scale)?JNI_TRUE:JNI_FALSE;}
JNIEXPORT void JNICALL Java_rs_whisker_runtime_WhiskerView_nativeDestroy(JNIEnv* env,jobject self,jlong handle){(void)self;WhiskerAndroidView* view=(void*)(uintptr_t)handle;if(!view)return;if(view->runtime)whisker_view_destroy(view->runtime);if(view->surface)(*env)->DeleteGlobalRef(env,view->surface);free(view);}

JNIEXPORT jboolean JNICALL Java_rs_whisker_runtime_WhiskerView_nativeDispatchEvent(JNIEnv* env,jobject self,jlong handle,jlong node,jstring name,jobject detail,jdouble timestamp){
    (void)self;WhiskerAndroidView* view=(void*)(uintptr_t)handle;if(!view||!view->runtime||!name)return JNI_FALSE;
    size_t name_len=0;char* bytes=copy_utf8(env,name,&name_len);WhiskerValueRaw raw=object_to_raw(env,detail);
    bool consumed=bytes&&whisker_view_dispatch_event(view->runtime,timestamp,(uint64_t)node,(const uint8_t*)bytes,name_len,&raw);
    release_raw(&raw);free(bytes);return consumed?JNI_TRUE:JNI_FALSE;
}

JNIEXPORT jboolean JNICALL Java_rs_whisker_runtime_WhiskerView_nativeDispatchPointer(JNIEnv* env,jobject self,jlong handle,jdouble timestamp,jint event,jlong pointer_id,jint pointer_kind,jfloat x,jfloat y,jint buttons,jint changed_button){
    (void)env;(void)self;WhiskerAndroidView* view=(void*)(uintptr_t)handle;
    if(!view||!view->runtime||pointer_id<=0||buttons<0||changed_button<INT16_MIN||changed_button>INT16_MAX)return JNI_FALSE;
    return whisker_view_dispatch_pointer(view->runtime,timestamp,(uint32_t)event,(uint64_t)pointer_id,
        (uint32_t)pointer_kind,x,y,(uint32_t)buttons,(int16_t)changed_button)?JNI_TRUE:JNI_FALSE;
}

JNIEXPORT void JNICALL Java_rs_whisker_runtime_WhiskerView_nativeResolveModule(JNIEnv* env,jobject self,jlong callback_ptr,jlong data_ptr,jobject payload){
    (void)self;WhiskerMobileModuleResultCallback callback=(void*)(uintptr_t)callback_ptr;if(!callback)return;WhiskerValueRaw raw=object_to_raw(env,payload);callback((void*)(uintptr_t)data_ptr,&raw);release_raw(&raw);
}

JNIEXPORT jboolean JNICALL Java_rs_whisker_runtime_WhiskerView_nativeDispatchModuleEvent(JNIEnv* env,jobject self,jlong handle,jstring module,jstring event,jobject payload){
    (void)self;WhiskerAndroidView* view=(void*)(uintptr_t)handle;if(!view||!view->runtime||!module||!event)return JNI_FALSE;
    size_t m_len=0,e_len=0;char* m=copy_utf8(env,module,&m_len);char* e=copy_utf8(env,event,&e_len);WhiskerValueRaw raw=object_to_raw(env,payload);
    bool consumed=m&&e&&whisker_view_dispatch_module_event(view->runtime,(const uint8_t*)m,m_len,(const uint8_t*)e,e_len,&raw);
    release_raw(&raw);free(e);free(m);return consumed?JNI_TRUE:JNI_FALSE;
}

JNIEXPORT jboolean JNICALL Java_rs_whisker_runtime_WhiskerView_nativeDispatchResourceEvent(JNIEnv* env,jobject self,jlong handle,jint status,jint failure_code,jlong resource,jlong generation,jfloat width,jfloat height,jfloat scale,jint dimensions_mask,jstring diagnostic){
    (void)self;WhiskerAndroidView* view=(void*)(uintptr_t)handle;if(!view||!view->runtime||!diagnostic)return JNI_FALSE;
    size_t diagnostic_len=0;char* diagnostic_bytes=copy_utf8(env,diagnostic,&diagnostic_len);
    if(!diagnostic_bytes)return JNI_FALSE;
    WhiskerMobileResourceEvent event={
        .status=(uint32_t)status,.failure_code=(uint32_t)failure_code,
        .resource=(uint64_t)resource,.generation=(uint64_t)generation,
        .width=width,.height=height,.scale=scale,.dimensions_mask=(uint32_t)dimensions_mask,
        .diagnostic={.ptr=diagnostic_bytes,.len=diagnostic_len}
    };
    bool consumed=whisker_view_dispatch_resource_event(view->runtime,&event);
    free(diagnostic_bytes);return consumed?JNI_TRUE:JNI_FALSE;
}

#endif
