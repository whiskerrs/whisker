#include "whisker/whisker_view.h"

#include <array>
#include <cmath>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <tuple>
#include <utility>
#include <vector>

#include <winrt/Microsoft.UI.Dispatching.h>

#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <winrt/Microsoft.UI.Xaml.Input.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <winrt/Microsoft.UI.Xaml.h>
#include <winrt/Windows.Foundation.h>

#include "capabilities.h"
#include "graphics.h"
#include "host_scene.h"

namespace whisker {

namespace xaml = winrt::Microsoft::UI::Xaml;
namespace input = winrt::Microsoft::UI::Input;

namespace {

constexpr uint64_t kMousePointer = 1;
constexpr float kWheelStep = 48.0f;

input::InputSystemCursorShape CursorShape(int32_t keyword) {
  using Shape = input::InputSystemCursorShape;
  switch (keyword) {
    case 4: return Shape::Help;
    case 5: case 17: case 18: return Shape::Hand;
    case 6: return Shape::AppStarting;
    case 7: return Shape::Wait;
    case 8: case 9: return Shape::Cross;
    case 10: case 11: return Shape::IBeam;
    case 14: return Shape::SizeAll;
    case 15: case 16: return Shape::UniversalNo;
    case 19: case 22: case 24: case 29: return Shape::SizeWestEast;
    case 20: case 21: case 23: case 30: return Shape::SizeNorthSouth;
    case 25: case 28: case 31: return Shape::SizeNortheastSouthwest;
    case 26: case 27: case 32: return Shape::SizeNorthwestSoutheast;
    default: return Shape::Arrow;
  }
}

// Whisker numbers buttons left, middle, right, back, forward with bits 1, 4, 2, 8, 16.
int16_t ChangedButton(input::PointerUpdateKind kind) {
  using Kind = input::PointerUpdateKind;
  switch (kind) {
    case Kind::LeftButtonPressed: case Kind::LeftButtonReleased: return 0;
    case Kind::MiddleButtonPressed: case Kind::MiddleButtonReleased: return 1;
    case Kind::RightButtonPressed: case Kind::RightButtonReleased: return 2;
    case Kind::XButton1Pressed: case Kind::XButton1Released: return 3;
    case Kind::XButton2Pressed: case Kind::XButton2Released: return 4;
    default: return -1;
  }
}

uint32_t HeldButtons(input::PointerPointProperties const& properties) {
  return (properties.IsLeftButtonPressed() ? 1u : 0u) |
         (properties.IsRightButtonPressed() ? 2u : 0u) |
         (properties.IsMiddleButtonPressed() ? 4u : 0u) |
         (properties.IsXButton1Pressed() ? 8u : 0u) | (properties.IsXButton2Pressed() ? 16u : 0u);
}

double Now() { return static_cast<double>(GetTickCount64()); }

}  // namespace

struct WhiskerView::Impl {
  xaml::Controls::Panel host;
  TextEngine text;
  ResourceService resources;
  std::vector<WhiskerMobileResourceEvent> resource_events;
  std::optional<HostScene> scene;
  std::mutex module_events_mutex;
  std::vector<std::tuple<std::string, std::string, Value>> module_events;
  winrt::Microsoft::UI::Dispatching::DispatcherQueue queue =
      winrt::Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
  std::optional<ModuleServices> services;
  void* handle = nullptr;
  bool frame_requested = true;
  float width = 0;
  float height = 0;
  float scale = 1;
  int32_t cursor = -1;
  winrt::event_token rendering{};

