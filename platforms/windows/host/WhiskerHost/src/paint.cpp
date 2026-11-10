#include "paint.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace whisker {

namespace {

enum BorderStyle : uint32_t {
  kNone = 0,
  kHidden = 1,
  kSolid = 2,
  kDashed = 3,
  kDotted = 4,
  kDouble = 5,
  kGroove = 6,
  kRidge = 7,
  kInset = 8,
  kOutset = 9,
};

struct Band {
  float start;
  float end;
  float shade;
};

float Resolve(const WhiskerMobileLengthPercentage& value, float basis) {
  return value.length + value.fraction * basis;
}

D2D1_COLOR_F NamedColor(std::string_view name) {
  std::string lower(name);
  std::transform(lower.begin(), lower.end(), lower.begin(),
                 [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  if (lower == "black") return {0, 0, 0, 1};
  if (lower == "white") return {1, 1, 1, 1};
  if (lower == "red") return {1, 0, 0, 1};
  if (lower == "green") return {0, 128 / 255.0f, 0, 1};
  if (lower == "gold") return {1, 215 / 255.0f, 0, 1};
  if (lower == "blue") return {0, 0, 1, 1};
  return {0, 0, 0, 0};
}

// Lightens toward white for positive `amount` and darkens toward black for negative.
D2D1_COLOR_F Shade(D2D1_COLOR_F color, float amount) {
  auto channel = [amount](float value) {
    return amount < 0 ? value * (1 + amount) : value + (1 - value) * amount;
  };
  return {channel(color.r), channel(color.g), channel(color.b), color.a};
}

// Solid bands of `style` with depth 0 at the outer edge.
std::vector<Band> Bands(uint32_t style, bool top_or_left) {
  float dark = -0.35f;
  float light = 0.35f;
  switch (style) {
    case kSolid: return {{0, 1, 0}};
    case kDouble: return {{0, 1 / 3.0f, 0}, {2 / 3.0f, 1, 0}};
    case kInset: return {{0, 1, top_or_left ? dark : light}};
    case kOutset: return {{0, 1, top_or_left ? light : dark}};
    case kGroove:
      return {{0, 0.5f, top_or_left ? dark : light}, {0.5f, 1, top_or_left ? light : dark}};
    case kRidge:
      return {{0, 0.5f, top_or_left ? light : dark}, {0.5f, 1, top_or_left ? dark : light}};
    default: return {};
  }
}

winrt::com_ptr<ID2D1PathGeometry> Polygon(ID2D1Factory* factory,
                                          std::initializer_list<D2D1_POINT_2F> points) {
  winrt::com_ptr<ID2D1PathGeometry> geometry;
  winrt::check_hresult(factory->CreatePathGeometry(geometry.put()));
  winrt::com_ptr<ID2D1GeometrySink> sink;
  winrt::check_hresult(geometry->Open(sink.put()));
  auto it = points.begin();
  sink->BeginFigure(*it, D2D1_FIGURE_BEGIN_FILLED);
  for (++it; it != points.end(); ++it) sink->AddLine(*it);
  sink->EndFigure(D2D1_FIGURE_END_CLOSED);
  winrt::check_hresult(sink->Close());
  return geometry;
}

// The trapezoid one side owns, joined to its neighbours along the corner diagonals.
winrt::com_ptr<ID2D1PathGeometry> SideRegion(ID2D1Factory* factory, int side,
                                             const std::array<float, 4>& widths, float w,
                                             float h) {
  auto [top, right, bottom, left] = widths;
  switch (side) {
    case 0: return Polygon(factory, {{0, 0}, {w, 0}, {w - right, top}, {left, top}});
    case 1: return Polygon(factory, {{w, 0}, {w, h}, {w - right, h - bottom}, {w - right, top}});
    case 2: return Polygon(factory, {{w, h}, {0, h}, {left, h - bottom}, {w - right, h - bottom}});
    default: return Polygon(factory, {{0, h}, {0, 0}, {left, top}, {left, h - bottom}});
  }
}

winrt::com_ptr<ID2D1GeometryGroup> Ring(ID2D1Factory* factory, const RoundedRect& outer,
                                        const RoundedRect& inner) {
  auto a = outer.Geometry(factory);
  auto b = inner.Geometry(factory);
  ID2D1Geometry* parts[] = {a.get(), b.get()};
  winrt::com_ptr<ID2D1GeometryGroup> group;
  winrt::check_hresult(factory->CreateGeometryGroup(D2D1_FILL_MODE_ALTERNATE, parts, 2, group.put()));
  return group;
}

template <typename Draw>
void WithMask(ID2D1DeviceContext* context, ID2D1Geometry* mask, Draw&& draw) {
  context->PushLayer(D2D1::LayerParameters1(D2D1::InfiniteRect(), mask), nullptr);
  draw();
  context->PopLayer();
}

// Paints one dashed or dotted side, phased along the clockwise perimeter from the top-left.
void PaintPatternedSide(ID2D1DeviceContext* context, ID2D1SolidColorBrush* brush, int side,
                        uint32_t style, const std::array<float, 4>& widths, float w, float h) {
  float width = widths[side];
  if (width <= 0 || (style != kDashed && style != kDotted)) return;
  const float starts[] = {0, w, w + h, 2 * w + h};
  const float lengths[] = {w, h, w, h};
  float start = starts[side];
  float length = lengths[side];
  auto point = [&](float position, float depth) -> D2D1_POINT_2F {
    float along = position - start;
    switch (side) {
      case 0: return {along, depth};
      case 1: return {w - depth, along};
      case 2: return {w - along, h - depth};
      default: return {depth, h - along};
    }
  };
  winrt::com_ptr<ID2D1Factory> factory;
  context->GetFactory(factory.put());
  auto region = SideRegion(factory.get(), side, widths, w, h);
  bool dotted = style == kDotted;
  float period = width * (dotted ? 2.0f : 4.0f);
  WithMask(context, region.get(), [&] {
    for (float position = std::floor(start / period) * period; position < start + length;
         position += period) {
      if (dotted) {
        auto center = point(position + width, width / 2);
        context->FillEllipse(D2D1::Ellipse(center, width / 2, width / 2), brush);
      } else {
        auto from = point(std::max(position, start), 0);
        auto to = point(std::min(position + 3 * width, start + length), width);
        context->FillRectangle(D2D1::RectF(std::min(from.x, to.x), std::min(from.y, to.y),
                                           std::max(from.x, to.x), std::max(from.y, to.y)),
                               brush);
      }
    }
  });
}

void PaintBorder(ID2D1DeviceContext* context, const WhiskerMobileBoxPaint& paint, float w,
                 float h) {
  auto widths = BorderWidths(paint, w);
  if (std::all_of(widths.begin(), widths.end(), [](float v) { return v <= 0; })) return;
  winrt::com_ptr<ID2D1Factory> factory;
  context->GetFactory(factory.put());
  std::array<D2D1_COLOR_F, 4> colors;
  for (int side = 0; side < 4; ++side) colors[side] = ToD2DColor(paint.colors[side]);
  auto outer = RoundedRect::From(&paint, w, h);
  std::vector<std::pair<float, float>> ranges;
  for (int side = 0; side < 4; ++side) {
    for (auto const& band : Bands(paint.styles[side], side == 0 || side == 3)) {
      std::pair range{band.start, band.end};
      if (std::find(ranges.begin(), ranges.end(), range) == ranges.end()) ranges.push_back(range);
    }
  }
  winrt::com_ptr<ID2D1SolidColorBrush> brush;
  context->CreateSolidColorBrush({0, 0, 0, 0}, brush.put());
  for (auto [start, end] : ranges) {
    std::array<std::optional<D2D1_COLOR_F>, 4> band_colors;
    for (int side = 0; side < 4; ++side) {
      if (widths[side] <= 0) continue;
      for (auto const& band : Bands(paint.styles[side], side == 0 || side == 3)) {
        if (band.start == start && band.end == end) band_colors[side] = Shade(colors[side], band.shade);
      }
    }
    auto ring = Ring(factory.get(),
                     outer.Shrink(widths[0] * start, widths[1] * start, widths[2] * start,
                                  widths[3] * start),
                     outer.Shrink(widths[0] * end, widths[1] * end, widths[2] * end,
                                  widths[3] * end));
    auto same = [](const std::optional<D2D1_COLOR_F>& a, const std::optional<D2D1_COLOR_F>& b) {
      if (!a || !b) return !a && !b;
      return a->r == b->r && a->g == b->g && a->b == b->b && a->a == b->a;
    };
    if (same(band_colors[0], band_colors[1]) && same(band_colors[0], band_colors[2]) &&
        same(band_colors[0], band_colors[3])) {
      if (band_colors[0]) {
        brush->SetColor(*band_colors[0]);
        context->FillGeometry(ring.get(), brush.get());
      }
      continue;
    }
    for (int side = 0; side < 4; ++side) {
      if (!band_colors[side]) continue;
      auto region = SideRegion(factory.get(), side, widths, w, h);
      brush->SetColor(*band_colors[side]);
      WithMask(context, region.get(), [&] { context->FillGeometry(ring.get(), brush.get()); });
    }
  }
  bool patterned = std::any_of(std::begin(paint.styles), std::end(paint.styles),
                               [](uint32_t s) { return s == kDashed || s == kDotted; });
  if (!patterned) return;
  auto ring = Ring(factory.get(), outer, outer.Shrink(widths[0], widths[1], widths[2], widths[3]));
  WithMask(context, ring.get(), [&] {
    for (int side = 0; side < 4; ++side) {
      brush->SetColor(colors[side]);
      PaintPatternedSide(context, brush.get(), side, paint.styles[side], widths, w, h);
    }
  });
}

}  // namespace

D2D1_COLOR_F ToD2DColor(const WhiskerMobileColor& color) {
  if (color.kind == 1) {
    return {color.red / 255.0f, color.green / 255.0f, color.blue / 255.0f, color.alpha};
  }
  return NamedColor({color.name.ptr ? color.name.ptr : "", color.name.len});
}

WhiskerMobileColor OwnedColor(const WhiskerMobileColor& color) {
  if (color.kind == 1) return {1, color.red, color.green, color.blue, 0, color.alpha, {}};
  auto resolved = ToD2DColor(color);
  auto byte = [](float v) { return static_cast<uint8_t>(std::lround(v * 255.0f)); };
  return {1, byte(resolved.r), byte(resolved.g), byte(resolved.b), 0, resolved.a, {}};
}

WhiskerMobileBoxPaint OwnedPaint(const WhiskerMobileBoxPaint& paint) {
  auto owned = paint;
  owned.background = OwnedColor(paint.background);
  for (auto& color : owned.colors) color = OwnedColor(color);
  return owned;
}

RoundedRect RoundedRect::From(const WhiskerMobileBoxPaint* paint, float width, float height) {
  RoundedRect rounded;
  rounded.rect = D2D1::RectF(0, 0, width, height);
  if (!paint) return rounded;
  for (int corner = 0; corner < 4; ++corner) {
    float rx = std::max(0.0f, Resolve(paint->radii_horizontal[corner], width));
    float ry = std::max(0.0f, Resolve(paint->radii_vertical[corner], height));
    rounded.radii[corner] = rx == 0 || ry == 0 ? D2D1_SIZE_F{0, 0} : D2D1_SIZE_F{rx, ry};
  }
  // CSS scales every radius by the same factor until no two adjacent corners overlap.
  auto const& r = rounded.radii;
  float factor = 1;
  auto limit = [&factor](float side, float sum) {
    if (sum > side && sum > 0) factor = std::min(factor, side / sum);
  };
  limit(width, r[0].width + r[1].width);
  limit(width, r[3].width + r[2].width);
  limit(height, r[0].height + r[3].height);
  limit(height, r[1].height + r[2].height);
  for (auto& radius : rounded.radii) {
    radius.width *= factor;
    radius.height *= factor;
  }
  return rounded;
}

bool RoundedRect::IsRectilinear() const {
  return std::all_of(radii.begin(), radii.end(),
                     [](D2D1_SIZE_F r) { return r.width == 0 || r.height == 0; });
}

RoundedRect RoundedRect::Shrink(float top, float right, float bottom, float left) const {
  RoundedRect inner;
  inner.rect = D2D1::RectF(rect.left + left, rect.top + top, std::max(rect.left + left, rect.right - right),
                           std::max(rect.top + top, rect.bottom - bottom));
  const float dx[] = {left, right, right, left};
  const float dy[] = {top, top, bottom, bottom};
  for (int corner = 0; corner < 4; ++corner) {
    float rx = std::max(0.0f, radii[corner].width - dx[corner]);
    float ry = std::max(0.0f, radii[corner].height - dy[corner]);
    inner.radii[corner] = rx == 0 || ry == 0 ? D2D1_SIZE_F{0, 0} : D2D1_SIZE_F{rx, ry};
  }
  return inner;
}

winrt::com_ptr<ID2D1PathGeometry> RoundedRect::Geometry(ID2D1Factory* factory) const {
  winrt::com_ptr<ID2D1PathGeometry> geometry;
  winrt::check_hresult(factory->CreatePathGeometry(geometry.put()));
  winrt::com_ptr<ID2D1GeometrySink> sink;
  winrt::check_hresult(geometry->Open(sink.put()));
  auto [l, t, r, b] = rect;
  auto arc = [&](D2D1_POINT_2F to, D2D1_SIZE_F radius) {
    if (radius.width > 0 && radius.height > 0) {
      sink->AddArc(D2D1::ArcSegment(to, radius, 0, D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                    D2D1_ARC_SIZE_SMALL));
    } else {
      sink->AddLine(to);
    }
  };
  sink->BeginFigure({l + radii[0].width, t}, D2D1_FIGURE_BEGIN_FILLED);
  sink->AddLine({r - radii[1].width, t});
  arc({r, t + radii[1].height}, radii[1]);
  sink->AddLine({r, b - radii[2].height});
  arc({r - radii[2].width, b}, radii[2]);
  sink->AddLine({l + radii[3].width, b});
  arc({l, b - radii[3].height}, radii[3]);
  sink->AddLine({l, t + radii[0].height});
  arc({l + radii[0].width, t}, radii[0]);
  sink->EndFigure(D2D1_FIGURE_END_CLOSED);
  winrt::check_hresult(sink->Close());
  return geometry;
}

std::array<float, 4> BorderWidths(const WhiskerMobileBoxPaint& paint, float width) {
  std::array<float, 4> widths{};
  for (int side = 0; side < 4; ++side) {
    bool drawn = paint.styles[side] != kNone && paint.styles[side] != kHidden;
    widths[side] = drawn ? std::max(0.0f, Resolve(paint.widths[side], width)) : 0;
  }
  return widths;
}

bool NeedsBoxSurface(const WhiskerMobileBoxPaint& paint, float width, float height) {
  auto widths = BorderWidths(paint, width);
  bool bordered = std::any_of(widths.begin(), widths.end(), [](float v) { return v > 0; });
  return bordered || !RoundedRect::From(&paint, width, height).IsRectilinear();
}

void PaintBox(ID2D1DeviceContext* context, const WhiskerMobileBoxPaint& paint, float width,
              float height) {
  winrt::com_ptr<ID2D1Factory> factory;
  context->GetFactory(factory.put());
  auto background = ToD2DColor(paint.background);
  if (background.a > 0) {
    winrt::com_ptr<ID2D1SolidColorBrush> brush;
    context->CreateSolidColorBrush(background, brush.put());
    context->FillGeometry(RoundedRect::From(&paint, width, height).Geometry(factory.get()).get(),
                          brush.get());
  }
  PaintBorder(context, paint, width, height);
}

}  // namespace whisker
