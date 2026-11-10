#include <windows.h>
#undef GetCurrentTime

#include <memory>
#include <string>

#include <MddBootstrap.h>
#include <WindowsAppSDK-VersionInfo.h>
#include <winrt/Microsoft.UI.Windowing.h>
#include <winrt/Microsoft.UI.Xaml.Controls.h>
#include <winrt/Microsoft.UI.Xaml.Markup.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <winrt/Microsoft.UI.Xaml.XamlTypeInfo.h>
#include <winrt/Microsoft.UI.Xaml.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.UI.Xaml.Interop.h>

#include "whisker/whisker_app.h"
#include "whisker/whisker_view.h"

namespace {

using namespace winrt;
using namespace winrt::Microsoft::UI::Xaml;

struct App : ApplicationT<App, Markup::IXamlMetadataProvider> {
  App(std::wstring title, winrt::Windows::UI::Color background)
      : title_(std::move(title)), background_(background) {}

  void OnLaunched(LaunchActivatedEventArgs const&) {
    Resources().MergedDictionaries().Append(Controls::XamlControlsResources());
    window_ = Window();
    window_.Title(title_);
    Controls::Grid root;
    root.Background(Media::SolidColorBrush(background_));
    window_.Content(root);
    view_ = std::make_unique<whisker::WhiskerView>(root);
    window_.Activate();
    if (GetEnvironmentVariableW(L"WHISKER_MAXIMIZE", nullptr, 0) > 0) {
      window_.AppWindow().Presenter().as<winrt::Microsoft::UI::Windowing::OverlappedPresenter>().Maximize();
    }
  }

  Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type) {
    return provider_.GetXamlType(type);
  }
  Markup::IXamlType GetXamlType(hstring const& name) { return provider_.GetXamlType(name); }
  com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions() { return provider_.GetXmlnsDefinitions(); }

 private:
  std::wstring title_;
  winrt::Windows::UI::Color background_;
  XamlTypeInfo::XamlControlsXamlMetaDataProvider provider_;
  Window window_{nullptr};
  std::unique_ptr<whisker::WhiskerView> view_;
};

std::wstring Widen(const char* text, uintptr_t length) {
  if (!text || length == 0) return {};
  int count = MultiByteToWideChar(CP_UTF8, 0, text, static_cast<int>(length), nullptr, 0);
  std::wstring wide(count, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, text, static_cast<int>(length), wide.data(), count);
  return wide;
}

}  // namespace

extern "C" int32_t whisker_windows_run(const WhiskerWindowsAppOptions* options) {
  if (!options) return E_INVALIDARG;
  PACKAGE_VERSION minimum{};
  minimum.Version = WINDOWSAPPSDK_RUNTIME_VERSION_UINT64;
  HRESULT hr = MddBootstrapInitialize2(WINDOWSAPPSDK_RELEASE_MAJORMINOR,
                                       WINDOWSAPPSDK_RELEASE_VERSION_TAG_W, minimum,
                                       MddBootstrapInitializeOptions_OnNoMatch_ShowUI);
  if (FAILED(hr)) return hr;
  std::wstring title = Widen(options->title, options->title_len);
  winrt::Windows::UI::Color background{255, options->background_rgb[0], options->background_rgb[1],
                                       options->background_rgb[2]};
  try {
    init_apartment(apartment_type::single_threaded);
    Application::Start([&](auto&&) { make<App>(title, background); });
  } catch (winrt::hresult_error const& error) {
    hr = error.code();
  }
  MddBootstrapShutdown();
  return hr;
}
