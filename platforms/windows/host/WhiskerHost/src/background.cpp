#include "background.h"

#include <algorithm>
#include <cmath>
#include <numbers>

namespace whisker {

namespace {

float Resolve(const WhiskerMobileLengthPercentage& value, float basis) {
  return value.length + value.fraction * basis;
}

float Width(const D2D1_RECT_F& rect) { return rect.right - rect.left; }
float Height(const D2D1_RECT_F& rect) { return rect.bottom - rect.top; }

struct ResolvedStop {
  D2D1_COLOR_F color;
  float position;
};

D2D1_COLOR_F Mix(const D2D1_COLOR_F& a, const D2D1_COLOR_F& b, float t) {
  return {a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t,
          a.a + (b.a - a.a) * t};
}

D2D1_COLOR_F ColorAt(const std::vector<ResolvedStop>& stops, float position) {
  if (position <= stops.front().position) return stops.front().color;
  for (size_t i = 1; i < stops.size(); ++i) {
    if (position > stops[i].position) continue;
    float distance = stops[i].position - stops[i - 1].position;
    if (distance <= 0) return stops[i].color;
    return Mix(stops[i - 1].color, stops[i].color, (position - stops[i - 1].position) / distance);
  }
  return stops.back().color;
}

// Positions along a gradient line of `length`, made non-decreasing as CSS requires.
std::vector<ResolvedStop> ResolveStops(const std::vector<GradientStop>& stops, float length) {
  std::vector<ResolvedStop> resolved;
  float previous = -INFINITY;
  for (auto const& stop : stops) {
    float position = length > 0 ? stop.position.length / length + stop.position.fraction
                                : stop.position.fraction;
    previous = std::max(previous, position);
    resolved.push_back({stop.color, previous});
  }
  return resolved;
}

winrt::com_ptr<ID2D1GradientStopCollection1> StopCollection(
    ID2D1DeviceContext* context, const std::vector<ResolvedStop>& stops, float start, float end) {
  std::vector<D2D1_GRADIENT_STOP> raw;
  for (auto const& stop : stops) raw.push_back({(stop.position - start) / (end - start), stop.color});
  winrt::com_ptr<ID2D1GradientStopCollection1> collection;
  winrt::check_hresult(context->CreateGradientStopCollection(
      raw.data(), static_cast<UINT32>(raw.size()), D2D1_COLOR_SPACE_SRGB, D2D1_COLOR_SPACE_SRGB,
      D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_EXTEND_MODE_CLAMP,
      D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED, collection.put()));
  return collection;
}

void PaintLinear(ID2D1DeviceContext* context, const BackgroundLayer& layer, D2D1_RECT_F tile) {
  float radians = layer.degrees * std::numbers::pi_v<float> / 180;
  float dx = std::sin(radians);
  float dy = -std::cos(radians);
  float length = std::abs(Width(tile) * dx) + std::abs(Height(tile) * dy);
  if (length <= 0) return;
  auto stops = ResolveStops(layer.stops, length);
  float start = std::min(0.0f, stops.front().position);
  float end = std::max(1.0f, stops.back().position);
  if (end <= start) return;
  D2D1_POINT_2F center{(tile.left + tile.right) / 2, (tile.top + tile.bottom) / 2};
  auto point = [&](float t) {
    return D2D1_POINT_2F{center.x + dx * length * (t - 0.5f), center.y + dy * length * (t - 0.5f)};
  };
  winrt::com_ptr<ID2D1LinearGradientBrush> brush;
  winrt::check_hresult(context->CreateLinearGradientBrush(
      D2D1::LinearGradientBrushProperties(point(start), point(end)),
      StopCollection(context, stops, start, end).get(), brush.put()));
  context->FillRectangle(tile, brush.get());
}

void PaintRadial(ID2D1DeviceContext* context, const BackgroundLayer& layer, D2D1_RECT_F tile) {
  float rx = Resolve(layer.radius_x, Width(tile));
  float ry = Resolve(layer.radius_y, Height(tile));
  if (rx <= 0 || ry <= 0) return;
  auto stops = ResolveStops(layer.stops, rx);
  if (stops.front().position < 0) {
    auto first = ColorAt(stops, 0);
    std::erase_if(stops, [](const ResolvedStop& stop) { return stop.position < 0; });
    stops.insert(stops.begin(), {first, 0});
  }
  float end = std::max(1.0f, stops.back().position);
  D2D1_POINT_2F center{tile.left + Resolve(layer.center_x, Width(tile)),
                       tile.top + Resolve(layer.center_y, Height(tile))};
  winrt::com_ptr<ID2D1RadialGradientBrush> brush;
  winrt::check_hresult(context->CreateRadialGradientBrush(
      D2D1::RadialGradientBrushProperties(center, {0, 0}, rx * end, ry * end),
      StopCollection(context, stops, 0, end).get(), brush.put()));
  context->FillRectangle(tile, brush.get());
}

// Direct2D has no conic brush, so the tile is rasterized per device pixel.
void PaintConic(ID2D1DeviceContext* context, const BackgroundLayer& layer, D2D1_RECT_F tile) {
  std::vector<ResolvedStop> ordered = ResolveStops(layer.stops, 0);
  std::vector<ResolvedStop> stops{{ColorAt(ordered, 0), 0}};
  for (auto const& stop : ordered) {
    if (stop.position >= 0 && stop.position <= 1) stops.push_back(stop);
  }
  stops.push_back({ColorAt(ordered, 1), 1});
  float dpi_x = 96;
  float dpi_y = 96;
  context->GetDpi(&dpi_x, &dpi_y);
  float scale = dpi_x / 96;
  UINT32 width = std::max(1u, static_cast<UINT32>(std::ceil(Width(tile) * scale)));
  UINT32 height = std::max(1u, static_cast<UINT32>(std::ceil(Height(tile) * scale)));
  float cx = Resolve(layer.center_x, Width(tile)) * scale;
  float cy = Resolve(layer.center_y, Height(tile)) * scale;
  float start = layer.degrees / 360;
  std::vector<uint8_t> pixels(static_cast<size_t>(width) * height * 4);
  auto byte = [](float v) { return static_cast<uint8_t>(std::lround(std::clamp(v, 0.0f, 1.0f) * 255)); };
  for (UINT32 y = 0; y < height; ++y) {
    for (UINT32 x = 0; x < width; ++x) {
      float turn = std::atan2(x + 0.5f - cx, cy - (y + 0.5f)) / (2 * std::numbers::pi_v<float>) - start;
      turn -= std::floor(turn);
      auto color = ColorAt(stops, turn);
      float alpha = std::clamp(color.a, 0.0f, 1.0f);
      auto* pixel = &pixels[(static_cast<size_t>(y) * width + x) * 4];
      pixel[0] = byte(color.b * alpha);
      pixel[1] = byte(color.g * alpha);
      pixel[2] = byte(color.r * alpha);
      pixel[3] = byte(alpha);
    }
  }
  winrt::com_ptr<ID2D1Bitmap1> bitmap;
  winrt::check_hresult(context->CreateBitmap(
      D2D1::SizeU(width, height), pixels.data(), width * 4,
      D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_NONE,
                              D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM,
                                                D2D1_ALPHA_MODE_PREMULTIPLIED),
                              dpi_x, dpi_y),
      bitmap.put()));
  context->DrawBitmap(bitmap.get(), tile);
}

