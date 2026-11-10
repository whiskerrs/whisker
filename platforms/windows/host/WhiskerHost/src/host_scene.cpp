#include "host_scene.h"

#include <algorithm>
#include <cmath>

#include <winrt/Microsoft.Graphics.DirectX.h>
#include <winrt/Windows.Foundation.Numerics.h>
#include <winrt/Windows.UI.h>

namespace whisker {

namespace comp = winrt::Microsoft::UI::Composition;
namespace dx = winrt::Microsoft::Graphics::DirectX;

namespace {

constexpr float kUnclipped = 1.0e5f;
constexpr uint64_t kRoot = 0;

winrt::Windows::UI::Color ToColor(const WhiskerMobileColor& color) {
  if (color.kind != 1) return {0, 0, 0, 0};
  return {static_cast<uint8_t>(std::lround(color.alpha * 255.0f)), color.red, color.green,
          color.blue};
}

}  // namespace

HostScene::HostScene(comp::ContainerVisual root, const TextEngine& text)
    : compositor_(root.Compositor()),
      root_(std::move(root)),
      graphics_(CreateGraphicsDevice(compositor_)),
      text_(text) {}

void HostScene::SetScale(float scale) {
  if (scale == scale_) return;
  scale_ = scale;
  for (auto& [id, node] : nodes_) {
    if (node.has_text) node.text_dirty = true;
  }
}

bool HostScene::Apply(const WhiskerMobileFrame& frame, WhiskerMobileApplyResponse& response) {
  response.revision = revision_;
  if (frame.abi_major != WHISKER_MOBILE_ABI_MAJOR ||
      frame.protocol_major != WHISKER_FRAME_PROTOCOL_MAJOR ||
      (frame.operation_count != 0 && frame.operations == nullptr)) {
    response.status = WHISKER_APPLY_REJECTED;
    return true;
  }
  bool snapshot = frame.mode == WHISKER_FRAME_SNAPSHOT;
  if (!snapshot && (frame.scene_epoch != scene_epoch_ || frame.base_revision != revision_)) {
    response.status = WHISKER_APPLY_NEED_SNAPSHOT;
    return true;
  }
  if (snapshot && frame.base_revision != 0) {
    response.status = WHISKER_APPLY_REJECTED;
    return true;
  }
  if (snapshot) Clear();
  for (size_t i = 0; i < frame.operation_count; ++i) {
    if (!ApplyOperation(frame.operations[i])) {
      response.status = WHISKER_APPLY_REJECTED;
      return true;
    }
  }
  for (auto parent : dirty_children_) SyncChildren(parent);
  dirty_children_.clear();
  for (auto& [id, node] : nodes_) {
    if (node.text_dirty) DrawText(node);
  }
  scene_epoch_ = frame.scene_epoch;
  revision_ = frame.target_revision;
  response.status = WHISKER_APPLY_ACCEPTED;
  response.revision = revision_;
  return true;
}

void HostScene::Clear() {
  root_.Children().RemoveAll();
  nodes_.clear();
  roots_.clear();
  dirty_children_.clear();
}

bool HostScene::ApplyOperation(const WhiskerMobileOperation& op) {
  if (op.tag == WHISKER_OP_CREATE) {
    if (op.node == 0 || nodes_.contains(op.node)) return false;
    Create(op.node);
    return true;
  }
  if (op.tag == WHISKER_OP_INSERT || op.tag == WHISKER_OP_MOVE || op.tag == WHISKER_OP_REMOVE) {
    if (!nodes_.contains(op.parent) || !nodes_.contains(op.child)) return false;
    if (op.tag == WHISKER_OP_REMOVE) {
      Detach(op.child);
    } else {
      Insert(op.parent, op.child, op.index);
    }
    return true;
  }
  auto it = nodes_.find(op.node);
  if (it == nodes_.end()) return false;
  auto& node = it->second;
  switch (op.tag) {
    case WHISKER_OP_DELETE:
      Delete(op.node);
      break;
    case WHISKER_OP_LAYOUT:
      if (!op.payload) return false;
      Layout(node, *static_cast<const WhiskerMobileLayoutGeometry*>(op.payload));
      break;
    case WHISKER_OP_PAINT: {
      if (!op.payload) return false;
      auto const& paint = *static_cast<const WhiskerMobileBoxPaint*>(op.payload);
      node.background.Brush(compositor_.CreateColorBrush(ToColor(paint.background)));
      break;
    }
    case WHISKER_OP_CLIP:
      node.clip_horizontal = op.flags & 1;
      node.clip_vertical = op.flags & 2;
      UpdateClip(node);
      break;
    case WHISKER_OP_TRANSFORM: {
      if (!op.payload || op.payload_count != 16) return false;
      auto m = static_cast<const float*>(op.payload);
      node.visual.TransformMatrix({m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9],
                                   m[10], m[11], m[12], m[13], m[14], m[15]});
      break;
    }
    case WHISKER_OP_OPACITY:
      node.visual.Opacity(op.scalar);
      break;
    case WHISKER_OP_VISIBILITY:
      node.visible = op.integer != 0;
      node.background.IsVisible(node.visible);
      if (node.text_sprite) node.text_sprite.IsVisible(node.visible);
      break;
    case WHISKER_OP_Z_ORDER:
      node.z_order = op.integer;
      dirty_children_.insert(node.parent);
      break;
    case WHISKER_OP_TEXT:
    case WHISKER_OP_TEXT_STYLE: {
      if (!op.payload) return false;
      auto const& text = *static_cast<const WhiskerMobileText*>(op.payload);
      auto content = node.text.text;
      node.text = TextStyle::From(text);
      if (op.tag == WHISKER_OP_TEXT_STYLE) node.text.text = std::move(content);
      node.has_text = true;
      node.text_dirty = true;
      break;
    }
    default:
      break;
  }
  return true;
}

