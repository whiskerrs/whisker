// Lynx C ABI — function pointer dispatch surface.
//
// Step 6 of the build-decoupling pivot: the bridge no longer link-time
// resolves Lynx's `lynx_*` symbols (via `-framework Lynx` on iOS or
// `-llynx` on Android). Instead, this header declares one function
// pointer typedef per Lynx C ABI entry point, plus a dispatch struct
// `WhiskerLynxCapi`. `whisker_bridge_load_lynx()` dlopens Lynx and
// populates the struct via `dlsym`; bridge code calls through
// `whisker_lynx_capi()->fn(args)`.
//
// This lets `cargo build --target=aarch64-{linux-android,apple-ios}`
// succeed cold-start — no prior `whisker build` to fetch and stage
// the Lynx artifact tree.
//
// MUST stay in sync with whiskerrs/lynx:core/native_renderer_capi/
// public/lynx_native_renderer_capi.h. ABI mismatches are caught at
// runtime by `lynx_capi_abi_version()` (the first symbol the loader
// binds) — bump `WHISKER_LYNX_CAPI_ABI_VERSION` here in lockstep with
// the fork-side `kLynxCapiAbiVersion`.

#ifndef WHISKER_BRIDGE_LYNX_CAPI_H_
#define WHISKER_BRIDGE_LYNX_CAPI_H_

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ----- ABI version contract -------------------------------------------------

// Whisker refuses to load a Lynx whose ABI version differs from this.
// Bumped in lockstep with the fork's `kLynxCapiAbiVersion` whenever the
// C ABI changes incompatibly.
#define WHISKER_LYNX_CAPI_ABI_VERSION 2

// ----- Opaque handle types --------------------------------------------------

typedef struct lynx_shell_t lynx_shell_t;
typedef struct lynx_fiber_element_t lynx_fiber_element_t;

// ----- Element tag enum -----------------------------------------------------

typedef enum lynx_element_tag_e {
  LYNX_ELEMENT_TAG_PAGE = 0,
  LYNX_ELEMENT_TAG_VIEW = 1,
  LYNX_ELEMENT_TAG_TEXT = 2,
  LYNX_ELEMENT_TAG_RAW_TEXT = 3,
  LYNX_ELEMENT_TAG_IMAGE = 4,
  LYNX_ELEMENT_TAG_SCROLL_VIEW = 5,
} lynx_element_tag_e;

// ----- List native item provider --------------------------------------------

// Matches `lynx::tasm::list::kInvalidIndex`. Must NOT be 0 — that's a
// real `impl_id` and would be silently consumed by the C++ list as a
// missing-node lookup instead of "skip this slot".
#define LYNX_LIST_INVALID_INDEX (-1)

// Callback shapes — these are passed BY the embedder INTO Lynx, so they
// stay direct C function pointer types (Lynx invokes them; the embedder
// doesn't dispatch through any table).
typedef void (*lynx_tasm_callback_t)(void* user_data);
typedef int32_t (*lynx_list_component_at_index_fn)(uint32_t index,
                                                    int64_t operation_id,
                                                    int reuse_notification,
                                                    void* user_data);
typedef void (*lynx_list_enqueue_component_fn)(int32_t sign, void* user_data);
typedef void (*lynx_user_data_free_fn)(void* user_data);

// ----- UI method value tree -------------------------------------------------
//
// Mirrors the fork's struct layout. Used both for args (scalars only)
// and for async result trees (recursive arrays / maps). See the upstream
// header commentary in the Lynx fork for the full ownership contract.

typedef enum lynx_ui_method_value_type_e {
  LYNX_UI_METHOD_VALUE_NULL = 0,
  LYNX_UI_METHOD_VALUE_BOOL = 1,
  LYNX_UI_METHOD_VALUE_INT = 2,
  LYNX_UI_METHOD_VALUE_DOUBLE = 3,
  LYNX_UI_METHOD_VALUE_STRING = 4,
  LYNX_UI_METHOD_VALUE_ARRAY = 5,
  LYNX_UI_METHOD_VALUE_MAP = 6,
} lynx_ui_method_value_type_e;

