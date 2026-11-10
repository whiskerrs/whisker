#include "text.h"

#include "paint.h"
#include "value.h"

#include <algorithm>
#include <cmath>
#include <vector>

#include <d2d1effects.h>

#pragma comment(lib, "dwrite.lib")
#pragma comment(lib, "dxguid.lib")

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

constexpr wchar_t kIndentAnchor = L'\uFFFC';
constexpr auto kWordWrappingCharacter = static_cast<DWRITE_WORD_WRAPPING>(4);

std::wstring ResolveFamily(const std::wstring& family) {
  if (family == L"system" || family == L"system-ui" || family == L"sans-serif") return L"Segoe UI";
  if (family == L"serif") return L"Times New Roman";
  if (family == L"monospace") return L"Consolas";
  return family;
}

std::vector<std::pair<uint32_t, uint32_t>> Features(const WhiskerMobileFontFeature* features,
                                                    size_t count) {
  std::vector<std::pair<uint32_t, uint32_t>> out;
  for (size_t i = 0; features && i < count; ++i) {
    auto const& tag = features[i].tag;
    out.emplace_back(DWRITE_MAKE_OPENTYPE_TAG(tag[0], tag[1], tag[2], tag[3]), features[i].value);
  }
  return out;
}

std::optional<D2D1_COLOR_F> ColorValue(const WhiskerValueRaw* value) {
  if (!value) return std::nullopt;
  if (value->type == WHISKER_VALUE_STRING) {
    WhiskerMobileColor named{};
    named.name = value->v.s;
    return ToD2DColor(named);
  }
  if (value->type != WHISKER_VALUE_ARRAY || value->v.array.count != 4) return std::nullopt;
  auto items = value->v.array.items;
  return D2D1_COLOR_F{Number(&items[0]) / 255.0f, Number(&items[1]) / 255.0f,
                      Number(&items[2]) / 255.0f, Number(&items[3])};
}

std::vector<TextRun> Runs(const WhiskerValueRaw* paragraph) {
  std::vector<TextRun> runs;
  auto list = MapEntry(paragraph, "runs");
  if (!list || list->type != WHISKER_VALUE_ARRAY) return runs;
  for (size_t i = 0; i < list->v.array.count; ++i) {
    auto raw = &list->v.array.items[i];
    TextRun run;
    run.start = static_cast<UINT32>(Number(MapEntry(raw, "start")));
    run.end = static_cast<UINT32>(Number(MapEntry(raw, "end")));
    if (run.end <= run.start) continue;
    if (auto families = MapEntry(raw, "families"); families && families->type == WHISKER_VALUE_ARRAY) {
      for (size_t f = 0; f < families->v.array.count && run.family.empty(); ++f) {
        auto const& family = families->v.array.items[f];
        if (family.type == WHISKER_VALUE_STRING) run.family = ResolveFamily(Widen(family.v.s));
      }
    }
    run.size = Number(MapEntry(raw, "size"));
    run.weight = static_cast<uint16_t>(Number(MapEntry(raw, "weight"), 400));
    run.italic = Flag(MapEntry(raw, "italic"));
    run.spacing = Number(MapEntry(raw, "spacing"));
    if (auto features = MapEntry(raw, "features"); features && features->type == WHISKER_VALUE_MAP) {
      for (size_t f = 0; f < features->v.map.count; ++f) {
        auto tag = View(features->v.map.entries[f].key);
        if (tag.size() != 4) continue;
        run.features.emplace_back(DWRITE_MAKE_OPENTYPE_TAG(tag[0], tag[1], tag[2], tag[3]),
                                  static_cast<uint32_t>(Number(&features->v.map.entries[f].value)));
      }
    }
    if (auto paint = MapEntry(raw, "paint"); paint && paint->type == WHISKER_VALUE_MAP) {
      run.color = ColorValue(MapEntry(paint, "color"));
      run.background = ColorValue(MapEntry(paint, "background"));
      run.decoration.underline = Flag(MapEntry(paint, "underline"));
      run.decoration.line_through = Flag(MapEntry(paint, "strike"));
      run.decoration.style = static_cast<uint32_t>(Number(MapEntry(paint, "decorationStyle")));
      run.decoration.color = ColorValue(MapEntry(paint, "decorationColor"))
                                 .value_or(run.color.value_or(D2D1_COLOR_F{0, 0, 0, 1}));
    }
    runs.push_back(std::move(run));
  }
  return runs;
}