  explicit Impl(xaml::Controls::Panel panel) : host(std::move(panel)) {
    auto compositor = xaml::Hosting::ElementCompositionPreview::GetElementVisual(host).Compositor();
    auto root = compositor.CreateContainerVisual();
    xaml::Hosting::ElementCompositionPreview::SetElementChildVisual(host, root);
    scene.emplace(root, text, resources, CreateGraphicsDevice(compositor));
    scene->SetFrameRequester([this] { frame_requested = true; });
    // Module events may come from any thread and leave on the next tick.
    services.emplace([this](std::string module, std::string event, Value payload) {
      {
        std::lock_guard lock(module_events_mutex);
        module_events.emplace_back(std::move(module), std::move(event), std::move(payload));
      }
      queue.TryEnqueue([this] { frame_requested = true; });
    });
    // Events go out from the next tick so they never re-enter the runtime mid-command.
    resources.SetEventSink([this](const WhiskerMobileResourceEvent& event) {
      resource_events.push_back(event);
      frame_requested = true;
    });
    rendering = xaml::Media::CompositionTarget::Rendering(
        [this](auto&&, auto&&) { Tick(); });
    host.PointerPressed([this](auto&&, xaml::Input::PointerRoutedEventArgs const& e) {
      host.CapturePointer(e.Pointer());
      Pointer(e, WHISKER_POINTER_DOWN);
    });
    host.PointerMoved([this](auto&&, xaml::Input::PointerRoutedEventArgs const& e) {
      Pointer(e, WHISKER_POINTER_MOVE);
    });
    host.PointerReleased([this](auto&&, xaml::Input::PointerRoutedEventArgs const& e) {
      Pointer(e, WHISKER_POINTER_UP);
      host.ReleasePointerCapture(e.Pointer());
    });
    host.PointerCanceled([this](auto&&, xaml::Input::PointerRoutedEventArgs const& e) {
      Pointer(e, WHISKER_POINTER_CANCEL);
    });
    host.PointerWheelChanged([this](auto&&, xaml::Input::PointerRoutedEventArgs const& e) {
      auto point = e.GetCurrentPoint(host);
      auto properties = point.Properties();
      float delta = -properties.MouseWheelDelta() / 120.0f * kWheelStep;
      bool horizontal = properties.IsHorizontalMouseWheel();
      auto position = point.Position();
      if (scene->ScrollAt(static_cast<float>(position.X), static_cast<float>(position.Y),
                          horizontal ? -delta : 0, horizontal ? 0 : delta)) {
        frame_requested = true;
        e.Handled(true);
      }
    });
  }

  ~Impl() {
    xaml::Media::CompositionTarget::Rendering(rendering);
    if (handle) whisker_view_destroy(handle);
  }

  void Tick() {
    float w = static_cast<float>(host.ActualWidth());
    float h = static_cast<float>(host.ActualHeight());
    if (w <= 0 || h <= 0) return;
    float s = host.XamlRoot() ? static_cast<float>(host.XamlRoot().RasterizationScale()) : 1.0f;
    if (w != width || h != height || s != scale) {
      width = w;
      height = h;
      scale = s;
      scene->SetScale(s);
      frame_requested = true;
    }
    if (!handle) {
      auto capabilities = HostCapabilities();
      handle = whisker_view_create(width, height, scale, &capabilities, &OnRequestFrame, this,
                                   &OnBootstrap, this, &OnMeasure, this, &OnPresent, this,
                                   &OnResourceCommand, this, &OnInvokeModule, &OnObserveModule,
                                   this);
      if (!handle) return;
    }
    DispatchScrollEvents();
    DispatchModuleEvents();
    for (auto const& event : std::exchange(resource_events, {})) {
      whisker_view_dispatch_resource_event(handle, &event);
    }
    if (!frame_requested) return;
    frame_requested = false;
    whisker_view_tick(handle, Now(), width, height, scale);
    scene->DrawViews();
  }

  void Pointer(xaml::Input::PointerRoutedEventArgs const& e, uint32_t event) {
    auto point = e.GetCurrentPoint(host);
    auto position = point.Position();
    float x = static_cast<float>(position.X);
    float y = static_cast<float>(position.Y);
    auto type = e.Pointer().PointerDeviceType();
    if (type == winrt::Microsoft::UI::Input::PointerDeviceType::Mouse) UpdateCursor(x, y);
    if (!handle) return;
    uint64_t id = kMousePointer;
    uint32_t kind = WHISKER_POINTER_MOUSE;
    if (type == winrt::Microsoft::UI::Input::PointerDeviceType::Touch) {
      kind = WHISKER_POINTER_TOUCH;
      id = e.Pointer().PointerId() + uint64_t{2};
    } else if (type == winrt::Microsoft::UI::Input::PointerDeviceType::Pen) {
      kind = WHISKER_POINTER_PEN;
      id = e.Pointer().PointerId() + uint64_t{2};
    }
    auto properties = point.Properties();
    uint32_t buttons = HeldButtons(properties);
    int16_t changed = event == WHISKER_POINTER_MOVE ? -1 : ChangedButton(properties.PointerUpdateKind());
    if (kind == WHISKER_POINTER_TOUCH) {
      buttons = event == WHISKER_POINTER_DOWN || event == WHISKER_POINTER_MOVE ? 1 : 0;
      changed = event == WHISKER_POINTER_MOVE ? -1 : 0;
    }
    std::vector<uint64_t> nodes;
    std::vector<float> offsets;
    for (auto const& [node, offset] : scene->TakeScrollOffsets()) {
      nodes.push_back(node);
      offsets.push_back(offset[0]);
      offsets.push_back(offset[1]);
    }
    whisker_view_dispatch_pointer(handle, Now(), event, id, kind, x, y, buttons, changed,
                                  nodes.data(), offsets.data(), nodes.size());
    e.Handled(true);
  }

