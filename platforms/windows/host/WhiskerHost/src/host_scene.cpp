#include "host_scene.h"

#include "paint.h"
#include "value.h"

#include <algorithm>
#include <cmath>
#include <functional>
#include <string_view>
#include <utility>

#include <windows.graphics.interop.h>
#include <winrt/Microsoft.Graphics.DirectX.h>
#include <winrt/Windows.Graphics.h>
#include <winrt/Windows.Foundation.Numerics.h>
#include <winrt/Windows.UI.h>

namespace whisker {

namespace comp = winrt::Microsoft::UI::Composition;
namespace dx = winrt::Microsoft::Graphics::DirectX;

namespace {

constexpr float kUnclipped = 1.0e5f;
constexpr uint64_t kRoot = 0;
constexpr uint32_t kScrollToCommand = 1;
constexpr uint32_t kScrollByCommand = 2;
constexpr int32_t kHitTestNone = 1;
constexpr int32_t kHitTestBoxOnly = 2;
constexpr int32_t kHitTestDescendantsOnly = 3;

winrt::Windows::UI::Color ToColor(const WhiskerMobileColor& color) {
  auto c = ToD2DColor(color);
  auto byte = [](float v) { return static_cast<uint8_t>(std::lround(v * 255.0f)); };
  return {byte(c.a), byte(c.r), byte(c.g), byte(c.b)};
}

// Composition may ask for the geometry in its own factory, so it is rebuilt on demand.
struct GeometrySource
    : winrt::implements<GeometrySource, winrt::Windows::Graphics::IGeometrySource2D,
                        ABI::Windows::Graphics::IGeometrySource2DInterop> {
  using Build = std::function<winrt::com_ptr<ID2D1Geometry>(ID2D1Factory*)>;
  explicit GeometrySource(Build build) : build_(std::move(build)) {}
  HRESULT __stdcall GetGeometry(ID2D1Geometry** value) override;
  HRESULT __stdcall TryGetGeometryUsingFactory(ID2D1Factory* factory,
                                               ID2D1Geometry** value) override try {
    build_(factory).copy_to(value);
    return S_OK;
  } catch (...) {
    *value = nullptr;
    return winrt::to_hresult();
  }
  Build build_;
};

ID2D1Factory* SharedFactory() {
  static winrt::com_ptr<ID2D1Factory> factory = [] {
    winrt::com_ptr<ID2D1Factory> created;
    winrt::check_hresult(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, created.put()));
    return created;
  }();
  return factory.get();
}

HRESULT __stdcall GeometrySource::GetGeometry(ID2D1Geometry** value) {
  return TryGetGeometryUsingFactory(SharedFactory(), value);
}

BoxGeometry Boxes(const std::optional<WhiskerMobileBoxPaint>& paint,
                  const WhiskerMobileLayoutGeometry& geometry) {
  auto const& border = geometry.border;
  auto const& content = geometry.content;
  auto rounded = RoundedRect::From(paint ? &*paint : nullptr, border.width, border.height);
  auto widths = paint ? BorderWidths(*paint, border.width) : std::array<float, 4>{};
  return {rounded, rounded.Shrink(widths[0], widths[1], widths[2], widths[3]),
          rounded.Shrink(content.y, border.width - content.x - content.width,
                         border.height - content.y - content.height, content.x)};
}

bool HasInsetShadow(const std::vector<BoxShadow>& shadows) {
  return std::any_of(shadows.begin(), shadows.end(), [](auto const& s) { return s.inset; });
}

}  // namespace

bool HostScene::Bootstrap(const WhiskerMobileBootstrap& bootstrap) {
  if (bootstrap.abi_major != WHISKER_MOBILE_ABI_MAJOR ||
      (bootstrap.registration_count && !bootstrap.registrations)) {
    return false;
  }
  elements_.clear();
  for (size_t i = 0; i < bootstrap.registration_count; ++i) {
    auto const& raw = bootstrap.registrations[i];
    Element element;
    element.kind = View(raw.name) == "whisker.ui/ScrollView" ? Kind::ScrollView : Kind::View;
    for (size_t p = 0; p < raw.property_count; ++p) {
      element.properties.emplace(raw.properties[p].id, std::string(View(raw.properties[p].name)));
    }
    for (size_t c = 0; c < raw.command_count; ++c) {
      element.commands.emplace(raw.commands[c].id, std::string(View(raw.commands[c].name)));
    }
    element.view = FindModuleView(View(raw.name));
    for (size_t e = 0; e < raw.event_count; ++e) {
      auto id = raw.events[e].id;
      if (View(raw.events[e].name) == "scroll" && id >= 1 && id <= 64) {
        element.scroll_event_mask = uint64_t{1} << (id - 1);
      }
    }
    elements_.emplace(raw.element_type, std::move(element));
  }
  return true;
}