// Reserves the first-line indent as an empty inline box.
struct IndentSpacer : winrt::implements<IndentSpacer, IDWriteInlineObject> {
  explicit IndentSpacer(float width) : width_(width) {}
  HRESULT __stdcall Draw(void*, IDWriteTextRenderer*, FLOAT, FLOAT, BOOL, BOOL,
                         ::IUnknown*) override {
    return S_OK;
  }
  HRESULT __stdcall GetMetrics(DWRITE_INLINE_OBJECT_METRICS* metrics) override {
    *metrics = {width_, 0, 0, FALSE};
    return S_OK;
  }
  HRESULT __stdcall GetOverhangMetrics(DWRITE_OVERHANG_METRICS* overhangs) override {
    *overhangs = {};
    return S_OK;
  }
  HRESULT __stdcall GetBreakConditions(DWRITE_BREAK_CONDITION* before,
                                       DWRITE_BREAK_CONDITION* after) override {
    *before = DWRITE_BREAK_CONDITION_NEUTRAL;
    *after = DWRITE_BREAK_CONDITION_NEUTRAL;
    return S_OK;
  }
  float width_;
};

winrt::com_ptr<IDWriteTypography> Typography(IDWriteFactory* factory,
                                             const std::vector<std::pair<uint32_t, uint32_t>>& features) {
  winrt::com_ptr<IDWriteTypography> typography;
  winrt::check_hresult(factory->CreateTypography(typography.put()));
  for (auto [tag, value] : features) {
    typography->AddFontFeature({static_cast<DWRITE_FONT_FEATURE_TAG>(tag), value});
  }
  return typography;
}

float VisibleHeight(IDWriteTextLayout* layout, uint32_t max_lines) {
  UINT32 count = 0;
  layout->GetLineMetrics(nullptr, 0, &count);
  std::vector<DWRITE_LINE_METRICS> lines(count);
  layout->GetLineMetrics(lines.data(), count, &count);
  float height = 0;
  for (uint32_t i = 0; i < count && (max_lines == 0 || i < max_lines); ++i) {
    height += lines[i].height;
  }
  return height;
}

