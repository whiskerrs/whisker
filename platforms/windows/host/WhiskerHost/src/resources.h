#pragma once

#include <windows.h>

#include <wincodec.h>

#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>
#include <winrt/base.h>

#include "whisker_mobile.h"

namespace whisker {

struct Raster {
  winrt::com_ptr<IWICBitmapSource> pixels;
  uint32_t width = 0;
  uint32_t height = 0;
};

enum class ResourceState { Loading, Ready, Failed, Released };

// Acquires and decodes raster resources outside frame application, keyed by id and generation.
class ResourceService {
 public:
  using EventSink = std::function<void(const WhiskerMobileResourceEvent&)>;

  ResourceService();
  ~ResourceService();

  void SetEventSink(EventSink sink) { sink_ = std::move(sink); }
  bool Apply(const WhiskerMobileResourceCommand& command);
  // Installs premultiplied-free RGBA pixels directly, as tests register fixtures.
  bool Register(uint64_t id, uint32_t width, uint32_t height, const uint8_t* rgba);
  std::optional<Raster> Find(uint64_t id) const;
  std::optional<ResourceState> State(uint64_t id, uint64_t generation) const;

 private:
  bool Load(uint64_t id, uint64_t generation, const WhiskerMobileResourceCommand& command);
  bool Release(uint64_t id, uint64_t generation);
  void Decode(uint64_t id, uint64_t generation, const std::vector<uint8_t>& bytes);
  void Fail(uint64_t id, uint64_t generation, uint32_t code);
  void Emit(uint64_t id, uint64_t generation, ResourceState state, uint32_t failure = 0);

  winrt::com_ptr<IWICImagingFactory> wic_;
  EventSink sink_;
  std::unordered_map<uint64_t, uint64_t> latest_;
  std::unordered_map<uint64_t, uint64_t> current_;
  std::unordered_map<uint64_t, Raster> rasters_;
  std::map<std::pair<uint64_t, uint64_t>, ResourceState> states_;
  std::shared_ptr<ResourceService*> self_;
};

}  // namespace whisker
