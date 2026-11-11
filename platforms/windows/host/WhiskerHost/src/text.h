#pragma once

#include <windows.h>

#include <d2d1_1.h>
#include <dwrite_1.h>
#include <optional>
#include <string>
#include <utility>
#include <vector>
#include <winrt/base.h>

#include "whisker_mobile.h"

namespace whisker {

std::wstring Widen(WhiskerStringRef value);

struct TextDecoration {
  bool underline = false;
  bool line_through = false;
  uint32_t style = 0;
  D2D1_COLOR_F color{};
};

struct TextShadow {
  float x = 0;
  float y = 0;
  float blur = 0;
  D2D1_COLOR_F color{};
};

// One styled range of a rich paragraph, in UTF-16 offsets.
struct TextRun {
  UINT32 start = 0;
  UINT32 end = 0;
  std::wstring family;
  float size = 0;
  uint16_t weight = 400;
  bool italic = false;
  float spacing = 0;
  std::vector<std::pair<uint32_t, uint32_t>> features;
  std::optional<D2D1_COLOR_F> color;
  std::optional<D2D1_COLOR_F> background;
  TextDecoration decoration;
};

struct TextContent {
  std::wstring text;
  std::wstring family = L"Segoe UI";
  float size = 14;
  uint16_t weight = 400;
  uint8_t style = 0;
  bool wrap = true;
  uint8_t word_break = 0;
  bool ellipsis = false;
  uint32_t max_lines = 0;
  float line_height = 0;
  float letter_spacing = 0;
  float indent = 0;
  float indent_percentage = 0;
  std::vector<std::pair<uint32_t, uint32_t>> features;
  uint32_t alignment = 0;
  uint32_t direction = 0;
  D2D1_COLOR_F color{0, 0, 0, 1};
  TextDecoration decoration;
  std::optional<TextShadow> shadow;
  std::vector<TextRun> runs;

  static TextContent From(const WhiskerMobileText& text);
  static TextContent From(const WhiskerMobileMeasureRequest& request);

  // How far the painted text may reach past its content box on every side.
  float PaintOverflow() const;
};

class TextEngine {
 public:
  TextEngine();

  // `basis` resolves a percentage indent.
  winrt::com_ptr<IDWriteTextLayout> Layout(const TextContent& style, float max_width,
                                           float basis) const;
  void Measure(const WhiskerMobileMeasureRequest& request,
               WhiskerMobileMeasureResponse& response) const;
  void Draw(ID2D1DeviceContext* context, const TextContent& style, float width) const;

 private:
  winrt::com_ptr<IDWriteFactory> factory_;
};

}  // namespace whisker
