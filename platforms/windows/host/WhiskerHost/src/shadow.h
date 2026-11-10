#pragma once

#include <windows.h>

#include <optional>
#include <vector>

#include <d2d1_1.h>

#include "paint.h"
#include "whisker_mobile.h"

namespace whisker {

struct BoxShadow {
  D2D1_COLOR_F color{};
  float offset_x = 0;
  float offset_y = 0;
  float blur = 0;
  float spread = 0;
  bool inset = false;

  static std::optional<BoxShadow> From(const WhiskerMobileBoxShadow& raw);
};

// How far outer shadows reach past the border box on every side.
float OuterShadowExtent(const std::vector<BoxShadow>& shadows);

// `shadows` are ordered front to back; only the outer ones paint, outside `border`.
void PaintOuterShadows(ID2D1DeviceContext* context, const std::vector<BoxShadow>& shadows,
                       const RoundedRect& border);

// `shadows` are ordered front to back; only the inset ones paint, inside `padding`.
void PaintInsetShadows(ID2D1DeviceContext* context, const std::vector<BoxShadow>& shadows,
                       const RoundedRect& padding);

}  // namespace whisker
