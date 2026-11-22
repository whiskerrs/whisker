#pragma once

#include "whisker/module.h"

// windows.h's macro collides with a WinUI member of the same name.
#undef GetCurrentTime
#include <winrt/Microsoft.UI.Xaml.h>

namespace whisker {

// A view whose content is a WinUI element the Host places in the node's content box.
class XamlModuleView : public ModuleView {
 public:
  using ModuleView::ModuleView;

  // Called once, on the UI thread, after the view is constructed.
  virtual winrt::Microsoft::UI::Xaml::UIElement Element() = 0;

  void Draw(ID2D1DeviceContext*, float, float) final {}
};

}  // namespace whisker