  void UpdateCursor(float x, float y) {
    int32_t keyword = scene->CursorAt(x, y);
    if (keyword == cursor) return;
    cursor = keyword;
    host.as<xaml::IUIElementProtected>().ProtectedCursor(
        input::InputSystemCursor::Create(CursorShape(keyword)));
  }

  void DispatchScrollEvents() {
    if (!handle) return;
    for (auto const& event : scene->TakeScrollEvents()) {
      constexpr std::array<std::string_view, 6> keys{"scrollLeft",   "scrollTop",
                                                     "scrollWidth",  "scrollHeight",
                                                     "viewportWidth", "viewportHeight"};
      const float values[] = {event.left,  event.top,           event.width,
                              event.height, event.viewport_width, event.viewport_height};
      std::array<WhiskerKeyValueRaw, 6> entries{};
      for (size_t i = 0; i < entries.size(); ++i) {
        entries[i].key = {keys[i].data(), keys[i].size()};
        entries[i].value.type = WHISKER_VALUE_INT;
        entries[i].value.v.i = std::llround(values[i]);
      }
      WhiskerValueRaw detail{};
      detail.type = WHISKER_VALUE_MAP;
      detail.v.map = {entries.data(), entries.size()};
      constexpr std::string_view name = "scroll";
      whisker_view_dispatch_event(handle, Now(), event.node,
                                  reinterpret_cast<const uint8_t*>(name.data()), name.size(),
                                  &detail);
    }
  }

  void DispatchModuleEvents() {
    for (auto& event : scene->TakeViewEvents()) {
      RawEncoder encoder;
      whisker_view_dispatch_event(handle, Now(), event.node,
                                  reinterpret_cast<const uint8_t*>(event.name.data()),
                                  event.name.size(), encoder.Encode(event.detail));
    }
    decltype(module_events) pending;
    {
      std::lock_guard lock(module_events_mutex);
      pending.swap(module_events);
    }
    for (auto& [module, event, payload] : pending) {
      RawEncoder encoder;
      whisker_view_dispatch_module_event(
          handle, reinterpret_cast<const uint8_t*>(module.data()), module.size(),
          reinterpret_cast<const uint8_t*>(event.data()), event.size(), encoder.Encode(payload));
    }
  }

  static void OnRequestFrame(void* data) { static_cast<Impl*>(data)->frame_requested = true; }

  static bool OnBootstrap(void* data, const WhiskerMobileBootstrap* bootstrap) {
    return static_cast<Impl*>(data)->scene->Bootstrap(*bootstrap);
  }

  static bool OnMeasure(void* data, const WhiskerMobileMeasureRequest* requests, size_t count,
                        WhiskerMobileMeasureResponse* responses) {
    auto& text = static_cast<Impl*>(data)->text;
    for (size_t i = 0; i < count; ++i) text.Measure(requests[i], responses[i]);
    return true;
  }

  static bool OnPresent(void* data, const WhiskerMobileFrame* frame,
                        WhiskerMobileApplyResponse* response) {
    return static_cast<Impl*>(data)->scene->Apply(*frame, *response);
  }

  static bool OnResourceCommand(void* data, const WhiskerMobileResourceCommand* command) {
    return static_cast<Impl*>(data)->resources.Apply(*command);
  }

  static bool OnInvokeModule(void* data, const uint8_t* module, size_t module_len,
                             const uint8_t* function, size_t function_len,
                             const WhiskerValueRaw* args, size_t count, bool is_async,
                             WhiskerMobileModuleResultCallback result, void* result_data) {
    auto text = [](const uint8_t* bytes, size_t len) {
      return std::string_view(reinterpret_cast<const char*>(bytes), len);
    };
    return static_cast<Impl*>(data)->services->Invoke(text(module, module_len),
                                                      text(function, function_len), args, count,
                                                      is_async, result, result_data);
  }

  static void OnObserveModule(void*, const uint8_t*, size_t, const uint8_t*, size_t, bool) {}
};

WhiskerView::WhiskerView(xaml::Controls::Panel const& host) : impl_(std::make_unique<Impl>(host)) {}

WhiskerView::~WhiskerView() = default;

}  // namespace whisker
