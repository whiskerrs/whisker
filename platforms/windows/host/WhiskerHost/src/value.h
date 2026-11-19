#pragma once

#include <string_view>

#include "whisker_mobile.h"

namespace whisker {

inline std::string_view View(const WhiskerStringRef& value) {
  return {value.ptr ? value.ptr : "", value.len};
}

inline const WhiskerValueRaw* MapEntry(const WhiskerValueRaw* value, std::string_view key) {
  if (!value || value->type != WHISKER_VALUE_MAP) return nullptr;
  for (size_t i = 0; i < value->v.map.count; ++i) {
    if (View(value->v.map.entries[i].key) == key) return &value->v.map.entries[i].value;
  }
  return nullptr;
}

inline float Number(const WhiskerValueRaw* value, float fallback = 0) {
  if (!value) return fallback;
  if (value->type == WHISKER_VALUE_FLOAT) return static_cast<float>(value->v.f);
  if (value->type == WHISKER_VALUE_INT) return static_cast<float>(value->v.i);
  return fallback;
}

inline bool Flag(const WhiskerValueRaw* value) {
  return value && value->type == WHISKER_VALUE_BOOL && value->v.b;
}

}  // namespace whisker
