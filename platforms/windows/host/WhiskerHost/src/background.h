#pragma once

#include <windows.h>

#include <optional>
#include <vector>

#include <d2d1_1.h>

#include "paint.h"
#include "whisker_mobile.h"

namespace whisker {

struct GradientStop {
  D2D1_COLOR_F color;
  WhiskerMobileLengthPercentage position;
};

// A gradient background layer copied out of the frame that borrowed its payload.
struct BackgroundLayer {
  uint32_t kind = 0;
  float degrees = 0;
  WhiskerMobileLengthPercentage center_x{};
  WhiskerMobileLengthPercentage center_y{};
  WhiskerMobileLengthPercentage radius_x{};
  WhiskerMobileLengthPercentage radius_y{};
  std::vector<GradientStop> stops;
  WhiskerMobileBackgroundLayer geometry{};

  static std::optional<BackgroundLayer> From(const WhiskerMobileBackgroundLayer& raw);
};

// The border, padding, and content boxes of one node in its own border-box space.
struct BoxGeometry {
  RoundedRect border;
  RoundedRect padding;
  RoundedRect content;
};

// `layers` are ordered front to back, as CSS lists them.
void PaintBackgroundLayers(ID2D1DeviceContext* context, const std::vector<BackgroundLayer>& layers,
                           const BoxGeometry& boxes);

}  // namespace whisker
