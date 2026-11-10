#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "background.h"
#include "clip_path.h"
#include "graphics.h"
#include "resources.h"
#include "shadow.h"
#include "text.h"
#include "whisker_mobile.h"

namespace whisker {

// Mirrors the Rust scene as one Composition visual subtree per node.
class HostScene {
 public:
  HostScene(winrt::Microsoft::UI::Composition::ContainerVisual root, const TextEngine& text,
            const ResourceService& resources,
            winrt::Microsoft::UI::Composition::CompositionGraphicsDevice graphics);

  struct ScrollEvent {
    uint64_t node;
    float left, top, width, height, viewport_width, viewport_height;
  };

  bool Bootstrap(const WhiskerMobileBootstrap& bootstrap);
  void SetScale(float scale);
  bool Apply(const WhiskerMobileFrame& frame, WhiskerMobileApplyResponse& response);

  // Points are logical pixels relative to the surface.
  int32_t CursorAt(float x, float y) const;
  // Scrolls the innermost enabled ScrollView under the point that can still move.
  bool ScrollAt(float x, float y, float dx, float dy);
  std::vector<ScrollEvent> TakeScrollEvents();
  std::vector<std::pair<uint64_t, std::array<float, 2>>> TakeScrollOffsets();

 private:
  enum class Kind { View, ScrollView };

  struct Element {
    Kind kind = Kind::View;
    std::unordered_map<uint32_t, std::string> properties;
    uint64_t scroll_event_mask = 0;
  };

  struct Scroll {
    bool horizontal = false;
    bool enabled = true;
    std::array<float, 2> offset{};
  };

  struct Node {
    winrt::Microsoft::UI::Composition::ContainerVisual visual{nullptr};
    winrt::Microsoft::UI::Composition::ContainerVisual group{nullptr};
    winrt::Microsoft::UI::Composition::CompositionVisualSurface group_surface{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual group_sprite{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual shadow{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual background{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual text_sprite{nullptr};
    winrt::Microsoft::UI::Composition::ContainerVisual children_visual{nullptr};
    winrt::Microsoft::UI::Composition::ContainerVisual scroll_content{nullptr};
    std::vector<uint64_t> children;
    uint64_t parent = 0;
    int32_t z_order = 0;
    uint32_t element_type = 0;
    std::optional<Scroll> scroll;
    uint64_t event_mask = 0;
    int32_t cursor = 0;
    int32_t hit_test = 0;
    std::array<float, 6> transform{1, 0, 0, 1, 0, 0};
    WhiskerMobileLayoutGeometry geometry{};
    std::optional<WhiskerMobileBoxPaint> paint;
    std::vector<BackgroundLayer> layers;
    uint32_t image_rendering = 0;
    std::vector<BoxShadow> shadows;
    std::optional<ClipPath> clip_path;
    bool box_dirty = false;
    bool clip_horizontal = false;
    bool clip_vertical = false;
    bool visible = true;
    bool has_text = false;
    bool text_dirty = false;
    TextStyle text;
  };

  void Clear();
  bool ApplyOperation(const WhiskerMobileOperation& operation);
  void Create(uint64_t id);
  void Delete(uint64_t id);
  void Insert(uint64_t parent, uint64_t child, uint32_t index);
  void Detach(uint64_t child);
  void Layout(Node& node, const WhiskerMobileLayoutGeometry& geometry);
  void Flatten(Node& node);
  void UpdateClip(Node& node);
  void UpdateClipPath(Node& node);
  void SyncChildren(uint64_t parent);
  void DrawBox(Node& node);
  void DrawShadow(Node& node);
  void DrawText(Node& node);
  void SetProperty(Node& node, uint32_t property, const WhiskerValueRaw* value);
  void Invoke(uint64_t id, uint32_t command, const WhiskerValueRaw* arguments);
  bool ScrollTo(uint64_t id, std::array<float, 2> offset);
  std::array<float, 2> ContentExtent(const Node& node) const;
  std::array<float, 2> MaxScrollOffset(const Node& node) const;
  std::vector<uint64_t> PaintOrder(const std::vector<uint64_t>& ids) const;
  std::optional<uint64_t> HitTest(const std::vector<uint64_t>& ids, float x, float y) const;
  const Element* ElementOf(const Node& node) const;

  winrt::Microsoft::UI::Composition::Compositor compositor_;
  winrt::Microsoft::UI::Composition::ContainerVisual root_;
  winrt::Microsoft::UI::Composition::CompositionGraphicsDevice graphics_;
  const TextEngine& text_;
  const ResourceService& resources_;
  std::unordered_map<uint64_t, Node> nodes_;
  std::vector<uint64_t> roots_;
  std::unordered_set<uint64_t> dirty_children_;
  std::unordered_map<uint32_t, Element> elements_;
  std::vector<ScrollEvent> scroll_events_;
  std::unordered_map<uint64_t, std::array<float, 2>> scroll_offsets_;
  uint64_t revision_ = 0;
  uint32_t scene_epoch_ = 0;
  float scale_ = 1;
};

}  // namespace whisker
