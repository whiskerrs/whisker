// Vendored from whiskerrs/lynx:core/native_renderer_capi/lynx_native_renderer.cc.
//
// On Android this file is built INSIDE liblynx.so by the Lynx fork's
// CI (see lynx_android_lib's public_deps); the version in this
// repository is only compiled into the Whisker bridge on iOS, where
// upstream Lynx 3.7.0's CocoaPods spec doesn't include the
// `core/native_renderer_capi/` subtree.
//
// MUST stay in sync with the fork's copy — see the header sibling
// for the duplication rationale.

#include "lynx_native_renderer_capi.h"

#include <cstdlib>
#include <cstring>
#include <memory>
#include <utility>
#include <vector>

#include "base/include/value/base_string.h"
#include "base/include/value/array.h"
#include "base/include/value/table.h"
#include "core/public/pipeline_option.h"
#include "core/renderer/dom/element_manager.h"
#include "core/renderer/dom/fiber/fiber_element.h"
#include "core/renderer/dom/fiber/page_element.h"
#include "core/renderer/events/events.h"
#include "core/renderer/dom/fiber/raw_text_element.h"
#include "core/renderer/dom/fiber/scroll_element.h"
#include "core/renderer/dom/fiber/text_element.h"
#include "core/renderer/dom/fiber/view_element.h"
#include "core/renderer/page_proxy.h"
#include "core/renderer/template_assembler.h"
#include "core/renderer/utils/base/tasm_constants.h"
#include "core/renderer/ui_wrapper/painting/catalyzer.h"
#include "core/shell/lynx_shell.h"
#include "core/public/pub_value.h"
#include "core/template_bundle/template_codec/binary_decoder/page_config.h"
#include "core/value_wrapper/value_impl_lepus.h"

// ----- Internal handle structures ------------------------------------------
//
// Opaque to the C API; allocated/freed by this translation unit.

struct lynx_shell_t {
  lynx::shell::LynxShell* shell = nullptr;
  // Cached after first run_on_tasm_thread initialises fiber-arch.
  lynx::tasm::ElementManager* manager = nullptr;
  // Strong reference to the installed root page (so the underlying
  // PageElement isn't released while the shell still references it).
  fml::RefPtr<lynx::tasm::PageElement> root_page;
  bool fiber_arch_initialized = false;
};

struct lynx_fiber_element_t {
  fml::RefPtr<lynx::tasm::FiberElement> ref;
};

// ----- Shell wrapping + lifecycle -------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT lynx_shell_t* lynx_shell_from_native_ptr(
    void* native_shell_ptr) {
  if (native_shell_ptr == nullptr) {
    return nullptr;
  }
  auto* handle = new lynx_shell_t();
  handle->shell = reinterpret_cast<lynx::shell::LynxShell*>(native_shell_ptr);
  return handle;
}


LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_shell_release(lynx_shell_t* shell) {
  if (shell == nullptr) return;
  // Drop the page first to avoid a dangling FiberElement → ElementManager
  // back-pointer if the caller has already torn down the LynxView.
  shell->root_page = nullptr;
  delete shell;
}

// ----- Thread dispatch ------------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT bool lynx_shell_run_on_tasm_thread(
    lynx_shell_t* shell,
    lynx_tasm_callback_t callback,
    void* user_data) {
  if (shell == nullptr || shell->shell == nullptr || callback == nullptr) {
    return false;
  }
  lynx_shell_t* capture = shell;
  shell->shell->RunOnTasmThread([capture, callback, user_data]() {
    if (!capture->fiber_arch_initialized) {
      // Lazy-initialise fiber-arch + cache the ElementManager. The
      // ElementManager only becomes available once the shell's TASM
      // thread is running, which is the case here.
      auto* tasm = capture->shell->GetTasm();
      if (tasm != nullptr) {
        auto config = std::make_shared<lynx::tasm::PageConfig>();
        config->SetEnableFiberArch(true);
        tasm->SetPageConfig(config);
        auto* page_proxy = tasm->page_proxy();
        if (page_proxy != nullptr) {
          capture->manager = page_proxy->element_manager().get();
        }
      }
      capture->fiber_arch_initialized = true;
    }
    callback(user_data);
  });
  return true;
}

// ----- Element creation -----------------------------------------------------