void HostScene::Create(uint64_t id) {
  Node node;
  node.visual = compositor_.CreateContainerVisual();
  node.background = compositor_.CreateSpriteVisual();
  node.children_visual = compositor_.CreateContainerVisual();
  node.visual.Children().InsertAtTop(node.background);
  node.visual.Children().InsertAtTop(node.children_visual);
  nodes_.emplace(id, std::move(node));
  roots_.push_back(id);
  dirty_children_.insert(kRoot);
}

void HostScene::Delete(uint64_t id) {
  auto it = nodes_.find(id);
  if (it == nodes_.end()) return;
  for (auto child : std::vector<uint64_t>(it->second.children)) Delete(child);
  Detach(id);
  roots_.erase(std::remove(roots_.begin(), roots_.end(), id), roots_.end());
  dirty_children_.insert(kRoot);
  dirty_children_.erase(id);
  nodes_.erase(id);
}

void HostScene::Detach(uint64_t child) {
  auto& node = nodes_.at(child);
  if (node.parent == kRoot) return;
  auto& siblings = nodes_.at(node.parent).children;
  siblings.erase(std::remove(siblings.begin(), siblings.end(), child), siblings.end());
  dirty_children_.insert(node.parent);
  node.parent = kRoot;
  roots_.push_back(child);
  dirty_children_.insert(kRoot);
}

void HostScene::Insert(uint64_t parent, uint64_t child, uint32_t index) {
  auto& node = nodes_.at(child);
  if (node.parent == kRoot) {
    roots_.erase(std::remove(roots_.begin(), roots_.end(), child), roots_.end());
    dirty_children_.insert(kRoot);
  } else {
    auto& siblings = nodes_.at(node.parent).children;
    siblings.erase(std::remove(siblings.begin(), siblings.end(), child), siblings.end());
    dirty_children_.insert(node.parent);
  }
  auto& children = nodes_.at(parent).children;
  index = std::min<uint32_t>(index, static_cast<uint32_t>(children.size()));
  children.insert(children.begin() + index, child);
  node.parent = parent;
  dirty_children_.insert(parent);
}

void HostScene::SyncChildren(uint64_t parent) {
  std::vector<uint64_t> order;
  comp::VisualCollection collection{nullptr};
  if (parent == kRoot) {
    order = roots_;
    collection = root_.Children();
  } else {
    auto it = nodes_.find(parent);
    if (it == nodes_.end()) return;
    order = it->second.children;
    collection = it->second.children_visual.Children();
  }
  std::stable_sort(order.begin(), order.end(), [this](uint64_t a, uint64_t b) {
    return nodes_.at(a).z_order < nodes_.at(b).z_order;
  });
  collection.RemoveAll();
  for (auto id : order) {
    auto visual = nodes_.at(id).visual;
    if (auto old = visual.Parent()) old.Children().Remove(visual);
    collection.InsertAtTop(visual);
  }
}

void HostScene::Layout(Node& node, const WhiskerMobileLayoutGeometry& geometry) {
  node.geometry = geometry;
  auto const& border = geometry.border;
  node.visual.Offset({border.x, border.y, 0});
  node.visual.Size({border.width, border.height});
  node.background.Size({border.width, border.height});
  node.children_visual.Size({border.width, border.height});
  UpdateClip(node);
  if (node.has_text) node.text_dirty = true;
}

void HostScene::UpdateClip(Node& node) {
  if (!node.clip_horizontal && !node.clip_vertical) {
    node.children_visual.Clip(nullptr);
    return;
  }
  auto clip = compositor_.CreateInsetClip();
  if (!node.clip_horizontal) {
    clip.LeftInset(-kUnclipped);
    clip.RightInset(-kUnclipped);
  }
  if (!node.clip_vertical) {
    clip.TopInset(-kUnclipped);
    clip.BottomInset(-kUnclipped);
  }
  node.children_visual.Clip(clip);
}

void HostScene::DrawText(Node& node) {
  node.text_dirty = false;
  auto const& content = node.geometry.content;
  if (content.width <= 0 || content.height <= 0 || node.text.text.empty()) {
    if (node.text_sprite) node.text_sprite.Brush(nullptr);
    return;
  }
  if (!node.text_sprite) {
    node.text_sprite = compositor_.CreateSpriteVisual();
    node.text_sprite.IsVisible(node.visible);
    node.visual.Children().InsertAbove(node.text_sprite, node.background);
  }
  winrt::Windows::Graphics::SizeInt32 pixels{
      static_cast<int32_t>(std::ceil(content.width * scale_)),
      static_cast<int32_t>(std::ceil(content.height * scale_))};
  auto surface = graphics_.CreateDrawingSurface2(pixels, dx::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                                 dx::DirectXAlphaMode::Premultiplied);
  auto brush = compositor_.CreateSurfaceBrush(surface);
  brush.Stretch(comp::CompositionStretch::Fill);
  node.text_sprite.Brush(brush);
  node.text_sprite.Offset({content.x, content.y, 0});
  node.text_sprite.Size({content.width, content.height});
  DrawSurface(surface, scale_,
              [&](ID2D1DeviceContext* context) { text_.Draw(context, node.text, content.width); });
}

}  // namespace whisker