float RoundTile(float tile, float extent, uint32_t repeat) {
  if (repeat != WHISKER_BACKGROUND_ROUND || tile <= 0 || extent <= 0) return tile;
  float count = std::round(extent / tile);
  return count > 65536 ? 0 : extent / std::max(1.0f, count);
}

std::vector<float> TileOrigins(float base, float tile, float lo, float hi, float cover_lo,
                               float cover_hi, uint32_t repeat) {
  if (tile <= 0 || lo >= hi || cover_lo >= cover_hi) return {};
  switch (repeat) {
    case WHISKER_BACKGROUND_NO_REPEAT:
      return {base};
    case WHISKER_BACKGROUND_SPACE: {
      float count = std::floor((hi - lo) / tile);
      if (count > 65536) return {};
      if (count < 2) return {base};
      float gap = (hi - lo - count * tile) / (count - 1);
      std::vector<float> origins;
      for (int i = 0; i < static_cast<int>(count); ++i) origins.push_back(lo + i * (tile + gap));
      return origins;
    }
    default: {
      float first = base + std::floor((cover_lo - base) / tile) * tile;
      float count = std::ceil((cover_hi - first) / tile);
      if (!(count > 0) || count > 65536) return {};
      std::vector<float> origins;
      for (int i = 0; i < static_cast<int>(count); ++i) origins.push_back(first + i * tile);
      return origins;
    }
  }
}

