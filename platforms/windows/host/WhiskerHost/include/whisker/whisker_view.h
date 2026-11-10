#pragma once

#include <memory>

#include <winrt/Microsoft.UI.Xaml.Controls.h>

namespace whisker {

// Drives the Rust runtime linked into the app and presents its frames inside `host`.
class WhiskerView {
 public:
  explicit WhiskerView(winrt::Microsoft::UI::Xaml::Controls::Panel const& host);
  ~WhiskerView();
  WhiskerView(WhiskerView const&) = delete;
  WhiskerView& operator=(WhiskerView const&) = delete;

 private:
  struct Impl;
  std::unique_ptr<Impl> impl_;
};

}  // namespace whisker
