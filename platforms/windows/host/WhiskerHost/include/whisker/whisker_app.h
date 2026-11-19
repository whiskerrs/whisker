#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WhiskerWindowsAppOptions {
  // UTF-8, not NUL-terminated.
  const char* title;
  uintptr_t title_len;
  uint8_t background_rgb[3];
} WhiskerWindowsAppOptions;

// Initializes the Windows App Runtime and runs one Whisker window until it closes; returns an
// HRESULT.
int32_t whisker_windows_run(const WhiskerWindowsAppOptions* options);

#ifdef __cplusplus
}
#endif