HostScene::HostScene(comp::ContainerVisual root, const TextEngine& text,
                     const ResourceService& resources, comp::CompositionGraphicsDevice graphics)
    : compositor_(root.Compositor()),
      root_(std::move(root)),
      graphics_(std::move(graphics)),
      text_(text),
      resources_(resources) {}

void HostScene::SetFrameRequester(std::function<void()> request_frame) {
  request_frame_ = std::move(request_frame);
}

void HostScene::SetScale(float scale) {
  if (scale == scale_) return;
  scale_ = scale;
  for (auto& [id, node] : nodes_) {
    node.box_dirty = true;
    if (node.has_text) node.text_dirty = true;
    if (node.view) dirty_views_.insert(id);
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
    if (node.box_dirty) DrawBox(node);
    if (node.text_dirty) DrawText(node);
  }
  DrawViews();
  for (auto& [id, node] : nodes_) {
    if (node.group) FitGroup(node);
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
  scroll_events_.clear();
  scroll_offsets_.clear();
  view_events_.clear();
  dirty_views_.clear();
}

bool HostScene::ApplyOperation(const WhiskerMobileOperation& op) {
  if (op.tag == WHISKER_OP_CREATE) {
    if (op.node == 0 || nodes_.contains(op.node)) return false;
    Create(op.node);
    auto& node = nodes_.at(op.node);
    node.element_type = op.member;
    if (auto element = ElementOf(node); element && element->kind == Kind::ScrollView) {
      node.scroll.emplace();
      node.scroll_content = compositor_.CreateContainerVisual();
      node.children_visual.Children().InsertAtTop(node.scroll_content);
      UpdateClip(node);
    }
    if (auto element = ElementOf(node); element && element->view) {
      CreateView(op.node, *element->view);
    }
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
      if (node.view) dirty_views_.insert(op.node);
      break;
    case WHISKER_OP_PAINT: {
      if (!op.payload) return false;
      node.paint = OwnedPaint(*static_cast<const WhiskerMobileBoxPaint*>(op.payload));
      node.box_dirty = true;
      UpdateClip(node);
      UpdateClipPath(node);
      break;
    }
    case WHISKER_OP_BACKGROUND_LAYERS: {
      if (op.payload_count && !op.payload) return false;
      node.layers.clear();
      auto layers = static_cast<const WhiskerMobileBackgroundLayer*>(op.payload);
      for (size_t i = 0; i < op.payload_count; ++i) {
        if (auto layer = BackgroundLayer::From(layers[i], resources_)) {
          node.layers.push_back(std::move(*layer));
        }
      }
      node.box_dirty = true;
      break;
    }
    case WHISKER_OP_BOX_SHADOWS: {
      if (op.payload_count && !op.payload) return false;
      node.shadows.clear();
      auto shadows = static_cast<const WhiskerMobileBoxShadow*>(op.payload);
      for (size_t i = 0; i < op.payload_count; ++i) {
        if (auto shadow = BoxShadow::From(shadows[i])) node.shadows.push_back(*shadow);
      }
      node.box_dirty = true;
      break;
    }
    case WHISKER_OP_IMAGE_RENDERING:
      node.image_rendering = static_cast<uint32_t>(op.integer);
      node.box_dirty = true;
      break;
    case WHISKER_OP_CLIP_PATH:
      node.clip_path.reset();
      if (op.payload) {
        node.clip_path = ClipPath::From(*static_cast<const WhiskerMobileClipPath*>(op.payload));
      }
      UpdateClipPath(node);
      break;
    case WHISKER_OP_CLIP:
      node.clip_horizontal = op.flags & 1;
      node.clip_vertical = op.flags & 2;
      UpdateClip(node);
      break;
    case WHISKER_OP_TRANSFORM: {
      if (!op.payload || op.payload_count != 16) return false;
      auto m = static_cast<const float*>(op.payload);
      node.transform = {m[0], m[1], m[4], m[5], m[12], m[13]};
      node.visual.TransformMatrix({m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9],
                                   m[10], m[11], m[12], m[13], m[14], m[15]});
      break;
    }
    case WHISKER_OP_OPACITY:
      if (op.scalar < 1 && !node.group) Flatten(node);
      if (node.group) {
        node.group_sprite.Opacity(op.scalar);
      } else {
        node.visual.Opacity(op.scalar);
      }
      break;
    case WHISKER_OP_VISIBILITY:
      node.visible = op.integer != 0;
      node.background.IsVisible(node.visible);
      if (node.shadow) node.shadow.IsVisible(node.visible);
      if (node.text_sprite) node.text_sprite.IsVisible(node.visible);
      if (node.view_sprite) node.view_sprite.IsVisible(node.visible);
      break;
    case WHISKER_OP_Z_ORDER:
      node.z_order = op.integer;
      dirty_children_.insert(node.parent);
      break;
    case WHISKER_OP_PROPERTY:
      if (!op.payload) return false;
      SetProperty(op.node, node, op.member, static_cast<const WhiskerValueRaw*>(op.payload));
      break;
    case WHISKER_OP_CLEAR_PROPERTY:
      SetProperty(op.node, node, op.member, nullptr);
      break;
    case WHISKER_OP_EVENT_MASK:
      node.event_mask = op.wide;
      break;
    case WHISKER_OP_HIT_TEST:
      node.hit_test = op.integer;
      break;
    case WHISKER_OP_CURSOR:
      node.cursor = op.integer;
      break;
    case WHISKER_OP_COMMAND:
      if (!op.payload) return false;
      Invoke(op.node, op.member, static_cast<const WhiskerValueRaw*>(op.payload));
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

// Composition fades each child separately (even under a LayerVisual), so translucent nodes draw
// their subtree into one surface and fade that.
void HostScene::Flatten(Node& node) {
  auto size = node.visual.Size();
  node.visual.Children().RemoveAll();
  node.group = compositor_.CreateContainerVisual();
  node.group.Size(size);
  auto children = node.group.Children();
  if (node.shadow) children.InsertAtTop(node.shadow);
  children.InsertAtTop(node.background);
  if (node.view_sprite) children.InsertAtTop(node.view_sprite);
  if (node.text_sprite) children.InsertAtTop(node.text_sprite);
  children.InsertAtTop(node.children_visual);
  node.group_surface = compositor_.CreateVisualSurface();
  node.group_surface.SourceVisual(node.group);
  node.group_surface.SourceSize(size);
  node.group_sprite = compositor_.CreateSpriteVisual();
  node.group_sprite.Size(size);
  node.group_sprite.Brush(compositor_.CreateSurfaceBrush(node.group_surface));
  node.visual.Children().InsertAtTop(node.group_sprite);
  FitGroup(node);
}

void HostScene::FitGroup(Node& node) {
  auto bounds = PaintBounds(node);
  D2D1_SIZE_F size{std::max(0.0f, bounds.right - bounds.left), std::max(0.0f, bounds.bottom - bounds.top)};
  node.group_surface.SourceOffset({bounds.left, bounds.top});
  node.group_surface.SourceSize({size.width, size.height});
  node.group_sprite.Offset({bounds.left, bounds.top, 0});
  node.group_sprite.Size({size.width, size.height});
}

D2D1_RECT_F HostScene::PaintBounds(const Node& node) const {
  auto const& border = node.geometry.border;
  float extent = OuterShadowExtent(node.shadows);
  D2D1_RECT_F bounds{-extent, -extent, border.width + extent, border.height + extent};
  auto unite = [&bounds](D2D1_RECT_F r) {
    if (r.right <= r.left || r.bottom <= r.top) return;
    bounds = {std::min(bounds.left, r.left), std::min(bounds.top, r.top),
              std::max(bounds.right, r.right), std::max(bounds.bottom, r.bottom)};
  };
  if (node.has_text) {
    auto const& content = node.geometry.content;
    float overflow = node.text.PaintOverflow();
    unite({content.x - overflow, content.y - overflow, content.x + content.width + overflow,
           content.y + content.height + overflow});
  }
  bool clip_x = node.clip_horizontal || node.scroll;
  bool clip_y = node.clip_vertical || node.scroll;
  for (auto id : node.children) {
    auto const& child = nodes_.at(id);
    auto r = PaintBounds(child);
    auto const& m = child.transform;
    float xs[4];
    float ys[4];
    D2D1_POINT_2F corners[] = {{r.left, r.top}, {r.right, r.top}, {r.left, r.bottom}, {r.right, r.bottom}};
    for (int i = 0; i < 4; ++i) {
      xs[i] = corners[i].x * m[0] + corners[i].y * m[2] + m[4] + child.geometry.border.x;
      ys[i] = corners[i].x * m[1] + corners[i].y * m[3] + m[5] + child.geometry.border.y;
    }
    D2D1_RECT_F mapped{*std::min_element(xs, xs + 4), *std::min_element(ys, ys + 4),
                       *std::max_element(xs, xs + 4), *std::max_element(ys, ys + 4)};
    if (clip_x) {
      mapped.left = std::max(mapped.left, 0.0f);
      mapped.right = std::min(mapped.right, border.width);
    }
    if (clip_y) {
      mapped.top = std::max(mapped.top, 0.0f);
      mapped.bottom = std::min(mapped.bottom, border.height);
    }
    unite(mapped);
  }
  return bounds;
}

void HostScene::Delete(uint64_t id) {
  auto it = nodes_.find(id);
  if (it == nodes_.end()) return;
  for (auto child : std::vector<uint64_t>(it->second.children)) Delete(child);
  Detach(id);
  roots_.erase(std::remove(roots_.begin(), roots_.end(), id), roots_.end());
  dirty_children_.insert(kRoot);
  dirty_children_.erase(id);
  dirty_views_.erase(id);
  nodes_.erase(id);
  scroll_offsets_.erase(id);
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
    auto const& node = it->second;
    collection = (node.scroll_content ? node.scroll_content : node.children_visual).Children();
  }
  order = PaintOrder(order);
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
  if (node.scroll_content) node.scroll_content.Size({border.width, border.height});
  if (node.group) node.group.Size({border.width, border.height});
  UpdateClip(node);
  UpdateClipPath(node);
  node.box_dirty = true;
  if (node.has_text) node.text_dirty = true;
}

void HostScene::UpdateClip(Node& node) {
  bool horizontal = node.clip_horizontal || node.scroll;
  bool vertical = node.clip_vertical || node.scroll;
  if (!horizontal && !vertical) {
    node.children_visual.Clip(nullptr);
    return;
  }
  auto const& border = node.geometry.border;
  if (horizontal && vertical) {
    auto rounded = RoundedRect::From(node.paint ? &*node.paint : nullptr, border.width, border.height);
    if (!rounded.IsRectilinear()) {
      auto source = winrt::make<GeometrySource>(
          [rounded](ID2D1Factory* factory) { return rounded.Geometry(factory).as<ID2D1Geometry>(); });
      auto geometry = compositor_.CreatePathGeometry(comp::CompositionPath(source));
      node.children_visual.Clip(compositor_.CreateGeometricClip(geometry));
      return;
    }
  }
  auto clip = compositor_.CreateInsetClip();
  if (!horizontal) {
    clip.LeftInset(-kUnclipped);
    clip.RightInset(-kUnclipped);
  }
  if (!vertical) {
    clip.TopInset(-kUnclipped);
    clip.BottomInset(-kUnclipped);
  }
  node.children_visual.Clip(clip);
}

void HostScene::UpdateClipPath(Node& node) {
  if (!node.clip_path) {
    node.visual.Clip(nullptr);
    return;
  }
  auto source = winrt::make<GeometrySource>(
      [clip = *node.clip_path, boxes = Boxes(node.paint, node.geometry)](ID2D1Factory* factory) {
        return clip.Geometry(factory, boxes);
      });
  auto geometry = compositor_.CreatePathGeometry(comp::CompositionPath(source));
  node.visual.Clip(compositor_.CreateGeometricClip(geometry));
}

void HostScene::DrawBox(Node& node) {
  node.box_dirty = false;
  DrawShadow(node);
  auto const& border = node.geometry.border;
  if (!node.paint && node.layers.empty() && !HasInsetShadow(node.shadows)) {
    node.background.Brush(nullptr);
    return;
  }
  auto const paint = node.paint.value_or(WhiskerMobileBoxPaint{});
  if ((node.layers.empty() && !HasInsetShadow(node.shadows) &&
       !NeedsBoxSurface(paint, border.width, border.height)) ||
      border.width <= 0 || border.height <= 0) {
    node.background.Brush(compositor_.CreateColorBrush(ToColor(paint.background)));
    return;
  }
  winrt::Windows::Graphics::SizeInt32 pixels{
      static_cast<int32_t>(std::ceil(border.width * scale_)),
      static_cast<int32_t>(std::ceil(border.height * scale_))};
  auto surface = graphics_.CreateDrawingSurface2(pixels, dx::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                                 dx::DirectXAlphaMode::Premultiplied);
  auto brush = compositor_.CreateSurfaceBrush(surface);
  brush.Stretch(comp::CompositionStretch::Fill);
  node.background.Brush(brush);
  DrawSurface(surface, scale_, [&](ID2D1DeviceContext* context) {
    auto const& content = node.geometry.content;
    PaintBox(context, paint, node.layers, node.shadows, node.image_rendering, border.width,
             border.height,
             {content.x, content.y, content.x + content.width, content.y + content.height});
  });
}

void HostScene::DrawShadow(Node& node) {
  auto const& border = node.geometry.border;
  float extent = OuterShadowExtent(node.shadows);
  bool outer = std::any_of(node.shadows.begin(), node.shadows.end(),
                           [](auto const& s) { return !s.inset; });
  if (!outer || border.width <= 0 || border.height <= 0) {
    if (node.shadow) node.shadow.Brush(nullptr);
    return;
  }
  if (!node.shadow) {
    node.shadow = compositor_.CreateSpriteVisual();
    node.shadow.IsVisible(node.visible);
    (node.group ? node.group : node.visual).Children().InsertAtBottom(node.shadow);
  }
  float width = border.width + extent * 2;
  float height = border.height + extent * 2;
  winrt::Windows::Graphics::SizeInt32 pixels{static_cast<int32_t>(std::ceil(width * scale_)),
                                             static_cast<int32_t>(std::ceil(height * scale_))};
  auto surface = graphics_.CreateDrawingSurface2(pixels, dx::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                                 dx::DirectXAlphaMode::Premultiplied);
  auto brush = compositor_.CreateSurfaceBrush(surface);
  brush.Stretch(comp::CompositionStretch::Fill);
  node.shadow.Brush(brush);
  node.shadow.Offset({-extent, -extent, 0});
  node.shadow.Size({width, height});
  auto rounded = RoundedRect::From(node.paint ? &*node.paint : nullptr, border.width, border.height);
  DrawSurface(surface, scale_, [&](ID2D1DeviceContext* context) {
    D2D1_MATRIX_3X2_F transform;
    context->GetTransform(&transform);
    context->SetTransform(D2D1::Matrix3x2F::Translation(extent, extent) * transform);
    PaintOuterShadows(context, node.shadows, rounded);
  });
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
    auto parent = node.group ? node.group : node.visual;
    parent.Children().InsertAbove(node.text_sprite, node.background);
  }
  float overflow = node.text.PaintOverflow();
  float width = content.width + overflow * 2;
  float height = content.height + overflow * 2;
  winrt::Windows::Graphics::SizeInt32 pixels{static_cast<int32_t>(std::ceil(width * scale_)),
                                             static_cast<int32_t>(std::ceil(height * scale_))};
  auto surface = graphics_.CreateDrawingSurface2(pixels, dx::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                                 dx::DirectXAlphaMode::Premultiplied);
  auto brush = compositor_.CreateSurfaceBrush(surface);
  brush.Stretch(comp::CompositionStretch::Fill);
  node.text_sprite.Brush(brush);
  node.text_sprite.Offset({content.x - overflow, content.y - overflow, 0});
  node.text_sprite.Size({width, height});
  DrawSurface(surface, scale_, [&](ID2D1DeviceContext* context) {
    D2D1_MATRIX_3X2_F transform;
    context->GetTransform(&transform);
    context->SetTransform(D2D1::Matrix3x2F::Translation(overflow, overflow) * transform);
    text_.Draw(context, node.text, content.width);
  });
}

