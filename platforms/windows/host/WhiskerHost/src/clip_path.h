#pragma once

#include <windows.h>

#include <array>
#include <optional>
#include <vector>

#include <d2d1_1.h>
#include <winrt/base.h>

#include "background.h"
#include "whisker_mobile.h"

namespace whisker {

// A clip-path basic shape copied out of the frame that borrowed its payload.
struct ClipPath {
  uint32_t reference_box = 0;
  uint32_t shape_kind = 0;
  WhiskerMobileClipInset inset{};
  std::array<WhiskerMobileLengthPercentage, 4> values{};
  uint32_t fill_rule = 0;
  std::vector<WhiskerMobilePathCommand> commands;

  static std::optional<ClipPath> From(const WhiskerMobileClipPath& raw);
  winrt::com_ptr<ID2D1Geometry> Geometry(ID2D1Factory* factory, const BoxGeometry& boxes) const;
};

}  // namespace whisker
