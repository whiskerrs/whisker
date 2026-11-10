#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "whisker_mobile.h"

#ifdef __cplusplus
extern "C" {
#endif

// A detached Composition scene that conformance fixtures present into and capture from.
typedef struct WhiskerHostConformance WhiskerHostConformance;

// Every call must come from the thread of the first create, which needs an interactive logon.
WhiskerHostConformance* whisker_host_conformance_create(void);
void whisker_host_conformance_destroy(WhiskerHostConformance* host);

WhiskerMobileHostCapabilities whisker_host_conformance_capabilities(void);

bool whisker_host_conformance_present(void* host, const WhiskerMobileFrame* frame,
                                      WhiskerMobileApplyResponse* response);
bool whisker_host_conformance_measure(void* host, const WhiskerMobileMeasureRequest* requests,
                                      size_t count, WhiskerMobileMeasureResponse* responses);

bool whisker_host_conformance_resource(WhiskerHostConformance* host,
                                       const WhiskerMobileResourceCommand* command);
// `rgba` holds `width * height` straight-alpha pixels.
bool whisker_host_conformance_register_raster(WhiskerHostConformance* host, uint64_t id,
                                              uint32_t width, uint32_t height,
                                              const uint8_t* rgba);
// Returns 0 for an unknown generation, then loading, ready, failed, released from 1.
uint32_t whisker_host_conformance_resource_state(WhiskerHostConformance* host, uint64_t id,
                                                 uint64_t generation, uint32_t* width,
                                                 uint32_t* height);

// Writes `width * height` straight-alpha RGBA pixels rendered at one pixel per logical pixel.
bool whisker_host_conformance_capture(WhiskerHostConformance* host, uint32_t width,
                                      uint32_t height, uint8_t* rgba);

#ifdef __cplusplus
}
#endif