const HostScene::Element* HostScene::ElementOf(const Node& node) const {
  auto it = elements_.find(node.element_type);
  return it == elements_.end() ? nullptr : &it->second;
}

void HostScene::SetProperty(uint64_t id, Node& node, uint32_t property,
                            const WhiskerValueRaw* value) {
  auto element = ElementOf(node);
  if (!element) return;
  auto name = element->properties.find(property);
  if (name == element->properties.end()) return;
  if (node.view) {
    auto handler = node.view_definition->properties.find(name->second);
    if (handler != node.view_definition->properties.end()) {
      handler->second(*node.view, FromRaw(value));
      dirty_views_.insert(id);
    }
    return;
  }
  if (!node.scroll) return;
  if (name->second == "scroll-orientation") {
    node.scroll->horizontal = value && value->type == WHISKER_VALUE_STRING &&
                              View(value->v.s) == "horizontal";
  } else if (name->second == "enable-scroll") {
    node.scroll->enabled = !value || value->type != WHISKER_VALUE_BOOL || value->v.b;
  }
}

void HostScene::Invoke(uint64_t id, uint32_t command, const WhiskerValueRaw* arguments) {
  auto& node = nodes_.at(id);
  if (node.view) {
    auto element = ElementOf(node);
    if (!element) return;
    auto name = element->commands.find(command);
    if (name == element->commands.end()) return;
    auto handler = node.view_definition->commands.find(name->second);
    if (handler != node.view_definition->commands.end()) {
      handler->second(*node.view, FromRaw(arguments));
      dirty_views_.insert(id);
    }
    return;
  }
  if (!node.scroll || (command != kScrollToCommand && command != kScrollByCommand)) return;
  float distance = Number(MapEntry(arguments, "offset"));
  auto offset = node.scroll->offset;
  size_t axis = node.scroll->horizontal ? 0 : 1;
  offset[axis] = command == kScrollByCommand ? offset[axis] + distance : distance;
  ScrollTo(id, offset);
}

