#include "text.h"

#include "paint.h"

#include <cmath>
#include <vector>

#pragma comment(lib, "dwrite.lib")

namespace whisker {

namespace {

constexpr float kUnbounded = 1.0e6f;

std::wstring ResolveFamily(const WhiskerStringRef* families, size_t count) {
  for (size_t i = 0; i < count; ++i) {
    auto family = Widen(families[i]);
    if (family == L"system" || family == L"system-ui" || family == L"sans-serif") {
      return L"Segoe UI";
    }
    if (family == L"serif") return L"Times New Roman";
    if (family == L"monospace") return L"Consolas";
    if (!family.empty()) return family;
  }
  return L"Segoe UI";
}

DWRITE_FONT_STYLE FontStyle(uint8_t style) {
  switch (style) {
    case 1: return DWRITE_FONT_STYLE_ITALIC;
    case 2: return DWRITE_FONT_STYLE_OBLIQUE;
    default: return DWRITE_FONT_STYLE_NORMAL;
  }
}

}  // namespace

std::wstring Widen(WhiskerStringRef value) {
  if (!value.ptr || value.len == 0) return {};
  int length = MultiByteToWideChar(CP_UTF8, 0, value.ptr, static_cast<int>(value.len), nullptr, 0);
  std::wstring out(length, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, value.ptr, static_cast<int>(value.len), out.data(), length);
  return out;
}

TextStyle TextStyle::From(const WhiskerMobileText& text) {
  TextStyle style;
  style.text = Widen(text.text);
  style.family = ResolveFamily(text.font_families, text.font_family_count);
  style.size = text.font_size;
  style.weight = text.font_weight;
  style.style = text.font_style;
  style.wrap = text.wrap != 0;
  style.ellipsis = text.overflow != 0;
  style.max_lines = text.max_lines;
  style.line_height = text.line_height;
  style.alignment = text.alignment;
  style.direction = text.direction;
  style.color = OwnedColor(text.color);
  return style;
}

TextStyle TextStyle::From(const WhiskerMobileMeasureRequest& request) {
  TextStyle style;
  style.text = Widen(request.text);
  style.family = ResolveFamily(request.font_families, request.font_family_count);
  style.size = request.font_size;
  style.weight = request.font_weight;
  style.style = request.font_style;
  style.wrap = request.wrap != 0;
  style.ellipsis = request.overflow != 0;
  style.max_lines = request.max_lines;
  style.line_height = request.line_height;
  style.alignment = request.alignment;
  style.direction = request.direction;
  return style;
}

TextEngine::TextEngine() {
  winrt::check_hresult(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                           reinterpret_cast<::IUnknown**>(factory_.put())));
}

winrt::com_ptr<IDWriteTextLayout> TextEngine::Layout(const TextStyle& style,
                                                     float max_width) const {
  winrt::com_ptr<IDWriteTextFormat> format;
  winrt::check_hresult(factory_->CreateTextFormat(
      style.family.c_str(), nullptr, static_cast<DWRITE_FONT_WEIGHT>(style.weight),
      FontStyle(style.style), DWRITE_FONT_STRETCH_NORMAL, style.size, L"", format.put()));
  format->SetWordWrapping(style.wrap ? DWRITE_WORD_WRAPPING_WRAP : DWRITE_WORD_WRAPPING_NO_WRAP);
  if (style.ellipsis) {
    DWRITE_TRIMMING trimming{DWRITE_TRIMMING_GRANULARITY_CHARACTER, 0, 0};
    winrt::com_ptr<IDWriteInlineObject> sign;
    winrt::check_hresult(factory_->CreateEllipsisTrimmingSign(format.get(), sign.put()));
    format->SetTrimming(&trimming, sign.get());
  }
  if (style.line_height > 0) {
    format->SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, style.line_height,
                           style.line_height * 0.8f);
  }
  bool rtl = style.direction == 2;
  if (rtl) format->SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
  switch (style.alignment) {
    case 1: format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING); break;
    case 2:
      format->SetTextAlignment(rtl ? DWRITE_TEXT_ALIGNMENT_TRAILING : DWRITE_TEXT_ALIGNMENT_LEADING);
      break;
    case 3:
      format->SetTextAlignment(rtl ? DWRITE_TEXT_ALIGNMENT_LEADING : DWRITE_TEXT_ALIGNMENT_TRAILING);
      break;
    case 4: format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER); break;
    default: break;
  }
  winrt::com_ptr<IDWriteTextLayout> layout;
  winrt::check_hresult(factory_->CreateTextLayout(style.text.c_str(),
                                                  static_cast<UINT32>(style.text.size()),
                                                  format.get(), max_width, kUnbounded,
                                                  layout.put()));
  return layout;
}

