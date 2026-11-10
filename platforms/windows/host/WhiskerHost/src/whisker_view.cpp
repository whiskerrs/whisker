#include "whisker/whisker_view.h"

#include <optional>

#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <winrt/Windows.Foundation.h>

#include "capabilities.h"
#include "graphics.h"
#include "host_scene.h"

namespace whisker {

namespace xaml = winrt::Microsoft::UI::Xaml;

struct WhiskerView::Impl {
  xaml::Controls::Panel host;
  TextEngine text;
  std::optional<HostScene> scene;
  void* handle = nullptr;
  bool frame_requested = true;
  float width = 0;
  float height = 0;
  float scale = 1;
  winrt::event_token rendering{};

  explicit Impl(xaml::Controls::Panel panel) : host(std::move(panel)) {
    auto compositor = xaml::Hosting::ElementCompositionPreview::GetElementVisual(host).Compositor();
    auto root = compositor.CreateContainerVisual();
    xaml::Hosting::ElementCompositionPreview::SetElementChildVisual(host, root);
    scene.emplace(root, text, CreateGraphicsDevice(compositor));
    rendering = xaml::Media::CompositionTarget::Rendering(
        [this](auto&&, auto&&) { Tick(); });
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
    if (!frame_requested) return;
    frame_requested = false;
    whisker_view_tick(handle, static_cast<double>(GetTickCount64()), width, height, scale);
  }

  static void OnRequestFrame(void* data) { static_cast<Impl*>(data)->frame_requested = true; }

  static bool OnBootstrap(void*, const WhiskerMobileBootstrap* bootstrap) {
    return bootstrap->abi_major == WHISKER_MOBILE_ABI_MAJOR;
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

  static bool OnResourceCommand(void*, const WhiskerMobileResourceCommand*) { return false; }

  static bool OnInvokeModule(void*, const uint8_t*, size_t, const uint8_t*, size_t,
                             const WhiskerValueRaw*, size_t, bool,
                             WhiskerMobileModuleResultCallback, void*) {
    return false;
  }

  static void OnObserveModule(void*, const uint8_t*, size_t, const uint8_t*, size_t, bool) {}
};

WhiskerView::WhiskerView(xaml::Controls::Panel const& host) : impl_(std::make_unique<Impl>(host)) {}

WhiskerView::~WhiskerView() = default;

}  // namespace whisker
