// `whisker-surface` ModuleDefinition (Windows): a DirectComposition surface the renderer presents to.

#include <whisker/module.h>

#include <dcomp.h>

#include <charconv>
#include <cmath>
#include <cstdint>
#include <string>
#include <string_view>

#pragma comment(lib, "dcomp.lib")

extern "C" {
void whisker_module_surface_attach(uint64_t id, uint32_t kind, void* handle, uint32_t width,
                                   uint32_t height, float scale);
void whisker_module_surface_resize(uint64_t id, uint32_t width, uint32_t height, float scale);
bool whisker_module_surface_frame(uint64_t id, double timestamp_seconds);
void whisker_module_surface_detach(uint64_t id);
}

namespace {

constexpr uint32_t kDCompSurfaceHandle = 3;

uint64_t ParseId(std::string_view value) {
  uint64_t id = 0;
  auto end = value.find(':');
  auto digits = value.substr(0, end);
  std::from_chars(digits.data(), digits.data() + digits.size(), id);
  return id;
}

// Calls Rust only from `OnFrame`, never from a property setter inside Whisker's commit.
class SurfaceView final : public whisker::ModuleView {
 public:
  using ModuleView::ModuleView;

  ~SurfaceView() override {
    Detach();
    if (handle_) CloseHandle(handle_);
  }

  void Draw(ID2D1DeviceContext*, float, float) override {}

  void SetSurface(std::string_view value) {
    uint64_t id = ParseId(value);
    if (id != surface_) {
      Detach();
      surface_ = id;
    }
    RequestFrame();
  }

  void OnFrame(double timestamp, float width, float height, float scale) override {
    if (surface_ == 0) return;
    auto pixel_width = static_cast<uint32_t>(std::lround(width * scale));
    auto pixel_height = static_cast<uint32_t>(std::lround(height * scale));
    if (pixel_width == 0 || pixel_height == 0) {
      RequestFrame();
      return;
    }
    if (!handle_) {
      if (FAILED(DCompositionCreateSurfaceHandle(COMPOSITIONOBJECT_ALL_ACCESS, nullptr, &handle_))) {
        handle_ = nullptr;
        return;
      }
      SetCompositionSurfaceHandle(handle_);
    }
    if (attached_ != surface_) {
      whisker_module_surface_attach(surface_, kDCompSurfaceHandle, handle_, pixel_width,
                                    pixel_height, scale);
      attached_ = surface_;
    } else if (pixel_width != width_ || pixel_height != height_ || scale != scale_) {
      whisker_module_surface_resize(surface_, pixel_width, pixel_height, scale);
    }
    width_ = pixel_width;
    height_ = pixel_height;
    scale_ = scale;
    if (whisker_module_surface_frame(surface_, timestamp)) RequestFrame();
  }

 private:
  void Detach() {
    if (attached_ != 0) whisker_module_surface_detach(attached_);
    attached_ = 0;
  }

  HANDLE handle_ = nullptr;
  uint64_t surface_ = 0;
  uint64_t attached_ = 0;
  uint32_t width_ = 0;
  uint32_t height_ = 0;
  float scale_ = 0;
};

class SurfaceModule final : public whisker::Module {
 public:
  whisker::ModuleDefinition Definition() override {
    return whisker::ModuleDefinition().Name("Surface").View<SurfaceView>(
        "whisker-surface:Surface", [](auto& view) {
          view.Prop("surface", [](SurfaceView& surface, const whisker::Value& value) {
            surface.SetSurface(value.AsString().value_or(""));
          });
          view.Prop("wheel", [](SurfaceView&, const whisker::Value&) {});
          view.Events({"wheel"});
        });
  }
};

}  // namespace

WHISKER_MODULE(SurfaceModule)
