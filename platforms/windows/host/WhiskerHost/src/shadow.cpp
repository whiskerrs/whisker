#include "shadow.h"

#include <algorithm>
#include <cmath>

#include <d2d1effects.h>

#pragma comment(lib, "dxguid.lib")

namespace whisker {

namespace {

// CSS keeps sharp corners sharp and grows small radii slower than the spread.
float SpreadRadius(float radius, float spread) {
  if (spread < 0) return std::max(0.0f, radius + spread);
  if (radius >= spread) return radius + spread;
  float ratio = radius / spread - 1;
  return radius + spread * (1 + ratio * ratio * ratio);
}

std::optional<RoundedRect> Spread(const RoundedRect& shape, float spread, float dx, float dy) {
  RoundedRect spread_shape;
  auto const& r = shape.rect;
  spread_shape.rect = D2D1::RectF(r.left - spread + dx, r.top - spread + dy,
                                  r.right + spread + dx, r.bottom + spread + dy);
  if (spread_shape.rect.right <= spread_shape.rect.left ||
      spread_shape.rect.bottom <= spread_shape.rect.top) {
    return std::nullopt;
  }
  for (int corner = 0; corner < 4; ++corner) {
    float rx = SpreadRadius(shape.radii[corner].width, spread);
    float ry = SpreadRadius(shape.radii[corner].height, spread);
    spread_shape.radii[corner] = rx == 0 || ry == 0 ? D2D1_SIZE_F{0, 0} : D2D1_SIZE_F{rx, ry};
  }
  spread_shape.FitRadii();
  return spread_shape;
}

winrt::com_ptr<ID2D1Factory> FactoryOf(ID2D1DeviceContext* context) {
  winrt::com_ptr<ID2D1Factory> factory;
  context->GetFactory(factory.put());
  return factory;
}

winrt::com_ptr<ID2D1Geometry> EvenOdd(ID2D1Factory* factory,
                                      std::initializer_list<ID2D1Geometry*> parts) {
  std::vector<ID2D1Geometry*> list(parts);
  winrt::com_ptr<ID2D1GeometryGroup> group;
  winrt::check_hresult(factory->CreateGeometryGroup(D2D1_FILL_MODE_ALTERNATE, list.data(),
                                                    static_cast<UINT32>(list.size()), group.put()));
  return group.as<ID2D1Geometry>();
}

// Records the fill with an identity transform so the blur and the final draw share one space;
// the context must have no layers or clips pushed.
winrt::com_ptr<ID2D1Image> Blurred(ID2D1DeviceContext* context, ID2D1Geometry* geometry,
                                   D2D1_COLOR_F color, float blur) {
  winrt::com_ptr<ID2D1Image> target;
  context->GetTarget(target.put());
  D2D1_MATRIX_3X2_F transform;
  context->GetTransform(&transform);
  winrt::com_ptr<ID2D1CommandList> list;
  winrt::check_hresult(context->CreateCommandList(list.put()));
  context->SetTarget(list.get());
  context->SetTransform(D2D1::Matrix3x2F::Identity());
  winrt::com_ptr<ID2D1SolidColorBrush> brush;
  context->CreateSolidColorBrush(color, brush.put());
  context->FillGeometry(geometry, brush.get());
  context->SetTarget(target.get());
  context->SetTransform(transform);
  winrt::check_hresult(list->Close());
  if (blur <= 0) return list.as<ID2D1Image>();
  winrt::com_ptr<ID2D1Effect> effect;
  winrt::check_hresult(context->CreateEffect(CLSID_D2D1GaussianBlur, effect.put()));
  effect->SetInput(0, list.get());
  effect->SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, blur / 2);
  effect->SetValue(D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION, D2D1_GAUSSIANBLUR_OPTIMIZATION_QUALITY);
  winrt::com_ptr<ID2D1Image> output;
  effect->GetOutput(output.put());
  return output;
}

void DrawMasked(ID2D1DeviceContext* context, ID2D1Image* image, ID2D1Geometry* mask) {
  context->PushLayer(D2D1::LayerParameters1(D2D1::InfiniteRect(), mask), nullptr);
  context->DrawImage(image);
  context->PopLayer();
}

}  // namespace

std::optional<BoxShadow> BoxShadow::From(const WhiskerMobileBoxShadow& raw) {
  if (!std::isfinite(raw.offset_x) || !std::isfinite(raw.offset_y) ||
      !std::isfinite(raw.blur_radius) || raw.blur_radius < 0 ||
      !std::isfinite(raw.spread_radius) || raw.inset > 1) {
    return std::nullopt;
  }
  return BoxShadow{ToD2DColor(raw.color), raw.offset_x, raw.offset_y, raw.blur_radius,
                   raw.spread_radius,     raw.inset == 1};
}

float OuterShadowExtent(const std::vector<BoxShadow>& shadows) {
  float extent = 0;
  for (auto const& shadow : shadows) {
    if (shadow.inset) continue;
    extent = std::max(extent, std::max(std::abs(shadow.offset_x), std::abs(shadow.offset_y)) +
                                  std::max(0.0f, shadow.spread) + shadow.blur * 1.5f);
  }
  return std::ceil(extent);
}

void PaintOuterShadows(ID2D1DeviceContext* context, const std::vector<BoxShadow>& shadows,
                       const RoundedRect& border) {
  auto factory = FactoryOf(context);
  auto border_geometry = border.Geometry(factory.get());
  float extent = OuterShadowExtent(shadows);
  winrt::com_ptr<ID2D1RectangleGeometry> outside;
  auto const& r = border.rect;
  winrt::check_hresult(factory->CreateRectangleGeometry(
      D2D1::RectF(r.left - extent, r.top - extent, r.right + extent, r.bottom + extent),
      outside.put()));
  auto mask = EvenOdd(factory.get(), {outside.get(), border_geometry.get()});
  for (auto it = shadows.rbegin(); it != shadows.rend(); ++it) {
    if (it->inset || it->color.a <= 0) continue;
    auto shape = Spread(border, it->spread, it->offset_x, it->offset_y);
    if (!shape) continue;
    auto image = Blurred(context, shape->Geometry(factory.get()).get(), it->color, it->blur);
    DrawMasked(context, image.get(), mask.get());
  }
}

void PaintInsetShadows(ID2D1DeviceContext* context, const std::vector<BoxShadow>& shadows,
                       const RoundedRect& padding) {
  auto const& r = padding.rect;
  if (r.right <= r.left || r.bottom <= r.top) return;
  auto factory = FactoryOf(context);
  auto padding_geometry = padding.Geometry(factory.get());
  for (auto it = shadows.rbegin(); it != shadows.rend(); ++it) {
    if (!it->inset || it->color.a <= 0) continue;
    float extent = std::max(r.right - r.left, r.bottom - r.top) + std::abs(it->offset_x) +
                   std::abs(it->offset_y) + std::abs(it->spread) + it->blur * 2;
    winrt::com_ptr<ID2D1RectangleGeometry> exterior;
    winrt::check_hresult(factory->CreateRectangleGeometry(
        D2D1::RectF(r.left - extent, r.top - extent, r.right + extent, r.bottom + extent),
        exterior.put()));
    winrt::com_ptr<ID2D1Geometry> ring = exterior.as<ID2D1Geometry>();
    if (auto hole = Spread(padding, -it->spread, it->offset_x, it->offset_y)) {
      ring = EvenOdd(factory.get(), {exterior.get(), hole->Geometry(factory.get()).get()});
    }
    auto image = Blurred(context, ring.get(), it->color, it->blur);
    DrawMasked(context, image.get(), padding_geometry.get());
  }
}

}  // namespace whisker
