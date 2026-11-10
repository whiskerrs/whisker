#pragma once

#include <windows.h>

#include <array>
#include <d2d1_1.h>
#include <winrt/base.h>

#include "whisker_mobile.h"

namespace whisker {

D2D1_COLOR_F ToD2DColor(const WhiskerMobileColor& color);

// Copies that resolve named colors, so they outlive the frame that borrowed the names.
WhiskerMobileColor OwnedColor(const WhiskerMobileColor& color);
WhiskerMobileBoxPaint OwnedPaint(const WhiskerMobileBoxPaint& paint);

// A box with elliptical corners in top-left, top-right, bottom-right, bottom-left order.
struct RoundedRect {
  D2D1_RECT_F rect{};
  std::array<D2D1_SIZE_F, 4> radii{};

  static RoundedRect From(const WhiskerMobileBoxPaint* paint, float width, float height);
  bool IsRectilinear() const;
  RoundedRect Shrink(float top, float right, float bottom, float left) const;
  winrt::com_ptr<ID2D1PathGeometry> Geometry(ID2D1Factory* factory) const;
};

// Border widths in top, right, bottom, left order; `none` and `hidden` edges are zero.
std::array<float, 4> BorderWidths(const WhiskerMobileBoxPaint& paint, float width);

// True when the paint needs more than a flat rectangle of its background color.
bool NeedsBoxSurface(const WhiskerMobileBoxPaint& paint, float width, float height);

void PaintBox(ID2D1DeviceContext* context, const WhiskerMobileBoxPaint& paint, float width,
              float height);

}  // namespace whisker