std::array<float, 2> HostScene::ContentExtent(const Node& node) const {
  auto const& border = node.geometry.border;
  auto const& content = node.geometry.content;
  float trailing_x = border.width - content.x - content.width;
  float trailing_y = border.height - content.y - content.height;
  std::array<float, 2> extent{border.width, border.height};
  for (auto child : node.children) {
    auto const& rect = nodes_.at(child).geometry.border;
    extent[0] = std::max(extent[0], rect.x + rect.width + trailing_x);
    extent[1] = std::max(extent[1], rect.y + rect.height + trailing_y);
  }
  return extent;
}

std::array<float, 2> HostScene::MaxScrollOffset(const Node& node) const {
  if (!node.scroll) return {0, 0};
  auto extent = ContentExtent(node);
  auto const& border = node.geometry.border;
  std::array<float, 2> max{std::max(0.0f, extent[0] - border.width),
                           std::max(0.0f, extent[1] - border.height)};
  max[node.scroll->horizontal ? 1 : 0] = 0;
  return max;
}

bool HostScene::ScrollTo(uint64_t id, std::array<float, 2> offset) {
  auto& node = nodes_.at(id);
  if (!node.scroll) return false;
  auto max = MaxScrollOffset(node);
  std::array<float, 2> clamped{std::clamp(offset[0], 0.0f, max[0]),
                               std::clamp(offset[1], 0.0f, max[1])};
  if (clamped == node.scroll->offset) return false;
  node.scroll->offset = clamped;
  node.scroll_content.Offset({-clamped[0], -clamped[1], 0});
  scroll_offsets_[id] = clamped;
  auto element = ElementOf(node);
  if (element && (node.event_mask & element->scroll_event_mask)) {
    auto extent = ContentExtent(node);
    scroll_events_.push_back({id, clamped[0], clamped[1], extent[0], extent[1],
                              node.geometry.border.width, node.geometry.border.height});
  }
  return true;
}

