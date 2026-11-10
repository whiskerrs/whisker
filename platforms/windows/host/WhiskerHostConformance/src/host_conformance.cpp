#include "whisker/host_conformance.h"

#include <windows.h>
#undef GetCurrentTime

#include <MddBootstrap.h>
#include <WindowsAppSDK-VersionInfo.h>

#include <cstdio>
#include <optional>

#include <winrt/Microsoft.Graphics.DirectX.h>
#include <winrt/Microsoft.UI.Dispatching.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.h>

#include "capabilities.h"
#include "graphics.h"
#include "host_scene.h"

namespace comp = winrt::Microsoft::UI::Composition;
namespace dx = winrt::Microsoft::Graphics::DirectX;

namespace {

struct Process {
  winrt::Microsoft::UI::Dispatching::DispatcherQueueController queue{nullptr};
  comp::Compositor compositor{nullptr};
  comp::CompositionGraphicsDevice graphics{nullptr};
  winrt::com_ptr<ID3D11Device> d3d;
};

Process* SharedProcess() {
  static Process* process = [] {
    winrt::check_hresult(MddBootstrapInitialize2(WINDOWSAPPSDK_RELEASE_MAJORMINOR,
                                                 WINDOWSAPPSDK_RELEASE_VERSION_TAG_W,
                                                 PACKAGE_VERSION{},
                                                 MddBootstrapInitializeOptions_None));
    winrt::init_apartment(winrt::apartment_type::single_threaded);
    auto value = new Process;
    value->queue =
        winrt::Microsoft::UI::Dispatching::DispatcherQueueController::CreateOnCurrentThread();
    value->compositor = comp::Compositor();
    value->graphics = whisker::CreateGraphicsDevice(value->compositor, &value->d3d);
    return value;
  }();
  return process;
}

void Report(winrt::hresult_error const& error) {
  std::fprintf(stderr, "WhiskerHostConformance: %08x %ls\n", static_cast<uint32_t>(error.code()),
               error.message().c_str());
}

template <typename Operation>
void Pump(Operation const& operation) {
  while (operation.Status() == winrt::Windows::Foundation::AsyncStatus::Started) {
    MSG message;
    if (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
      TranslateMessage(&message);
      DispatchMessageW(&message);
    } else {
      MsgWaitForMultipleObjects(0, nullptr, FALSE, 10, QS_ALLINPUT);
    }
  }
}

}  // namespace

struct WhiskerHostConformance {
  comp::ContainerVisual root{nullptr};
  whisker::TextEngine text;
  std::optional<whisker::HostScene> scene;
};

extern "C" WhiskerHostConformance* whisker_host_conformance_create(void) {
  try {
    auto process = SharedProcess();
    auto host = new WhiskerHostConformance;
    host->root = process->compositor.CreateContainerVisual();
    host->scene.emplace(host->root, host->text, process->graphics);
    return host;
  } catch (winrt::hresult_error const& error) {
    Report(error);
    return nullptr;
  }
}

extern "C" void whisker_host_conformance_destroy(WhiskerHostConformance* host) { delete host; }

extern "C" WhiskerMobileHostCapabilities whisker_host_conformance_capabilities(void) {
  return whisker::HostCapabilities();
}

extern "C" bool whisker_host_conformance_present(void* host, const WhiskerMobileFrame* frame,
                                                 WhiskerMobileApplyResponse* response) {
  try {
    return static_cast<WhiskerHostConformance*>(host)->scene->Apply(*frame, *response);
  } catch (winrt::hresult_error const& error) {
    Report(error);
    return false;
  }
}

extern "C" bool whisker_host_conformance_measure(void* host,
                                                 const WhiskerMobileMeasureRequest* requests,
                                                 size_t count,
                                                 WhiskerMobileMeasureResponse* responses) {
  auto& text = static_cast<WhiskerHostConformance*>(host)->text;
  for (size_t i = 0; i < count; ++i) text.Measure(requests[i], responses[i]);
  return true;
}

extern "C" bool whisker_host_conformance_capture(WhiskerHostConformance* host, uint32_t width,
                                                 uint32_t height, uint8_t* rgba) {
  try {
    auto process = SharedProcess();
    host->root.Size({static_cast<float>(width), static_cast<float>(height)});
    auto operation = process->graphics.CaptureAsync(
        host->root, {static_cast<int32_t>(width), static_cast<int32_t>(height)},
        dx::DirectXPixelFormat::B8G8R8A8UIntNormalized, dx::DirectXAlphaMode::Premultiplied, 0);
    Pump(operation);
    auto surface = operation.GetResults();

    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    winrt::com_ptr<ID3D11Texture2D> copy;
    winrt::check_hresult(process->d3d->CreateTexture2D(&desc, nullptr, copy.put()));
    winrt::check_hresult(surface.as<whisker::ICompositionDrawingSurfaceInterop2>()->CopySurface(
        copy.get(), 0, 0, nullptr));

    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    winrt::com_ptr<ID3D11Texture2D> staging;
    winrt::check_hresult(process->d3d->CreateTexture2D(&desc, nullptr, staging.put()));
    winrt::com_ptr<ID3D11DeviceContext> context;
    process->d3d->GetImmediateContext(context.put());
    context->CopyResource(staging.get(), copy.get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    winrt::check_hresult(context->Map(staging.get(), 0, D3D11_MAP_READ, 0, &mapped));
    for (uint32_t y = 0; y < height; ++y) {
      auto source = static_cast<const uint8_t*>(mapped.pData) + y * mapped.RowPitch;
      for (uint32_t x = 0; x < width; ++x, source += 4, rgba += 4) {
        uint8_t alpha = source[3];
        auto straight = [alpha](uint8_t channel) {
          return alpha ? static_cast<uint8_t>((channel * 255 + alpha / 2) / alpha) : uint8_t{0};
        };
        rgba[0] = straight(source[2]);
        rgba[1] = straight(source[1]);
        rgba[2] = straight(source[0]);
        rgba[3] = alpha;
      }
    }
    context->Unmap(staging.get(), 0);
    return true;
  } catch (winrt::hresult_error const& error) {
    Report(error);
    return false;
  }
}
