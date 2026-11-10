#include <windows.h>
#undef GetCurrentTime

#include <memory>

#include <winrt/Microsoft.UI.Windowing.h>
#include <winrt/Microsoft.UI.Xaml.Controls.h>
#include <winrt/Microsoft.UI.Xaml.Markup.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <winrt/Microsoft.UI.Xaml.XamlTypeInfo.h>
#include <winrt/Microsoft.UI.Xaml.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.UI.Xaml.Interop.h>

#include "whisker/whisker_view.h"

using namespace winrt;
using namespace winrt::Microsoft::UI::Xaml;

struct App : ApplicationT<App, Markup::IXamlMetadataProvider> {
  XamlTypeInfo::XamlControlsXamlMetaDataProvider provider;
  Window window{nullptr};
  std::unique_ptr<whisker::WhiskerView> view;

  void OnLaunched(LaunchActivatedEventArgs const&) {
    Resources().MergedDictionaries().Append(Controls::XamlControlsResources());
    window = Window();
    window.Title(L"Whisker");
    Controls::Grid root;
    root.Background(Media::SolidColorBrush(winrt::Microsoft::UI::Colors::White()));
    window.Content(root);
    view = std::make_unique<whisker::WhiskerView>(root);
    window.Activate();
    if (GetEnvironmentVariableW(L"WHISKER_MAXIMIZE", nullptr, 0) > 0) {
      window.AppWindow().Presenter().as<winrt::Microsoft::UI::Windowing::OverlappedPresenter>().Maximize();
    }
  }

  Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type) {
    return provider.GetXamlType(type);
  }
  Markup::IXamlType GetXamlType(hstring const& name) { return provider.GetXamlType(name); }
  com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions() { return provider.GetXmlnsDefinitions(); }
};

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
  init_apartment(apartment_type::single_threaded);
  Application::Start([](auto&&) { make<App>(); });
  return 0;
}