std::vector<uint64_t> HostScene::PaintOrder(const std::vector<uint64_t>& ids) const {
  auto order = ids;
  std::stable_sort(order.begin(), order.end(), [this](uint64_t a, uint64_t b) {
    return nodes_.at(a).z_order < nodes_.at(b).z_order;
  });
  return order;
}

std::optional<uint64_t> HostScene::HitTest(const std::vector<uint64_t>& ids, float x,
                                           float y) const {
  auto order = PaintOrder(ids);
  for (auto it = order.rbegin(); it != order.rend(); ++it) {
    auto const& node = nodes_.at(*it);
    if (!node.visible || node.hit_test == kHitTestNone) continue;
    auto const& border = node.geometry.border;
    auto const& m = node.transform;
    float px = x - border.x - m[4];
    float py = y - border.y - m[5];
    float determinant = m[0] * m[3] - m[1] * m[2];
    if (determinant == 0) continue;
    float lx = (px * m[3] - py * m[2]) / determinant;
    float ly = (py * m[0] - px * m[1]) / determinant;
    bool inside = lx >= 0 && ly >= 0 && lx < border.width && ly < border.height;
    bool clips = node.scroll || (node.clip_horizontal && node.clip_vertical);
    if (node.hit_test != kHitTestBoxOnly && (inside || !clips)) {
      float sx = node.scroll ? node.scroll->offset[0] : 0;
      float sy = node.scroll ? node.scroll->offset[1] : 0;
      if (auto hit = HitTest(node.children, lx + sx, ly + sy)) return hit;
    }
    if (inside && node.hit_test != kHitTestDescendantsOnly) return *it;
  }
  return std::nullopt;
}