struct lynx_ui_method_value_t;
struct lynx_ui_method_kv_t;

typedef struct lynx_ui_method_value_array_t {
  struct lynx_ui_method_value_t* items;  // length = count
  size_t count;
} lynx_ui_method_value_array_t;

typedef struct lynx_ui_method_value_map_t {
  struct lynx_ui_method_kv_t* entries;  // length = count
  size_t count;
} lynx_ui_method_value_map_t;

typedef struct lynx_ui_method_value_t {
  lynx_ui_method_value_type_e type;
  union {
    bool b;
    int64_t i;
    double f;
    const char* s;  // NUL-terminated UTF-8
    lynx_ui_method_value_array_t array;
    lynx_ui_method_value_map_t map;
  } v;
} lynx_ui_method_value_t;

typedef struct lynx_ui_method_kv_t {
  const char* key;  // NUL-terminated UTF-8
  lynx_ui_method_value_t value;
} lynx_ui_method_kv_t;

// Callback shape for the async UI-method result path. Embedder passes
// it INTO Lynx, so direct function pointer (not dispatched).
typedef void (*lynx_ui_method_result_cb)(int32_t code,
                                          const lynx_ui_method_value_t* result,
                                          void* user_data);

// ----- Function pointer typedefs --------------------------------------------
//
// One per Lynx C ABI entry point. The bridge code calls these through
// the `WhiskerLynxCapi` dispatch table below; this header has zero
// `extern` declarations for the lynx_* symbols themselves, so bridge
// .o files never carry UND refs to them.

typedef int32_t (*lynx_capi_abi_version_fn)(void);

typedef lynx_shell_t* (*lynx_shell_from_native_ptr_fn)(void* native_shell_ptr);
typedef void (*lynx_shell_release_fn)(lynx_shell_t* shell);
typedef bool (*lynx_shell_run_on_tasm_thread_fn)(lynx_shell_t* shell,
                                                  lynx_tasm_callback_t callback,
                                                  void* user_data);

typedef lynx_fiber_element_t* (*lynx_create_fiber_element_fn)(lynx_shell_t* shell,
                                                               lynx_element_tag_e tag);
typedef lynx_fiber_element_t* (*lynx_create_fiber_element_by_name_fn)(
    lynx_shell_t* shell,
    const char* tag_name);
typedef void (*lynx_element_release_fn)(lynx_fiber_element_t* element);
typedef int32_t (*lynx_element_id_fn)(lynx_fiber_element_t* element);

typedef void (*lynx_element_set_attribute_fn)(lynx_fiber_element_t* element,
                                                const char* key,
                                                const char* value);
typedef void (*lynx_element_set_attribute_int_fn)(lynx_fiber_element_t* element,
                                                    const char* key,
                                                    int64_t value);
typedef void (*lynx_element_set_attribute_bool_fn)(lynx_fiber_element_t* element,
                                                    const char* key,
                                                    bool value);
typedef void (*lynx_element_set_attribute_double_fn)(lynx_fiber_element_t* element,
                                                      const char* key,
                                                      double value);
// Object-valued attribute (`{obj_keys[i]: obj_values[i]}` of doubles) —
// for props like `<list>` `item-snap` {factor, offset}.
typedef void (*lynx_element_set_attribute_object_fn)(lynx_fiber_element_t* element,
                                                      const char* key,
                                                      const char* const* obj_keys,
                                                      const double* obj_values,
                                                      int32_t obj_count);
typedef void (*lynx_element_set_inline_styles_fn)(lynx_fiber_element_t* element,
                                                    const char* css);