namespace {

fml::RefPtr<lynx::tasm::FiberElement> CreateForTag(
    lynx::tasm::ElementManager* manager,
    lynx_element_tag_e tag) {
  using namespace lynx;
  if (manager == nullptr) return nullptr;
  switch (tag) {
    case LYNX_ELEMENT_TAG_PAGE:
      // The "0" + id=0 pair mirrors the constants Lynx's internal
      // ReactLynx renderer uses for the root page. Native embedders
      // get the same shape so layout / pipeline behaves identically.
      return manager->CreateFiberPage(base::String("0"), 0);
    case LYNX_ELEMENT_TAG_VIEW:
      return manager->CreateFiberView();
    case LYNX_ELEMENT_TAG_TEXT:
      return manager->CreateFiberText(base::String("text"));
    case LYNX_ELEMENT_TAG_RAW_TEXT:
      return manager->CreateFiberRawText();
    case LYNX_ELEMENT_TAG_IMAGE:
      // TODO: expose a proper CreateFiberImage once the public API
      // grows it. For now, fall back to a View — the platform image
      // attribute is set via lynx_element_set_attribute("src", ...)
      // and Lynx's UI layer still handles rendering correctly.
      return manager->CreateFiberView();
    case LYNX_ELEMENT_TAG_SCROLL_VIEW:
      return manager->CreateFiberScrollView(
          base::String(lynx::tasm::kElementScrollViewTag));
  }
  return nullptr;
}

}  // namespace