void StrokeDecoration(ID2D1DeviceContext* context, ID2D1Factory* factory, ID2D1Brush* brush,
                      float x0, float x1, float y, float thickness, uint32_t style) {
  if (style == 4) {
    winrt::com_ptr<ID2D1PathGeometry> wave;
    winrt::check_hresult(factory->CreatePathGeometry(wave.put()));
    winrt::com_ptr<ID2D1GeometrySink> sink;
    winrt::check_hresult(wave->Open(sink.put()));
    float period = thickness * 4;
    sink->BeginFigure({x0, y}, D2D1_FIGURE_BEGIN_HOLLOW);
    for (float x = x0, sign = -1; x < x1; x += period / 2, sign = -sign) {
      sink->AddQuadraticBezier({{x + period / 4, y + sign * thickness * 1.5f}, {x + period / 2, y}});
    }
    sink->EndFigure(D2D1_FIGURE_END_OPEN);
    sink->Close();
    context->PushAxisAlignedClip({x0, y - thickness * 3, x1, y + thickness * 3},
                                 D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    context->DrawGeometry(wave.get(), brush, thickness);
    context->PopAxisAlignedClip();
    return;
  }
  winrt::com_ptr<ID2D1StrokeStyle> stroke;
  if (style == 2 || style == 3) {
    auto properties = D2D1::StrokeStyleProperties(
        style == 2 ? D2D1_CAP_STYLE_ROUND : D2D1_CAP_STYLE_FLAT,
        style == 2 ? D2D1_CAP_STYLE_ROUND : D2D1_CAP_STYLE_FLAT,
        style == 2 ? D2D1_CAP_STYLE_ROUND : D2D1_CAP_STYLE_FLAT, D2D1_LINE_JOIN_MITER, 10,
        style == 2 ? D2D1_DASH_STYLE_DOT : D2D1_DASH_STYLE_DASH);
    winrt::check_hresult(factory->CreateStrokeStyle(properties, nullptr, 0, stroke.put()));
  }
  if (style == 1) {
    context->DrawLine({x0, y - thickness}, {x1, y - thickness}, brush, thickness);
    context->DrawLine({x0, y + thickness}, {x1, y + thickness}, brush, thickness);
    return;
  }
  context->DrawLine({x0, y}, {x1, y}, brush, thickness, stroke.get());
}

// Draws CSS decoration lines under each line's part of `[start, start + length)`.
void DrawDecoration(ID2D1DeviceContext* context, IDWriteTextLayout* layout, UINT32 start,
                    UINT32 length, const TextDecoration& decoration) {
  if (!decoration.underline && !decoration.line_through) return;
  winrt::com_ptr<ID2D1Factory> factory;
  context->GetFactory(factory.put());
  winrt::com_ptr<IDWriteFontCollection> collection;
  WCHAR family[256] = {};
  layout->GetFontCollection(start, collection.put());
  layout->GetFontFamilyName(start, family, 256);
  DWRITE_FONT_WEIGHT weight;
  DWRITE_FONT_STYLE font_style;
  FLOAT size;
  layout->GetFontWeight(start, &weight);
  layout->GetFontStyle(start, &font_style);
  layout->GetFontSize(start, &size);
  DWRITE_FONT_METRICS metrics{};
  metrics.designUnitsPerEm = 2048;
  metrics.underlinePosition = -150;
  metrics.underlineThickness = 100;
  metrics.strikethroughPosition = 600;
  metrics.strikethroughThickness = 100;
  UINT32 index = 0;
  BOOL exists = FALSE;
  if (collection && SUCCEEDED(collection->FindFamilyName(family, &index, &exists)) && exists) {
    winrt::com_ptr<IDWriteFontFamily> font_family;
    winrt::com_ptr<IDWriteFont> font;
    if (SUCCEEDED(collection->GetFontFamily(index, font_family.put())) &&
        SUCCEEDED(font_family->GetFirstMatchingFont(weight, DWRITE_FONT_STRETCH_NORMAL, font_style,
                                                    font.put()))) {
      font->GetMetrics(&metrics);
    }
  }
  float unit = size / metrics.designUnitsPerEm;
  UINT32 line_count = 0;
  layout->GetLineMetrics(nullptr, 0, &line_count);
  std::vector<DWRITE_LINE_METRICS> lines(line_count);
  layout->GetLineMetrics(lines.data(), line_count, &line_count);
  UINT32 count = 0;
  layout->HitTestTextRange(start, length, 0, 0, nullptr, 0, &count);
  std::vector<DWRITE_HIT_TEST_METRICS> segments(count);
  layout->HitTestTextRange(start, length, 0, 0, segments.data(), count, &count);
  winrt::com_ptr<ID2D1SolidColorBrush> brush;
  context->CreateSolidColorBrush(decoration.color, brush.put());
  for (auto const& segment : segments) {
    float top = 0;
    float baseline = segment.top;
    for (auto const& line : lines) {
      if (segment.top < top + line.height - 0.01f) {
        baseline = top + line.baseline;
        break;
      }
      top += line.height;
    }
    float x0 = segment.left;
    float x1 = segment.left + segment.width;
    if (decoration.underline) {
      float thickness = std::max(1.0f, metrics.underlineThickness * unit);
      StrokeDecoration(context, factory.get(), brush.get(), x0, x1,
                       baseline - metrics.underlinePosition * unit + thickness / 2, thickness,
                       decoration.style);
    }
    if (decoration.line_through) {
      float thickness = std::max(1.0f, metrics.strikethroughThickness * unit);
      StrokeDecoration(context, factory.get(), brush.get(), x0, x1,
                       baseline - metrics.strikethroughPosition * unit, thickness,
                       decoration.style);
    }
  }
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
  style.word_break = text.word_break;
  style.letter_spacing = text.letter_spacing;
  style.indent = text.indent_logical_pixels;
  style.indent_percentage = text.indent_percentage;
  style.features = Features(text.font_features, text.font_feature_count);
  style.decoration.underline = text.decoration_flags & 1;
  style.decoration.line_through = text.decoration_flags & 2;
  style.decoration.style = text.decoration_style;
  style.decoration.color = ToD2DColor(text.decoration_color);
  if (text.shadow_flags & 1) {
    style.shadow = TextShadow{text.shadow_offset_x, text.shadow_offset_y, text.shadow_blur_radius,
                              ToD2DColor(text.shadow_color)};
  }
  style.runs = Runs(text.paragraph);
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
  style.color = ToD2DColor(text.color);
  return style;
}

TextStyle TextStyle::From(const WhiskerMobileMeasureRequest& request) {
  TextStyle style;
  style.text = Widen(request.text);
  style.word_break = request.word_break;
  style.letter_spacing = request.letter_spacing;
  style.indent = request.indent_logical_pixels;
  style.indent_percentage = request.indent_percentage;
  style.features = Features(request.font_features, request.font_feature_count);
  style.runs = Runs(request.paragraph);
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

float TextStyle::PaintOverflow() const {
  if (!shadow) return 0;
  return std::ceil(std::max(std::abs(shadow->x), std::abs(shadow->y)) + shadow->blur * 1.5f);
}

TextEngine::TextEngine() {
  winrt::check_hresult(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                           reinterpret_cast<::IUnknown**>(factory_.put())));
}

winrt::com_ptr<IDWriteTextLayout> TextEngine::Layout(const TextStyle& style, float max_width,
                                                     float basis) const {
  winrt::com_ptr<IDWriteTextFormat> format;
  winrt::check_hresult(factory_->CreateTextFormat(
      style.family.c_str(), nullptr, static_cast<DWRITE_FONT_WEIGHT>(style.weight),
      FontStyle(style.style), DWRITE_FONT_STRETCH_NORMAL, style.size, L"", format.put()));
  format->SetWordWrapping(!style.wrap              ? DWRITE_WORD_WRAPPING_NO_WRAP
                          : style.word_break == 1 ? kWordWrappingCharacter
                                                  : DWRITE_WORD_WRAPPING_WRAP);
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
  float indent = style.indent + style.indent_percentage / 100 * basis;
  UINT32 shift = indent > 0 ? 1 : 0;
  auto text = shift ? kIndentAnchor + style.text : style.text;
  auto length = static_cast<UINT32>(style.text.size());
  winrt::com_ptr<IDWriteTextLayout> layout;
  winrt::check_hresult(factory_->CreateTextLayout(text.c_str(), static_cast<UINT32>(text.size()),
                                                  format.get(), max_width, kUnbounded,
                                                  layout.put()));
  if (shift) layout->SetInlineObject(winrt::make<IndentSpacer>(indent).get(), {0, 1});
  auto layout1 = layout.as<IDWriteTextLayout1>();
  if (style.letter_spacing != 0) layout1->SetCharacterSpacing(0, style.letter_spacing, 0, {shift, length});
  if (!style.features.empty()) {
    layout->SetTypography(Typography(factory_.get(), style.features).get(), {shift, length});
  }
  for (auto const& run : style.runs) {
    DWRITE_TEXT_RANGE range{run.start + shift, std::min(run.end, length) - std::min(run.start, length)};
    if (range.length == 0) continue;
    if (!run.family.empty()) layout->SetFontFamilyName(run.family.c_str(), range);
    if (run.size > 0) layout->SetFontSize(run.size, range);
    layout->SetFontWeight(static_cast<DWRITE_FONT_WEIGHT>(run.weight), range);
    layout->SetFontStyle(run.italic ? DWRITE_FONT_STYLE_ITALIC : DWRITE_FONT_STYLE_NORMAL, range);
    layout1->SetCharacterSpacing(0, run.spacing, 0, range);
    if (!run.features.empty()) {
      layout->SetTypography(Typography(factory_.get(), run.features).get(), range);
    }
  }
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
  auto basis = request.available_width_kind == 0 ? request.available_width : 0.0f;
  auto layout = Layout(style, width, known_width ? request.known_width : basis);
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

void TextEngine::Draw(ID2D1DeviceContext* context, const TextStyle& style, float width) const {
  // Half a pixel of slack keeps DirectWrite from wrapping text measured at exactly this width.
  auto layout = Layout(style, style.wrap ? width + 0.5f : kUnbounded, width);
  if (!style.wrap) layout->SetMaxWidth(width);
  if (style.max_lines > 0) layout->SetMaxHeight(VisibleHeight(layout.get(), style.max_lines));
  UINT32 shift = style.indent + style.indent_percentage / 100 * width > 0 ? 1 : 0;
  auto length = static_cast<UINT32>(style.text.size());
  for (auto const& run : style.runs) {
    if (!run.color || run.end <= run.start || run.start >= length) continue;
    winrt::com_ptr<ID2D1SolidColorBrush> brush;
    context->CreateSolidColorBrush(*run.color, brush.put());
    layout->SetDrawingEffect(brush.get(), {run.start + shift, std::min(run.end, length) - run.start});
  }
  winrt::com_ptr<ID2D1SolidColorBrush> brush;
  context->CreateSolidColorBrush(style.color, brush.put());
  auto paint = [&] {
    for (auto const& run : style.runs) {
      if (!run.background || run.end <= run.start || run.start >= length) continue;
      UINT32 count = 0;
      layout->HitTestTextRange(run.start + shift, run.end - run.start, 0, 0, nullptr, 0, &count);
      std::vector<DWRITE_HIT_TEST_METRICS> segments(count);
      layout->HitTestTextRange(run.start + shift, run.end - run.start, 0, 0, segments.data(), count,
                               &count);
      winrt::com_ptr<ID2D1SolidColorBrush> fill;
      context->CreateSolidColorBrush(*run.background, fill.put());
      for (auto const& segment : segments) {
        context->FillRectangle({segment.left, segment.top, segment.left + segment.width,
                                segment.top + segment.height},
                               fill.get());
      }
    }
    context->DrawTextLayout({0, 0}, layout.get(), brush.get(),
                            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
    if (style.runs.empty()) {
      DrawDecoration(context, layout.get(), shift, length, style.decoration);
    }
    for (auto const& run : style.runs) {
      if (run.end > run.start && run.start < length) {
        DrawDecoration(context, layout.get(), run.start + shift, run.end - run.start, run.decoration);
      }
    }
  };
  if (!style.shadow) {
    paint();
    return;
  }
  winrt::com_ptr<ID2D1Image> target;
  context->GetTarget(target.put());
  D2D1_MATRIX_3X2_F transform;
  context->GetTransform(&transform);
  winrt::com_ptr<ID2D1CommandList> list;
  winrt::check_hresult(context->CreateCommandList(list.put()));
  context->SetTarget(list.get());
  context->SetTransform(D2D1::Matrix3x2F::Identity());
  paint();
  context->SetTarget(target.get());
  context->SetTransform(transform);
  winrt::check_hresult(list->Close());
  winrt::com_ptr<ID2D1Effect> shadow;
  winrt::check_hresult(context->CreateEffect(CLSID_D2D1Shadow, shadow.put()));
  shadow->SetInput(0, list.get());
  shadow->SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION, style.shadow->blur / 2);
  auto const& c = style.shadow->color;
  shadow->SetValue(D2D1_SHADOW_PROP_COLOR, D2D1::Vector4F(c.r, c.g, c.b, c.a));
  winrt::com_ptr<ID2D1Image> image;
  shadow->GetOutput(image.put());
  D2D1_POINT_2F offset{style.shadow->x, style.shadow->y};
  context->DrawImage(image.get(), &offset);
  context->DrawImage(list.get());
}

}  // namespace whisker