// Decoupled `<list>` data source: `item_keys[0..count)` are the real
// (stable) item-keys in current order; the parallel arrays carry per-item
// metadata (may be null); `prev_count` is the previous item count (full
// replace). ABI v2.
typedef void (*lynx_element_set_update_list_info_fn)(lynx_fiber_element_t* element,
                                                      int32_t prev_count,
                                                      const char* const* item_keys,
                                                      const int32_t* estimated_main_axis_px,
                                                      const uint8_t* full_span,
                                                      const uint8_t* sticky_top,
                                                      const uint8_t* sticky_bottom,
                                                      const uint8_t* recyclable,
                                                      int32_t count);
// Explicit diff actions for the decoupled `<list>` data source —
// minimal-action alternative to `lynx_element_set_update_list_info`.
// Removals: ascending pre-update indices, applied first. Inserts:
// ascending splice points into the post-removal list.
typedef void (*lynx_element_update_list_actions_fn)(
    lynx_fiber_element_t* element,
    const int32_t* remove_indices,
    int32_t remove_count,
    const int32_t* insert_positions,
    const char* const* insert_keys,
    int32_t insert_count);
typedef void (*lynx_element_set_event_handler_fn)(lynx_fiber_element_t* element,
                                                    const char* event_name);
typedef void (*lynx_element_append_child_fn)(lynx_fiber_element_t* parent,
                                              lynx_fiber_element_t* child);
typedef void (*lynx_element_remove_child_fn)(lynx_fiber_element_t* parent,
                                              lynx_fiber_element_t* child);
typedef void (*lynx_list_set_native_item_provider_fn)(
    lynx_fiber_element_t* element,
    lynx_list_component_at_index_fn component_at_index,
    lynx_list_enqueue_component_fn enqueue_component,
    void* user_data,
    lynx_user_data_free_fn user_data_free);

typedef void (*lynx_shell_set_root_element_fn)(lynx_shell_t* shell,
                                                lynx_fiber_element_t* page);
typedef void (*lynx_shell_flush_fn)(lynx_shell_t* shell);

typedef int32_t (*lynx_ui_invoke_method_fn)(lynx_shell_t* shell,
                                              int32_t sign,
                                              const char* method_name,
                                              const lynx_ui_method_value_t* args,
                                              size_t arg_count);
typedef int32_t (*lynx_ui_invoke_method_with_params_fn)(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* params);
typedef int32_t (*lynx_ui_invoke_method_async_fn)(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* args,
    size_t arg_count,
    lynx_ui_method_result_cb callback,
    void* user_data);
typedef int32_t (*lynx_ui_invoke_method_async_with_params_fn)(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* params,
    lynx_ui_method_result_cb callback,
    void* user_data);

typedef int32_t (*lynx_element_animate_fn)(
    lynx_shell_t* shell,
    lynx_fiber_element_t* element,
    int32_t operation,
    const char* animation_name,
    const lynx_ui_method_value_t* keyframes,
    const lynx_ui_method_value_t* options);

// Core-originated custom events (the `<list>` family + `<frame>`).
// `params` is the event payload (`detail`), valid only for the
// duration of the call. Return true to consume (skip the JS-path
// forward). Embedder passes it INTO Lynx → direct function pointer.
typedef bool (*lynx_custom_event_callback_t)(
    void* user_data,
    int32_t element_id,
    const char* event_name,
    const lynx_ui_method_value_t* params);
typedef void (*lynx_shell_set_custom_event_callback_fn)(
    lynx_shell_t* shell,
    lynx_custom_event_callback_t callback,
    void* user_data);

// ----- Dispatch table -------------------------------------------------------
//
// Populated by `whisker_bridge_load_lynx()`. Field order is purely
// presentational — bridge code calls through `whisker_lynx_capi()->fn`
// so the order doesn't have to match anything ABI-wise. Logical
// grouping mirrors the upstream header's section order to make a
// diff against `lynx_native_renderer_capi.h` easy to read.