D2D1_RECT_F ImageBounds(const WhiskerMobileBackgroundLayer& g, D2D1_RECT_F box) {
  float box_w = Width(box);
  float box_h = Height(box);
  float w = box_w;
  float h = box_h;
  switch (g.size_kind) {
    case WHISKER_BACKGROUND_SIZE_EXPLICIT:
      w = Resolve(g.size_width, box_w);
      h = Resolve(g.size_height, box_h);
      break;
    case WHISKER_BACKGROUND_SIZE_WIDTH: w = Resolve(g.size_width, box_w); break;
    case WHISKER_BACKGROUND_SIZE_HEIGHT: h = Resolve(g.size_height, box_h); break;
    default: break;
  }
  float tile_w = RoundTile(w, box_w, g.repeat_x);
  float tile_h = RoundTile(h, box_h, g.repeat_y);
  bool round_x = g.repeat_x == WHISKER_BACKGROUND_ROUND;
  bool round_y = g.repeat_y == WHISKER_BACKGROUND_ROUND;
  if (g.size_kind == WHISKER_BACKGROUND_SIZE_WIDTH && round_x && !round_y && w > 0) {
    tile_h = h * tile_w / w;
  } else if (g.size_kind == WHISKER_BACKGROUND_SIZE_HEIGHT && round_y && !round_x && h > 0) {
    tile_w = w * tile_h / h;
  }
  float x = box.left + g.position_x.length + g.position_x.fraction * (box_w - tile_w);
  float y = box.top + g.position_y.length + g.position_y.fraction * (box_h - tile_h);
  return {x, y, x + tile_w, y + tile_h};
}

const RoundedRect& BoxFor(uint32_t value, const BoxGeometry& boxes) {
  switch (value) {
    case WHISKER_BACKGROUND_BOX_PADDING: return boxes.padding;
    case WHISKER_BACKGROUND_BOX_CONTENT: return boxes.content;
    default: return boxes.border;
  }
}