void TextEngine::Measure(const WhiskerMobileMeasureRequest& request,
                         WhiskerMobileMeasureResponse& response) const {
  response.key = request.key;
  response.environment_epoch = request.environment_epoch;
  bool known_width = request.known_mask & 1;
  bool known_height = request.known_mask & 2;
  if (request.kind != WHISKER_MEASURE_TEXT) {
    if (request.intrinsic_mask != 3) {
      response.status = WHISKER_MEASURE_UNSUPPORTED;
      return;
    }
    response.status = WHISKER_MEASURE_READY;
    response.width = known_width ? request.known_width : request.intrinsic_width;
    response.height = known_height ? request.known_height : request.intrinsic_height;
    return;
  }
  auto style = TextStyle::From(request);
  float width = kUnbounded;
  if (style.wrap) {
    switch (request.available_width_kind) {
      case 0: width = request.available_width; break;
      case 1: width = 1; break;
      default: break;
    }
  }
  if (known_width) width = request.known_width;
  auto layout = Layout(style, width);
  DWRITE_TEXT_METRICS metrics{};
  layout->GetMetrics(&metrics);
  UINT32 line_count = 0;
  layout->GetLineMetrics(nullptr, 0, &line_count);
  std::vector<DWRITE_LINE_METRICS> lines(line_count);
  layout->GetLineMetrics(lines.data(), line_count, &line_count);
  if (style.max_lines > 0 && lines.size() > style.max_lines) lines.resize(style.max_lines);
  float top = 0;
  float last_baseline = 0;
  for (auto const& line : lines) {
    last_baseline = top + line.baseline;
    top += line.height;
  }
  response.status = WHISKER_MEASURE_READY;
  response.width = known_width ? request.known_width
                               : std::ceil(metrics.widthIncludingTrailingWhitespace);
  response.height = known_height ? request.known_height : std::ceil(top);
  response.first_baseline = lines.empty() ? 0 : lines.front().baseline;
  response.last_baseline = last_baseline;
  response.metrics_mask = 7;
}

static float VisibleHeight(IDWriteTextLayout* layout, uint32_t max_lines) {
  UINT32 count = 0;
  layout->GetLineMetrics(nullptr, 0, &count);
  std::vector<DWRITE_LINE_METRICS> lines(count);
  layout->GetLineMetrics(lines.data(), count, &count);
  float height = 0;
  for (uint32_t i = 0; i < count && i < max_lines; ++i) height += lines[i].height;
  return height;
}

void TextEngine::Draw(ID2D1DeviceContext* context, const TextStyle& style, float width) const {
  // Half a pixel of slack keeps DirectWrite from wrapping text measured at exactly this width.
  auto layout = Layout(style, style.wrap ? width + 0.5f : kUnbounded);
  if (!style.wrap) layout->SetMaxWidth(width);
  if (style.max_lines > 0) layout->SetMaxHeight(VisibleHeight(layout.get(), style.max_lines));
  winrt::com_ptr<ID2D1SolidColorBrush> brush;
  context->CreateSolidColorBrush(ToD2DColor(style.color), brush.put());
  context->DrawTextLayout({0, 0}, layout.get(), brush.get(),
                          D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
}

}  // namespace whisker