int32_t HostScene::CursorAt(float x, float y) const {
  auto hit = HitTest(roots_, x, y);
  for (auto id = hit; id && *id != kRoot;) {
    auto const& node = nodes_.at(*id);
    if (node.cursor != 0) return node.cursor;
    id = node.parent;
  }
  return 0;
}

bool HostScene::ScrollAt(float x, float y, float dx, float dy) {
  auto hit = HitTest(roots_, x, y);
  for (auto id = hit; id && *id != kRoot;) {
    auto const& node = nodes_.at(*id);
    if (node.scroll && node.scroll->enabled) {
      size_t axis = node.scroll->horizontal ? 0 : 1;
      std::array<float, 2> delta{dx, dy};
      float amount = delta[axis] == 0 ? delta[1 - axis] : delta[axis];
      auto offset = node.scroll->offset;
      offset[axis] += amount;
      if (ScrollTo(*id, offset)) return true;
    }
    id = node.parent;
  }
  return false;
}

void HostScene::CreateView(uint64_t id, const ViewDefinition& definition) {
  auto& node = nodes_.at(id);
  node.view_definition = &definition;
  node.view_context = std::make_unique<NodeViewContext>(*this, id);
  node.view = definition.create(*node.view_context);
  node.view_sprite = compositor_.CreateSpriteVisual();
  node.view_sprite.IsVisible(node.visible);
  node.visual.Children().InsertAbove(node.view_sprite, node.background);
}

