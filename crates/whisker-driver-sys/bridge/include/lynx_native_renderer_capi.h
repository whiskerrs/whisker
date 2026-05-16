// Vendored copy of the Lynx fork's native renderer C ABI header.
//
// MUST stay in sync with whiskerrs/lynx:core/native_renderer_capi/
// public/lynx_native_renderer_capi.h. The API surface is small and
// stable (Phase 6-α contract), but if either side adds a function or
// changes a signature, the other has to be updated in lockstep.
//
// Why vendored:
// - On Android, the symbols this header declares live inside
//   liblynx.so (the Lynx fork's CI compiles
//   core/native_renderer_capi/lynx_native_renderer.cc into the
//   LynxAndroid AAR). Whisker's bridge dlopen's liblynx.so and the
//   linker resolves these names — but the bridge still needs the
//   header to compile its callers.
// - On iOS, upstream Lynx 3.7.0's CocoaPods spec doesn't include
//   `core/native_renderer_capi/` (it's a Whisker fork-only addition
//   that hasn't been upstreamed yet). The iOS Lynx.xcframework is
//   built from the upstream pod, so it doesn't carry these symbols.
//   Whisker compiles `lynx_native_renderer.cc` itself on iOS — see
//   crates/whisker-driver-sys/build.rs's iOS path — and the resulting
//   symbols land inside WhiskerDriver.framework alongside the bridge
//   code.
// - Keeping a vendored copy means the build doesn't have to fish the
//   header out of either tarball (their staging layouts differ); the
//   bridge just resolves it from its own `include/` directory.

#ifndef WHISKER_VENDORED_LYNX_NATIVE_RENDERER_CAPI_H_
#define WHISKER_VENDORED_LYNX_NATIVE_RENDERER_CAPI_H_

#include <stdbool.h>
#include <stdint.h>

#if defined(__GNUC__) || defined(__clang__)
#define LYNX_NATIVE_RENDERER_CAPI_EXPORT \
  __attribute__((visibility("default")))
#else
#define LYNX_NATIVE_RENDERER_CAPI_EXPORT
#endif

#ifdef __cplusplus
extern "C" {
#endif

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

// ----- Shell wrapping + lifecycle -------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT lynx_shell_t* lynx_shell_from_native_ptr(
    void* native_shell_ptr);

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_shell_release(lynx_shell_t* shell);

// ----- Thread dispatch ------------------------------------------------------

typedef void (*lynx_tasm_callback_t)(void* user_data);

LYNX_NATIVE_RENDERER_CAPI_EXPORT bool lynx_shell_run_on_tasm_thread(
    lynx_shell_t* shell,
    lynx_tasm_callback_t callback,
    void* user_data);

// ----- Element creation -----------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT lynx_fiber_element_t* lynx_create_fiber_element(
    lynx_shell_t* shell,
    lynx_element_tag_e tag);

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_release(
    lynx_fiber_element_t* element);

LYNX_NATIVE_RENDERER_CAPI_EXPORT int32_t lynx_element_id(
    lynx_fiber_element_t* element);

// ----- Element manipulation -------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_set_attribute(
    lynx_fiber_element_t* element,
    const char* key,
    const char* value);

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_set_inline_styles(
    lynx_fiber_element_t* element,
    const char* css);

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_append_child(
    lynx_fiber_element_t* parent,
    lynx_fiber_element_t* child);

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_remove_child(
    lynx_fiber_element_t* parent,
    lynx_fiber_element_t* child);

// ----- Pipeline -------------------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_shell_set_root_element(
    lynx_shell_t* shell,
    lynx_fiber_element_t* page);

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_shell_flush(lynx_shell_t* shell);

// ----- subsecond ASLR anchor ------------------------------------------------

// Whisker's subsecond hot-patcher dlsym's this symbol on startup to
// compute the runtime ASLR slide of the image that contains it.
// Stable across Lynx versions by contract.
LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_aslr_reference(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // WHISKER_VENDORED_LYNX_NATIVE_RENDERER_CAPI_H_
