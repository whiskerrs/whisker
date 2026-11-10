#pragma once

#include <cstdint>
#include <optional>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "background.h"
#include "clip_path.h"
#include "graphics.h"
#include "shadow.h"
#include "text.h"
#include "whisker_mobile.h"

namespace whisker {

// Mirrors the Rust scene as one Composition visual subtree per node.
class HostScene {
 public:
  HostScene(winrt::Microsoft::UI::Composition::ContainerVisual root, const TextEngine& text,
            winrt::Microsoft::UI::Composition::CompositionGraphicsDevice graphics);

  void SetScale(float scale);
  bool Apply(const WhiskerMobileFrame& frame, WhiskerMobileApplyResponse& response);

 private:
  struct Node {
    winrt::Microsoft::UI::Composition::ContainerVisual visual{nullptr};
    winrt::Microsoft::UI::Composition::ContainerVisual group{nullptr};
    winrt::Microsoft::UI::Composition::CompositionVisualSurface group_surface{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual group_sprite{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual shadow{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual background{nullptr};
    winrt::Microsoft::UI::Composition::SpriteVisual text_sprite{nullptr};
    winrt::Microsoft::UI::Composition::ContainerVisual children_visual{nullptr};
    std::vector<uint64_t> children;
    uint64_t parent = 0;
    int32_t z_order = 0;
    WhiskerMobileLayoutGeometry geometry{};
    std::optional<WhiskerMobileBoxPaint> paint;
    std::vector<BackgroundLayer> layers;
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

  winrt::Microsoft::UI::Composition::Compositor compositor_;
  winrt::Microsoft::UI::Composition::ContainerVisual root_;
  winrt::Microsoft::UI::Composition::CompositionGraphicsDevice graphics_;
  const TextEngine& text_;
  std::unordered_map<uint64_t, Node> nodes_;
  std::vector<uint64_t> roots_;
  std::unordered_set<uint64_t> dirty_children_;
  uint64_t revision_ = 0;
  uint32_t scene_epoch_ = 0;
  float scale_ = 1;
};

}  // namespace whisker