typedef struct WhiskerLynxCapi {
  // ABI handshake (bound first by the loader).
  lynx_capi_abi_version_fn abi_version;

  // Shell lifecycle.
  lynx_shell_from_native_ptr_fn shell_from_native_ptr;
  lynx_shell_release_fn shell_release;
  lynx_shell_run_on_tasm_thread_fn shell_run_on_tasm_thread;

  // Element create / release / id.
  lynx_create_fiber_element_fn create_fiber_element;
  lynx_create_fiber_element_by_name_fn create_fiber_element_by_name;
  lynx_element_release_fn element_release;
  lynx_element_id_fn element_id;

  // Element manipulation.
  lynx_element_set_attribute_fn element_set_attribute;
  lynx_element_set_attribute_int_fn element_set_attribute_int;
  lynx_element_set_attribute_bool_fn element_set_attribute_bool;
  lynx_element_set_attribute_double_fn element_set_attribute_double;
  lynx_element_set_attribute_object_fn element_set_attribute_object;
  lynx_element_set_inline_styles_fn element_set_inline_styles;
  lynx_element_set_update_list_info_fn element_set_update_list_info;
  lynx_element_set_event_handler_fn element_set_event_handler;
  lynx_element_append_child_fn element_append_child;
  lynx_element_remove_child_fn element_remove_child;
  lynx_list_set_native_item_provider_fn list_set_native_item_provider;

  // Pipeline.
  lynx_shell_set_root_element_fn shell_set_root_element;
  lynx_shell_flush_fn shell_flush;

  // UI method dispatch.
  lynx_ui_invoke_method_fn ui_invoke_method;
  lynx_ui_invoke_method_with_params_fn ui_invoke_method_with_params;
  lynx_ui_invoke_method_async_fn ui_invoke_method_async;
  lynx_ui_invoke_method_async_with_params_fn ui_invoke_method_async_with_params;

  // Element-level animation.
  lynx_element_animate_fn element_animate;

  // Core-originated custom events. OPTIONAL: bound tail-additively —
  // NULL when the loaded Lynx predates the symbol (feature-detect at
  // the call site; list events simply stay dark on old engines).
  lynx_shell_set_custom_event_callback_fn shell_set_custom_event_callback;

  // Explicit list diff actions. OPTIONAL (tail-added after ABI v2) —
  // NULL on an older Lynx; the caller falls back to the full-replace
  // `element_set_update_list_info` (pre-feature behaviour: scroll
  // position resets on data updates, but nothing breaks).
  lynx_element_update_list_actions_fn element_update_list_actions;
} WhiskerLynxCapi;

// ----- Loader API -----------------------------------------------------------

// Result codes returned by `whisker_bridge_load_lynx()`. Negative on
// failure; 0 on success. Detailed values are emitted via the platform
// log channel.
typedef enum whisker_bridge_lynx_load_result {
  WHISKER_BRIDGE_LYNX_LOAD_OK = 0,
  WHISKER_BRIDGE_LYNX_LOAD_ERR_DLOPEN = -1,
  WHISKER_BRIDGE_LYNX_LOAD_ERR_MISSING_SYMBOL = -2,
  WHISKER_BRIDGE_LYNX_LOAD_ERR_ABI_MISMATCH = -3,
} whisker_bridge_lynx_load_result;

// Idempotent, thread-safe one-shot loader. dlopens Lynx, binds every
// function pointer in `WhiskerLynxCapi`, verifies
// `lynx_capi_abi_version()` matches `WHISKER_LYNX_CAPI_ABI_VERSION`.
// Subsequent calls return the cached result without redoing the work.
//
// Called from `whisker_bridge_engine_attach` at the top of every
// attach so a missed prior call surfaces as a clean error instead of
// a downstream NULL dispatch crash.
int whisker_bridge_load_lynx(void);

// Returns the dispatch table once `whisker_bridge_load_lynx` has
// succeeded; NULL otherwise. Bridge code calls
// `whisker_lynx_capi()->fn(args)` at every Lynx call site.
const WhiskerLynxCapi* whisker_lynx_capi(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // WHISKER_BRIDGE_LYNX_CAPI_H_
