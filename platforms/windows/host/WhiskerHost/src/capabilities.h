#pragma once

#include "whisker_mobile.h"

namespace whisker {

inline WhiskerMobileHostCapabilities HostCapabilities() {
  WhiskerMobileHostCapabilities capabilities{};
  capabilities.abi_major = WHISKER_MOBILE_ABI_MAJOR;
  capabilities.abi_minor = WHISKER_MOBILE_ABI_MINOR;
  capabilities.protocol_major = WHISKER_FRAME_PROTOCOL_MAJOR;
  capabilities.protocol_minor = WHISKER_FRAME_PROTOCOL_MINOR;
  capabilities.native = WHISKER_CAPABILITY_ELLIPTICAL_BORDER_RADIUS |
                        WHISKER_CAPABILITY_VISUAL_EFFECTS | WHISKER_CAPABILITY_TEXT_EFFECTS |
                        WHISKER_CAPABILITY_TEXT_TYPOGRAPHY | WHISKER_CAPABILITY_RICH_TEXT |
                        WHISKER_CAPABILITY_CURSOR | WHISKER_CAPABILITY_RESOURCE_LIFECYCLE |
                        WHISKER_CAPABILITY_LINEAR_GRADIENTS |
                        WHISKER_CAPABILITY_RADIAL_GRADIENTS | WHISKER_CAPABILITY_CONIC_GRADIENTS |
                        WHISKER_CAPABILITY_BACKGROUND_GEOMETRY |
                        WHISKER_CAPABILITY_BACKGROUND_LAYER_STACKING |
                        WHISKER_CAPABILITY_BACKGROUND_IMAGE_RESOURCES;
  capabilities.emulated = WHISKER_CAPABILITY_BACKDROP_BLUR;
  return capabilities;
}

}  // namespace whisker