void HostScene::DrawViews() {
  for (auto id : std::exchange(dirty_views_, {})) {
    auto it = nodes_.find(id);
    if (it != nodes_.end() && it->second.view) DrawView(it->second);
  }
}

void HostScene::DrawView(Node& node) {
  auto const& content = node.geometry.content;
  if (content.width <= 0 || content.height <= 0) {
    node.view_sprite.Brush(nullptr);
    return;
  }
  winrt::Windows::Graphics::SizeInt32 pixels{static_cast<int32_t>(std::ceil(content.width * scale_)),
                                             static_cast<int32_t>(std::ceil(content.height * scale_))};
  auto surface = graphics_.CreateDrawingSurface2(pixels, dx::DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                                 dx::DirectXAlphaMode::Premultiplied);
  auto brush = compositor_.CreateSurfaceBrush(surface);
  brush.Stretch(comp::CompositionStretch::Fill);
  node.view_sprite.Brush(brush);
  node.view_sprite.Offset({content.x, content.y, 0});
  node.view_sprite.Size({content.width, content.height});
  DrawSurface(surface, scale_, [&](ID2D1DeviceContext* context) {
    node.view->Draw(context, content.width, content.height);
  });
}

void HostScene::NodeViewContext::Emit(std::string_view event, Value detail) {
  scene_.view_events_.push_back({id_, std::string(event), std::move(detail)});
  if (scene_.request_frame_) scene_.request_frame_();
}

void HostScene::NodeViewContext::Invalidate() {
  scene_.dirty_views_.insert(id_);
  if (scene_.request_frame_) scene_.request_frame_();
}

std::vector<HostScene::ViewEvent> HostScene::TakeViewEvents() { return std::exchange(view_events_, {}); }

std::vector<HostScene::ScrollEvent> HostScene::TakeScrollEvents() {
  return std::exchange(scroll_events_, {});
}

std::vector<std::pair<uint64_t, std::array<float, 2>>> HostScene::TakeScrollOffsets() {
  std::vector<std::pair<uint64_t, std::array<float, 2>>> offsets(scroll_offsets_.begin(),
                                                                 scroll_offsets_.end());
  scroll_offsets_.clear();
  return offsets;
}

}  // namespace whisker