LYNX_NATIVE_RENDERER_CAPI_EXPORT lynx_fiber_element_t* lynx_create_fiber_element(
    lynx_shell_t* shell,
    lynx_element_tag_e tag) {
  if (shell == nullptr || shell->manager == nullptr) return nullptr;
  auto ref = CreateForTag(shell->manager, tag);
  if (!ref) return nullptr;
  return new lynx_fiber_element_t{std::move(ref)};
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT lynx_fiber_element_t*
lynx_create_fiber_element_by_name(lynx_shell_t* shell, const char* tag_name) {
  if (shell == nullptr || shell->manager == nullptr || tag_name == nullptr ||
      tag_name[0] == '\0') {
    return nullptr;
  }
  // `CreateFiberNode` is the generic factory ElementManager exposes
  // for any registered tag — built-ins (`view` / `text` / `image` /
  // `scroll-view`) and custom (`x-input` / `x-refresh` / …) alike.
  // For tags Lynx's behaviour registry doesn't know, it returns
  // nullptr and we surface that to the Rust caller.
  auto ref = shell->manager->CreateFiberNode(lynx::base::String(tag_name));
  if (!ref) return nullptr;
  return new lynx_fiber_element_t{std::move(ref)};
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_release(lynx_fiber_element_t* element) {
  delete element;
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT int32_t lynx_element_id(lynx_fiber_element_t* element) {
  if (element == nullptr || !element->ref) return 0;
  return element->ref->impl_id();
}

// ----- Element manipulation -------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_set_attribute(
    lynx_fiber_element_t* element,
    const char* key,
    const char* value) {
  if (element == nullptr || !element->ref || key == nullptr ||
      value == nullptr) {
    return;
  }
  element->ref->SetAttribute(
      lynx::base::String(key),
      lynx::lepus::Value(lynx::base::String(value)));
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_set_event_handler(
    lynx_fiber_element_t* element,
    const char* event_name) {
  if (element == nullptr || !element->ref || event_name == nullptr) {
    return;
  }
  // Bind a bubble-phase (`bindEvent`) handler. The function name is a
  // sentinel — Whisker observes the fire via the reporter, not by
  // calling a JS function (there is no JS runtime). Registering the
  // handler is what puts the event in the element's event set, which is
  // what makes Lynx's UI components emit their component-specific events
  // (scroll, layout, uiappear, …) in the first place.
  element->ref->SetJSEventHandler(
      lynx::base::String(event_name),
      lynx::base::String(lynx::tasm::kEventBindEvent),
      lynx::base::String("__whisker_native__"));
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_set_inline_styles(
    lynx_fiber_element_t* element,
    const char* css) {
  if (element == nullptr || !element->ref || css == nullptr) return;
  element->ref->SetRawInlineStyles(lynx::base::String(css));
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_append_child(
    lynx_fiber_element_t* parent,
    lynx_fiber_element_t* child) {
  if (parent == nullptr || child == nullptr || !parent->ref || !child->ref) {
    return;
  }
  parent->ref->InsertNode(child->ref);
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_element_remove_child(
    lynx_fiber_element_t* parent,
    lynx_fiber_element_t* child) {
  if (parent == nullptr || child == nullptr || !parent->ref || !child->ref) {
    return;
  }
  parent->ref->RemoveNode(child->ref);
}

// ----- Pipeline -------------------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_shell_set_root_element(
    lynx_shell_t* shell,
    lynx_fiber_element_t* page) {
  if (shell == nullptr || shell->manager == nullptr || page == nullptr ||
      !page->ref) {
    return;
  }
  auto page_ref = fml::RefPtr<lynx::tasm::PageElement>(
      static_cast<lynx::tasm::PageElement*>(page->ref.get()));
  shell->manager->SetFiberPageElement(page_ref);
  shell->root_page = std::move(page_ref);
}

LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_shell_flush(lynx_shell_t* shell) {
  if (shell == nullptr || shell->manager == nullptr || !shell->root_page) {
    return;
  }
  shell->root_page->FlushActionsAsRoot();
  auto options = std::make_shared<lynx::tasm::PipelineOptions>();
  shell->manager->OnPatchFinish(options, shell->root_page.get());
}

// ----- UI method dispatch ---------------------------------------------------

LYNX_NATIVE_RENDERER_CAPI_EXPORT int32_t lynx_ui_invoke_method(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* args,
    size_t arg_count) {
  if (shell == nullptr || shell->manager == nullptr ||
      method_name == nullptr) {
    return -1;
  }
  auto* catalyzer = shell->manager->catalyzer();
  if (catalyzer == nullptr) {
    return -2;
  }

  // Package the args as `{"args": [arg0, arg1, ...]}` — the
  // convention Whisker's `WhiskerValue.fromNSDictionary` (iOS) /
  // `WhiskerValue.fromReadableMap` (Android) decoders expect from
  // their `@WhiskerUIMethod`-emitted forwarders.
  auto args_array = lynx::lepus::CArray::Create();
  for (size_t i = 0; args != nullptr && i < arg_count; i++) {
    const lynx_ui_method_value_t& v = args[i];
    switch (v.type) {
      case LYNX_UI_METHOD_VALUE_NULL:
        args_array->emplace_back(lynx::lepus::Value());
        break;
      case LYNX_UI_METHOD_VALUE_BOOL:
        args_array->emplace_back(lynx::lepus::Value(v.v.b));
        break;
      case LYNX_UI_METHOD_VALUE_INT:
        args_array->emplace_back(lynx::lepus::Value(v.v.i));
        break;
      case LYNX_UI_METHOD_VALUE_DOUBLE:
        args_array->emplace_back(lynx::lepus::Value(v.v.f));
        break;
      case LYNX_UI_METHOD_VALUE_STRING:
        args_array->emplace_back(lynx::lepus::Value(
            lynx::base::String(v.v.s != nullptr ? v.v.s : "")));
        break;
      default:
        // Array / map args aren't supported by the dispatch ABI;
        // treat as null. (Results use the recursive variants.)
        args_array->emplace_back(lynx::lepus::Value());
        break;
    }
  }
  auto params_dict = lynx::lepus::Dictionary::Create();
  BASE_STATIC_STRING_DECL(kArgs, "args");
  params_dict->SetValue(kArgs, lynx::lepus::Value(std::move(args_array)));
  lynx::lepus::Value params_lepus(std::move(params_dict));

  // Fire-and-forget: the platform Invoke routes the actual UI
  // method call to the main / UI thread via `dispatch_async` (iOS)
  // / a posted runnable (Android). The callback fires after the
  // method completes — typically synchronously inside the platform
  // method, but the Whisker C wrapper has no useful way to surface
  // an async result without growing an async API. v1 contract is
  // sync-only / discard-result.
  auto noop_callback = [](int32_t /*code*/, const lynx::pub::Value& /*data*/) {};

  catalyzer->Invoke(static_cast<int64_t>(sign),
                    std::string(method_name),
                    lynx::pub::ValueImplLepus(params_lepus),
                    noop_callback);
  return 0;
}

// ----- Params-map UI-method dispatch (fire-and-forget) ----------------------
//
// Built-in Lynx UI methods (`scrollTo`, `scrollIntoView`, ...) read
// their arguments as *named fields* of the params object
// (`params.getString("offset")`, `params.getBoolean("smooth")`, ...) —
// not from the `{"args": [...]}` wrapper `lynx_ui_invoke_method` builds
// for Whisker module methods. This entry takes a single MAP value and
// passes it through as the params object directly, so the named fields
// land where the platform method looks for them. Nested maps / arrays
// (e.g. `scrollIntoView`'s `scrollIntoViewOptions`) round-trip via the
// recursive converter.

namespace {

// Recursively convert a Whisker UI-method value into a lepus value.
// Mirrors the scalar arg handling in `lynx_ui_invoke_method`, plus the
// MAP / ARRAY variants the params path needs.
lynx::lepus::Value WhiskerCapiValueToLepus(const lynx_ui_method_value_t& v) {
  switch (v.type) {
    case LYNX_UI_METHOD_VALUE_BOOL:
      return lynx::lepus::Value(v.v.b);
    case LYNX_UI_METHOD_VALUE_INT:
      return lynx::lepus::Value(v.v.i);
    case LYNX_UI_METHOD_VALUE_DOUBLE:
      return lynx::lepus::Value(v.v.f);
    case LYNX_UI_METHOD_VALUE_STRING:
      return lynx::lepus::Value(
          lynx::base::String(v.v.s != nullptr ? v.v.s : ""));
    case LYNX_UI_METHOD_VALUE_ARRAY: {
      auto arr = lynx::lepus::CArray::Create();
      for (size_t i = 0; i < v.v.array.count; i++) {
        arr->emplace_back(WhiskerCapiValueToLepus(v.v.array.items[i]));
      }
      return lynx::lepus::Value(std::move(arr));
    }
    case LYNX_UI_METHOD_VALUE_MAP: {
      auto dict = lynx::lepus::Dictionary::Create();
      for (size_t i = 0; i < v.v.map.count; i++) {
        const lynx_ui_method_kv_t& kv = v.v.map.entries[i];
        dict->SetValue(lynx::base::String(kv.key != nullptr ? kv.key : ""),
                       WhiskerCapiValueToLepus(kv.value));
      }
      return lynx::lepus::Value(std::move(dict));
    }
    case LYNX_UI_METHOD_VALUE_NULL:
    default:
      return lynx::lepus::Value();
  }
}

}  // namespace

LYNX_NATIVE_RENDERER_CAPI_EXPORT int32_t lynx_ui_invoke_method_with_params(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* params) {
  if (shell == nullptr || shell->manager == nullptr ||
      method_name == nullptr) {
    return -1;
  }
  auto* catalyzer = shell->manager->catalyzer();
  if (catalyzer == nullptr) {
    return -2;
  }

  // Pass the MAP through as the params object directly (no `{"args":
  // [...]}` wrapper). A null / non-map `params` degrades to an empty
  // object so the platform method still runs with its defaults.
  lynx::lepus::Value params_lepus =
      (params != nullptr && params->type == LYNX_UI_METHOD_VALUE_MAP)
          ? WhiskerCapiValueToLepus(*params)
          : lynx::lepus::Value(lynx::lepus::Dictionary::Create());

  auto noop_callback = [](int32_t /*code*/, const lynx::pub::Value& /*data*/) {};

  catalyzer->Invoke(static_cast<int64_t>(sign),
                    std::string(method_name),
                    lynx::pub::ValueImplLepus(params_lepus),
                    noop_callback);
  return 0;
}

// ----- Async UI-method dispatch (result-returning) --------------------------

namespace {

// `malloc` a NUL-terminated copy of `s` (freed by
// `lynx_ui_method_value_free`).
char* CapiDupCStr(const std::string& s) {
  char* buf = static_cast<char*>(std::malloc(s.size() + 1));
  std::memcpy(buf, s.c_str(), s.size() + 1);
  return buf;
}

// Convert a Lynx `pub::Value` (the Catalyzer callback's result) into a
// heap-owned `lynx_ui_method_value_t` tree. **Lynx-neutral** — no
// dependency on the Whisker bridge, so this file compiles identically
// into liblynx.so (Android) and WhiskerDriver (iOS). The Whisker side
// converts this into a `WhiskerValueRaw`.
lynx_ui_method_value_t PubValueToCapi(const lynx::pub::Value& v) {
  lynx_ui_method_value_t out;
  std::memset(&out, 0, sizeof(out));
  if (v.IsNil() || v.IsUndefined()) {
    out.type = LYNX_UI_METHOD_VALUE_NULL;
    return out;
  }
  if (v.IsBool()) {
    out.type = LYNX_UI_METHOD_VALUE_BOOL;
    out.v.b = v.Bool();
    return out;
  }
  if (v.IsInt64()) {
    out.type = LYNX_UI_METHOD_VALUE_INT;
    out.v.i = v.Int64();
    return out;
  }
  if (v.IsUInt64()) {
    out.type = LYNX_UI_METHOD_VALUE_INT;
    out.v.i = static_cast<int64_t>(v.UInt64());
    return out;
  }
  if (v.IsDouble()) {
    out.type = LYNX_UI_METHOD_VALUE_DOUBLE;
    out.v.f = v.Double();
    return out;
  }
  if (v.IsNumber()) {
    out.type = LYNX_UI_METHOD_VALUE_DOUBLE;
    out.v.f = v.Number();
    return out;
  }
  if (v.IsString()) {
    out.type = LYNX_UI_METHOD_VALUE_STRING;
    out.v.s = CapiDupCStr(v.str());
    return out;
  }
  if (v.IsArray()) {
    int n = v.Length();
    if (n < 0) n = 0;
    out.type = LYNX_UI_METHOD_VALUE_ARRAY;
    out.v.array.count = static_cast<size_t>(n);
    out.v.array.items =
        n > 0 ? static_cast<lynx_ui_method_value_t*>(
                    std::malloc(sizeof(lynx_ui_method_value_t) * n))
              : nullptr;
    for (int i = 0; i < n; i++) {
      auto child = v.GetValueAtIndex(static_cast<uint32_t>(i));
      out.v.array.items[i] =
          child ? PubValueToCapi(*child) : lynx_ui_method_value_t{};
    }
    return out;
  }
  if (v.IsMap()) {
    std::vector<std::pair<std::string, lynx_ui_method_value_t>> tmp;
    v.ForeachMap([&tmp](const lynx::pub::Value& key,
                        const lynx::pub::Value& val) {
      tmp.emplace_back(key.str(), PubValueToCapi(val));
    });
    size_t n = tmp.size();
    out.type = LYNX_UI_METHOD_VALUE_MAP;
    out.v.map.count = n;
    out.v.map.entries =
        n > 0 ? static_cast<lynx_ui_method_kv_t*>(
                    std::malloc(sizeof(lynx_ui_method_kv_t) * n))
              : nullptr;
    for (size_t i = 0; i < n; i++) {
      out.v.map.entries[i].key = CapiDupCStr(tmp[i].first);
      out.v.map.entries[i].value = tmp[i].second;
    }
    return out;
  }
  out.type = LYNX_UI_METHOD_VALUE_NULL;
  return out;
}

// Recursively free a tree produced by `PubValueToCapi`.
void CapiValueFree(lynx_ui_method_value_t* v) {
  if (v == nullptr) return;
  switch (v->type) {
    case LYNX_UI_METHOD_VALUE_STRING:
      std::free(const_cast<char*>(v->v.s));
      break;
    case LYNX_UI_METHOD_VALUE_ARRAY:
      for (size_t i = 0; i < v->v.array.count; i++) {
        CapiValueFree(&v->v.array.items[i]);
      }
      std::free(v->v.array.items);
      break;
    case LYNX_UI_METHOD_VALUE_MAP:
      for (size_t i = 0; i < v->v.map.count; i++) {
        std::free(const_cast<char*>(v->v.map.entries[i].key));
        CapiValueFree(&v->v.map.entries[i].value);
      }
      std::free(v->v.map.entries);
      break;
    default:
      break;
  }
}

}  // namespace

LYNX_NATIVE_RENDERER_CAPI_EXPORT int32_t lynx_ui_invoke_method_async(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* args,
    size_t arg_count,
    lynx_ui_method_result_cb callback,
    void* user_data) {
  if (shell == nullptr || shell->manager == nullptr ||
      method_name == nullptr) {
    return -1;
  }
  auto* catalyzer = shell->manager->catalyzer();
  if (catalyzer == nullptr) {
    return -2;
  }

  auto args_array = lynx::lepus::CArray::Create();
  for (size_t i = 0; args != nullptr && i < arg_count; i++) {
    const lynx_ui_method_value_t& v = args[i];
    switch (v.type) {
      case LYNX_UI_METHOD_VALUE_NULL:
        args_array->emplace_back(lynx::lepus::Value());
        break;
      case LYNX_UI_METHOD_VALUE_BOOL:
        args_array->emplace_back(lynx::lepus::Value(v.v.b));
        break;
      case LYNX_UI_METHOD_VALUE_INT:
        args_array->emplace_back(lynx::lepus::Value(v.v.i));
        break;
      case LYNX_UI_METHOD_VALUE_DOUBLE:
        args_array->emplace_back(lynx::lepus::Value(v.v.f));
        break;
      case LYNX_UI_METHOD_VALUE_STRING:
        args_array->emplace_back(lynx::lepus::Value(
            lynx::base::String(v.v.s != nullptr ? v.v.s : "")));
        break;
      default:
        // Array / map args aren't supported by the dispatch ABI;
        // treat as null. (Results use the recursive variants.)
        args_array->emplace_back(lynx::lepus::Value());
        break;
    }
  }
  auto params_dict = lynx::lepus::Dictionary::Create();
  BASE_STATIC_STRING_DECL(kArgs, "args");
  params_dict->SetValue(kArgs, lynx::lepus::Value(std::move(args_array)));
  lynx::lepus::Value params_lepus(std::move(params_dict));

  // The callback fires (typically on the UI thread) once the platform
  // method completes. Convert the result `pub::Value` into a
  // heap-owned `lynx_ui_method_value_t` tree, hand it to the C
  // callback, then free it once the callback returns (the callee
  // copies out — the Whisker bridge converts it to a `WhiskerValueRaw`).
  catalyzer->Invoke(
      static_cast<int64_t>(sign), std::string(method_name),
      lynx::pub::ValueImplLepus(params_lepus),
      [callback, user_data](int32_t code, const lynx::pub::Value& data) {
        if (callback == nullptr) return;
        lynx_ui_method_value_t result = PubValueToCapi(data);
        callback(code, &result, user_data);
        CapiValueFree(&result);
      });
  return 0;
}

// ----- Unified params-map + result dispatch ---------------------------------
//
// The general element-method path: a single MAP value passed through as
// the params object directly (named fields for built-in Lynx methods;
// `{"args": [...]}` for Whisker module elements — the caller builds the
// shape) PLUS the async result callback. This is the one capi the
// Whisker `ElementRef::invoke` / `invoke_typed` build on, so new methods
// never need a new capi. Combines `WhiskerCapiValueToLepus` (params,
// shared with the fire-and-forget `lynx_ui_invoke_method_with_params`)
// with the result conversion from `lynx_ui_invoke_method_async`.
LYNX_NATIVE_RENDERER_CAPI_EXPORT int32_t lynx_ui_invoke_method_async_with_params(
    lynx_shell_t* shell,
    int32_t sign,
    const char* method_name,
    const lynx_ui_method_value_t* params,
    lynx_ui_method_result_cb callback,
    void* user_data) {
  if (shell == nullptr || shell->manager == nullptr ||
      method_name == nullptr) {
    return -1;
  }
  auto* catalyzer = shell->manager->catalyzer();
  if (catalyzer == nullptr) {
    return -2;
  }

  lynx::lepus::Value params_lepus =
      (params != nullptr && params->type == LYNX_UI_METHOD_VALUE_MAP)
          ? WhiskerCapiValueToLepus(*params)
          : lynx::lepus::Value(lynx::lepus::Dictionary::Create());

  catalyzer->Invoke(
      static_cast<int64_t>(sign), std::string(method_name),
      lynx::pub::ValueImplLepus(params_lepus),
      [callback, user_data](int32_t code, const lynx::pub::Value& data) {
        if (callback == nullptr) return;
        lynx_ui_method_value_t result = PubValueToCapi(data);
        callback(code, &result, user_data);
        CapiValueFree(&result);
      });
  return 0;
}

// ----- subsecond ASLR anchor ------------------------------------------------

// Intentionally non-empty so the linker doesn't merge it with other
// trivially-empty functions or strip it. The volatile keeps the
// compiler honest about the assignment having an observable effect.
LYNX_NATIVE_RENDERER_CAPI_EXPORT void lynx_aslr_reference(void) {
  static volatile int marker = 0;
  marker = marker + 1;
}