void PaintLayer(ID2D1DeviceContext* context, ID2D1Factory* factory, const BackgroundLayer& layer,
                const BoxGeometry& boxes) {
  auto const& g = layer.geometry;
  auto positioning = BoxFor(g.origin, boxes).rect;
  auto image = ImageBounds(g, positioning);
  if (Width(image) <= 0 || Height(image) <= 0) return;
  auto const& clip = BoxFor(g.clip, boxes);
  winrt::com_ptr<ID2D1Geometry> mask;
  if (g.clip == WHISKER_BACKGROUND_BOX_BORDER_AREA) {
    auto outer = boxes.border.Geometry(factory);
    auto inner = boxes.padding.Geometry(factory);
    ID2D1Geometry* parts[] = {outer.get(), inner.get()};
    winrt::com_ptr<ID2D1GeometryGroup> ring;
    winrt::check_hresult(factory->CreateGeometryGroup(D2D1_FILL_MODE_ALTERNATE, parts, 2, ring.put()));
    mask = ring;
  } else {
    mask = clip.Geometry(factory);
  }
  float dpi_x = 96;
  float dpi_y = 96;
  context->GetDpi(&dpi_x, &dpi_y);
  // Keeps a gradient tile's antialiased leading edge from bleeding into the pixel before it.
  float inset = 0.5f / std::max(dpi_x / 96, 1.0f);
  auto cover = clip.rect;
  auto xs = TileOrigins(image.left, Width(image), positioning.left, positioning.right, cover.left,
                        cover.right, g.repeat_x);
  auto ys = TileOrigins(image.top, Height(image), positioning.top, positioning.bottom, cover.top,
                        cover.bottom, g.repeat_y);
  if (xs.empty() || ys.empty() || xs.size() > 65536 / ys.size()) return;
  context->PushLayer(D2D1::LayerParameters1(D2D1::InfiniteRect(), mask.get()), nullptr);
  for (float x : xs) {
    for (float y : ys) {
      D2D1_RECT_F tile{x, y, x + Width(image), y + Height(image)};
      auto tile_clip = tile;
      if (g.repeat_x == WHISKER_BACKGROUND_NO_REPEAT && tile.left > positioning.left) {
        tile_clip.left = std::min(tile_clip.right, tile_clip.left + inset);
      }
      if (g.repeat_y == WHISKER_BACKGROUND_NO_REPEAT && tile.top > positioning.top) {
        tile_clip.top = std::min(tile_clip.bottom, tile_clip.top + inset);
      }
      context->PushAxisAlignedClip(tile_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
      switch (layer.kind) {
        case WHISKER_BACKGROUND_LINEAR: PaintLinear(context, layer, tile); break;
        case WHISKER_BACKGROUND_RADIAL: PaintRadial(context, layer, tile); break;
        case WHISKER_BACKGROUND_CONIC: PaintConic(context, layer, tile); break;
        default: break;
      }
      context->PopAxisAlignedClip();
    }
  }
  context->PopLayer();
}

}  // namespace

std::optional<BackgroundLayer> BackgroundLayer::From(const WhiskerMobileBackgroundLayer& raw) {
  if (!raw.image.payload) return std::nullopt;
  BackgroundLayer layer;
  layer.kind = raw.image.kind;
  layer.degrees = raw.image.scalar;
  layer.geometry = raw;
  layer.geometry.image = {};
  const WhiskerMobileGradientStop* stops = nullptr;
  size_t count = 0;
  switch (raw.image.kind) {
    case WHISKER_BACKGROUND_LINEAR:
      stops = static_cast<const WhiskerMobileGradientStop*>(raw.image.payload);
      count = raw.image.payload_count;
      break;
    case WHISKER_BACKGROUND_RADIAL: {
      auto const& radial = *static_cast<const WhiskerMobileRadialGradient*>(raw.image.payload);
      layer.center_x = radial.center_x;
      layer.center_y = radial.center_y;
      layer.radius_x = radial.radius_x;
      layer.radius_y = radial.radius_y;
      stops = radial.stops;
      count = radial.stop_count;
      break;
    }
    case WHISKER_BACKGROUND_CONIC: {
      auto const& conic = *static_cast<const WhiskerMobileConicGradient*>(raw.image.payload);
      layer.center_x = conic.center_x;
      layer.center_y = conic.center_y;
      stops = conic.stops;
      count = conic.stop_count;
      break;
    }
    default:
      return std::nullopt;
  }
  if (!stops || count < 2) return std::nullopt;
  for (size_t i = 0; i < count; ++i) {
    layer.stops.push_back({ToD2DColor(stops[i].color), stops[i].position});
  }
  return layer;
}

void PaintBackgroundLayers(ID2D1DeviceContext* context, const std::vector<BackgroundLayer>& layers,
                           const BoxGeometry& boxes) {
  winrt::com_ptr<ID2D1Factory> factory;
  context->GetFactory(factory.put());
  for (auto it = layers.rbegin(); it != layers.rend(); ++it) {
    PaintLayer(context, factory.get(), *it, boxes);
  }
}

}  // namespace whisker
