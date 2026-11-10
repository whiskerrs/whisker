#pragma once

#include <windows.h>

#include <d2d1_1.h>
#include <dwrite.h>
#include <string>
#include <winrt/base.h>

#include "whisker_mobile.h"

namespace whisker {

std::wstring Widen(WhiskerStringRef value);

struct TextStyle {
  std::wstring text;
  std::wstring family = L"Segoe UI";
  float size = 14;
  uint16_t weight = 400;
  uint8_t style = 0;
  bool wrap = true;
  float line_height = 0;
  uint32_t alignment = 0;
  uint32_t direction = 0;
  WhiskerMobileColor color{};

  static TextStyle From(const WhiskerMobileText& text);
  static TextStyle From(const WhiskerMobileMeasureRequest& request);
};

class TextEngine {
 public:
  TextEngine();

  winrt::com_ptr<IDWriteTextLayout> Layout(const TextStyle& style, float max_width) const;
  void Measure(const WhiskerMobileMeasureRequest& request,
               WhiskerMobileMeasureResponse& response) const;
  void Draw(ID2D1DeviceContext* context, const TextStyle& style, float width) const;

 private:
  winrt::com_ptr<IDWriteFactory> factory_;
};

}  // namespace whisker
