#include "clip_path.h"

#include <algorithm>
#include <cmath>

namespace whisker {

namespace {

float Resolve(const WhiskerMobileLengthPercentage& value, float basis) {
  return value.length + value.fraction * basis;
}

winrt::com_ptr<ID2D1Geometry> Ellipse(ID2D1Factory* factory, float cx, float cy, float rx,
                                      float ry) {
  winrt::com_ptr<ID2D1EllipseGeometry> geometry;
  winrt::check_hresult(factory->CreateEllipseGeometry(
      D2D1::Ellipse({cx, cy}, std::max(0.0f, rx), std::max(0.0f, ry)), geometry.put()));
  return geometry.as<ID2D1Geometry>();
}

}  // namespace

std::optional<ClipPath> ClipPath::From(const WhiskerMobileClipPath& raw) {
  if (raw.reference_box > WHISKER_BACKGROUND_BOX_CONTENT || !raw.payload ||
      raw.payload_count != 1) {
    return std::nullopt;
  }
  ClipPath clip;
  clip.reference_box = raw.reference_box;
  clip.shape_kind = raw.shape_kind;
  switch (raw.shape_kind) {
    case WHISKER_CLIP_SHAPE_INSET:
      clip.inset = *static_cast<const WhiskerMobileClipInset*>(raw.payload);
      break;
    case WHISKER_CLIP_SHAPE_CIRCLE: {
      auto const& circle = *static_cast<const WhiskerMobileClipCircle*>(raw.payload);
      clip.values = {circle.radius, circle.radius, circle.center_x, circle.center_y};
      break;
    }
    case WHISKER_CLIP_SHAPE_ELLIPSE: {
      auto const& ellipse = *static_cast<const WhiskerMobileClipEllipse*>(raw.payload);
      clip.values = {ellipse.radius_x, ellipse.radius_y, ellipse.center_x, ellipse.center_y};
      break;
    }
    case WHISKER_CLIP_SHAPE_PATH: {
      auto const& path = *static_cast<const WhiskerMobileClipPathCommands*>(raw.payload);
      if (path.command_count && !path.commands) return std::nullopt;
      clip.fill_rule = path.fill_rule;
      clip.commands.assign(path.commands, path.commands + path.command_count);
      break;
    }
    default:
      return std::nullopt;
  }
  return clip;
}

winrt::com_ptr<ID2D1Geometry> ClipPath::Geometry(ID2D1Factory* factory,
                                                 const BoxGeometry& boxes) const {
  auto const box = reference_box == WHISKER_BACKGROUND_BOX_PADDING   ? boxes.padding.rect
                   : reference_box == WHISKER_BACKGROUND_BOX_CONTENT ? boxes.content.rect
                                                                     : boxes.border.rect;
  float w = box.right - box.left;
  float h = box.bottom - box.top;
  switch (shape_kind) {
    case WHISKER_CLIP_SHAPE_INSET: {
      RoundedRect rounded;
      float left = box.left + Resolve(inset.edges[3], w);
      float top = box.top + Resolve(inset.edges[0], h);
      rounded.rect = D2D1::RectF(left, top, std::max(left, box.right - Resolve(inset.edges[1], w)),
                                 std::max(top, box.bottom - Resolve(inset.edges[2], h)));
      float iw = rounded.rect.right - rounded.rect.left;
      float ih = rounded.rect.bottom - rounded.rect.top;
      for (int corner = 0; corner < 4; ++corner) {
        float rx = std::max(0.0f, Resolve(inset.radii_horizontal[corner], iw));
        float ry = std::max(0.0f, Resolve(inset.radii_vertical[corner], ih));
        rounded.radii[corner] = rx == 0 || ry == 0 ? D2D1_SIZE_F{0, 0} : D2D1_SIZE_F{rx, ry};
      }
      rounded.FitRadii();
      return rounded.Geometry(factory).as<ID2D1Geometry>();
    }
    case WHISKER_CLIP_SHAPE_CIRCLE: {
      float r = Resolve(values[0], std::hypot(w, h) / std::sqrt(2.0f));
      return Ellipse(factory, box.left + Resolve(values[2], w), box.top + Resolve(values[3], h), r,
                     r);
    }
    case WHISKER_CLIP_SHAPE_ELLIPSE:
      return Ellipse(factory, box.left + Resolve(values[2], w), box.top + Resolve(values[3], h),
                     Resolve(values[0], w), Resolve(values[1], h));
    default:
      break;
  }
  winrt::com_ptr<ID2D1PathGeometry> geometry;
  winrt::check_hresult(factory->CreatePathGeometry(geometry.put()));
  winrt::com_ptr<ID2D1GeometrySink> sink;
  winrt::check_hresult(geometry->Open(sink.put()));
  sink->SetFillMode(fill_rule == WHISKER_FILL_RULE_EVEN_ODD ? D2D1_FILL_MODE_ALTERNATE
                                                            : D2D1_FILL_MODE_WINDING);
  bool open = false;
  D2D1_POINT_2F current{box.left, box.top};
  D2D1_POINT_2F start = current;
  auto point = [&](const WhiskerMobilePathCommand& command, int slot) {
    return D2D1_POINT_2F{box.left + Resolve(command.points[slot], w),
                         box.top + Resolve(command.points[slot + 1], h)};
  };
  auto begin = [&] {
    if (open) return;
    sink->BeginFigure(current, D2D1_FIGURE_BEGIN_FILLED);
    start = current;
    open = true;
  };
  for (auto const& command : commands) {
    switch (command.kind) {
      case WHISKER_PATH_MOVE_TO:
        if (open) sink->EndFigure(D2D1_FIGURE_END_OPEN);
        open = false;
        current = point(command, 0);
        break;
      case WHISKER_PATH_LINE_TO:
        begin();
        current = point(command, 0);
        sink->AddLine(current);
        break;
      case WHISKER_PATH_QUADRATIC_TO:
        begin();
        current = point(command, 2);
        sink->AddQuadraticBezier(D2D1::QuadraticBezierSegment(point(command, 0), current));
        break;
      case WHISKER_PATH_CUBIC_TO:
        begin();
        current = point(command, 4);
        sink->AddBezier(D2D1::BezierSegment(point(command, 0), point(command, 2), current));
        break;
      case WHISKER_PATH_CLOSE:
        if (open) sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        open = false;
        current = start;
        break;
      default:
        break;
    }
  }
  if (open) sink->EndFigure(D2D1_FIGURE_END_OPEN);
  winrt::check_hresult(sink->Close());
  return geometry.as<ID2D1Geometry>();
}

}  // namespace whisker
