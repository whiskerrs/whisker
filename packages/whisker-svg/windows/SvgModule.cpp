// `whisker-svg` ModuleDefinition (Windows): replays the display list from SPEC.md with Direct2D.

#include <whisker/module.h>

#include <wrl/client.h>

#include <algorithm>
#include <charconv>
#include <cstring>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;

constexpr uint8_t kMagic[4] = {0x57, 0x53, 0x44, 0x4C};
constexpr uint8_t kVersion = 1;
constexpr size_t kHeaderLen = 6;

std::vector<uint8_t> DecodeBase64(std::string_view text) {
  auto digit = [](char c) -> int {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+') return 62;
    if (c == '/') return 63;
    return -1;
  };
  std::vector<uint8_t> bytes;
  uint32_t buffer = 0;
  int bits = 0;
  for (char c : text) {
    if (c == '=') break;
    int value = digit(c);
    if (value < 0) return {};
    buffer = (buffer << 6) | static_cast<uint32_t>(value);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bytes.push_back(static_cast<uint8_t>(buffer >> bits));
    }
  }
  return bytes;
}

std::optional<D2D1_COLOR_F> ParseHex(std::string_view hex) {
  uint32_t n = 0;
  if (hex.size() != 3 && hex.size() != 6 && hex.size() != 8) return std::nullopt;
  auto [end, error] = std::from_chars(hex.data(), hex.data() + hex.size(), n, 16);
  if (error != std::errc{} || end != hex.data() + hex.size()) return std::nullopt;
  auto channel = [](uint32_t value) { return static_cast<float>(value) / 255.0f; };
  switch (hex.size()) {
    case 3:
      return D2D1::ColorF(channel(((n >> 8) & 0xF) * 17), channel(((n >> 4) & 0xF) * 17),
                          channel((n & 0xF) * 17));
    case 6:
      return D2D1::ColorF(channel((n >> 16) & 0xFF), channel((n >> 8) & 0xFF), channel(n & 0xFF));
    default:
      return D2D1::ColorF(channel((n >> 24) & 0xFF), channel((n >> 16) & 0xFF),
                          channel((n >> 8) & 0xFF), channel(n & 0xFF));
  }
}

// `rgb(r, g, b)` / `rgba(r, g, b, a)` is what `whisker-css` emits for non-hex colours.
std::optional<D2D1_COLOR_F> ParseRgbFunction(std::string_view text) {
  bool rgba = text.starts_with("rgba(");
  if (!rgba && !text.starts_with("rgb(")) return std::nullopt;
  if (!text.ends_with(")")) return std::nullopt;
  text = text.substr(rgba ? 5 : 4, text.size() - (rgba ? 6 : 5));
  float parts[4] = {0, 0, 0, 1};
  size_t count = 0;
  while (!text.empty() && count < 4) {
    auto comma = text.find(',');
    auto part = text.substr(0, comma);
    while (!part.empty() && part.front() == ' ') part.remove_prefix(1);
    while (!part.empty() && part.back() == ' ') part.remove_suffix(1);
    auto [end, error] = std::from_chars(part.data(), part.data() + part.size(), parts[count]);
    if (error != std::errc{} || end != part.data() + part.size()) return std::nullopt;
    ++count;
    text = comma == std::string_view::npos ? std::string_view{} : text.substr(comma + 1);
  }
  if (count != (rgba ? 4u : 3u) || !text.empty()) return std::nullopt;
  return D2D1::ColorF(parts[0] / 255.0f, parts[1] / 255.0f, parts[2] / 255.0f, parts[3]);
}

D2D1_COLOR_F ParseColor(std::string_view text) {
  while (!text.empty() && text.front() == ' ') text.remove_prefix(1);
  while (!text.empty() && text.back() == ' ') text.remove_suffix(1);
  if (text.starts_with("#")) {
    if (auto color = ParseHex(text.substr(1))) return *color;
  }
  if (auto color = ParseRgbFunction(text)) return *color;
  if (text == "white") return D2D1::ColorF(1, 1, 1);
  if (text == "red") return D2D1::ColorF(1, 0, 0);
  if (text == "green") return D2D1::ColorF(0, 128.0f / 255, 0);
  if (text == "blue") return D2D1::ColorF(0, 0, 1);
  if (text == "transparent") return D2D1::ColorF(0, 0, 0, 0);
  return D2D1::ColorF(0, 0, 0);
}

class Reader {
 public:
  explicit Reader(const std::vector<uint8_t>& bytes) : bytes_(bytes) {}

  bool U8(uint8_t& value) {
    if (pos_ >= bytes_.size()) return false;
    value = bytes_[pos_++];
    return true;
  }
  bool F32(float& value) {
    if (pos_ + 4 > bytes_.size()) return false;
    std::memcpy(&value, bytes_.data() + pos_, 4);
    pos_ += 4;
    return true;
  }
  template <size_t N>
  bool F32s(float (&values)[N]) {
    for (auto& value : values) {
      if (!F32(value)) return false;
    }
    return true;
  }
  bool Color(D2D1_COLOR_F& color) {
    if (pos_ + 4 > bytes_.size()) return false;
    auto channel = [&](size_t i) { return bytes_[pos_ + i] / 255.0f; };
    color = D2D1::ColorF(channel(0), channel(1), channel(2), channel(3));
    pos_ += 4;
    return true;
  }
  void Skip(size_t count) { pos_ = count; }

 private:
  const std::vector<uint8_t>& bytes_;
  size_t pos_ = 0;
};

class Replayer {
 public:
  Replayer(ID2D1DeviceContext* context, D2D1_COLOR_F tint, float width, float height)
      : context_(context), tint_(tint), width_(width), height_(height) {
    context_->GetTransform(&state_.transform);
  }

  // Stops at the first malformed or unsupported opcode, keeping what was drawn.
  void Run(const std::vector<uint8_t>& bytes) {
    if (bytes.size() < kHeaderLen || !std::equal(kMagic, kMagic + 4, bytes.begin()) ||
        bytes[4] > kVersion || bytes[5] != 0) {
      return;
    }
    Reader reader(bytes);
    reader.Skip(kHeaderLen);
    uint8_t op = 0;
    while (reader.U8(op) && Step(op, reader)) {
    }
  }

 private:
  struct State {
    D2D1_MATRIX_3X2_F transform{};
    std::optional<D2D1_COLOR_F> fill = D2D1::ColorF(0, 0, 0);
    std::optional<D2D1_COLOR_F> stroke;
    bool fill_tint = false;
    bool stroke_tint = false;
    float stroke_width = 1;
    float opacity = 1;
  };

  bool Step(uint8_t op, Reader& reader) {
    float v[6];
    D2D1_COLOR_F color;
    switch (op) {
      case 0xFF:
        return false;
      case 0x01:
        saved_.push_back(state_);
        return true;
      case 0x02:
        if (!saved_.empty()) {
          state_ = saved_.back();
          saved_.pop_back();
        }
        return true;
      case 0x03:
        if (!reader.F32s(v)) return false;
        state_.transform = D2D1::Matrix3x2F(v[0], v[1], v[2], v[3], v[4], v[5]) *
                           *D2D1::Matrix3x2F::ReinterpretBaseType(&state_.transform);
        return true;
      case 0x04: {
        float r[4];
        if (!reader.F32s(r)) return false;
        Viewport(r[0], r[1], r[2], r[3]);
        return true;
      }
      case 0x10:
        if (!reader.Color(color)) return false;
        state_.fill = color;
        state_.fill_tint = false;
        return true;
      case 0x11:
        if (!reader.Color(color)) return false;
        state_.stroke = color;
        state_.stroke_tint = false;
        return true;
      case 0x12:
        return reader.F32(state_.stroke_width);
      case 0x13:
        return reader.F32(state_.opacity);
      case 0x14:
        state_.fill_tint = true;
        return true;
      case 0x15:
        state_.stroke_tint = true;
        return true;
      case 0x20:
        BeginPath();
        return true;
      case 0x21: {
        float p[2];
        if (!reader.F32s(p)) return false;
        MoveTo(p[0], p[1]);
        return true;
      }
      case 0x22: {
        float p[2];
        if (!reader.F32s(p)) return false;
        Sink()->AddLine({p[0], p[1]});
        return true;
      }
      case 0x23: {
        float p[4];
        if (!reader.F32s(p)) return false;
        Sink()->AddQuadraticBezier({{p[0], p[1]}, {p[2], p[3]}});
        return true;
      }
      case 0x24:
        if (!reader.F32s(v)) return false;
        Sink()->AddBezier({{v[0], v[1]}, {v[2], v[3]}, {v[4], v[5]}});
        return true;
      case 0x25:
        if (figure_open_) {
          sink_->EndFigure(D2D1_FIGURE_END_CLOSED);
          figure_open_ = false;
        }
        return true;
      case 0x30:
        Paint(true, false);
        return true;
      case 0x32:
        Paint(false, true);
        return true;
      case 0x33:
        Paint(true, true);
        return true;
      default:
        return false;
    }
  }

  // preserveAspectRatio="xMidYMid meet", per SPEC.md §"Viewport mapping".
  void Viewport(float min_x, float min_y, float width, float height) {
    if (width <= 0 || height <= 0) return;
    float scale = std::min(width_ / width, height_ / height);
    float tx = (width_ - width * scale) / 2 - min_x * scale;
    float ty = (height_ - height * scale) / 2 - min_y * scale;
    state_.transform = D2D1::Matrix3x2F(scale, 0, 0, scale, tx, ty) *
                       *D2D1::Matrix3x2F::ReinterpretBaseType(&state_.transform);
  }

  void BeginPath() {
    sink_.Reset();
    path_.Reset();
    figure_open_ = false;
    figure_start_ = {0, 0};
    ComPtr<ID2D1Factory> factory;
    context_->GetFactory(&factory);
    if (FAILED(factory->CreatePathGeometry(&path_))) return;
    path_->Open(&sink_);
    sink_->SetFillMode(D2D1_FILL_MODE_WINDING);
  }

  ID2D1GeometrySink* Sink() {
    if (!sink_) BeginPath();
    // Like CoreGraphics, a segment after CLOSE starts at the closed subpath's start point.
    if (!figure_open_) MoveTo(figure_start_.x, figure_start_.y);
    return sink_.Get();
  }

  void MoveTo(float x, float y) {
    if (!sink_) BeginPath();
    if (figure_open_) sink_->EndFigure(D2D1_FIGURE_END_OPEN);
    sink_->BeginFigure({x, y}, D2D1_FIGURE_BEGIN_FILLED);
    figure_start_ = {x, y};
    figure_open_ = true;
  }

  void Paint(bool fill, bool stroke) {
    if (sink_) {
      if (figure_open_) sink_->EndFigure(D2D1_FIGURE_END_OPEN);
      sink_->Close();
      sink_.Reset();
      figure_open_ = false;
    }
    if (!path_) return;
    context_->SetTransform(state_.transform);
    if (fill) {
      if (auto brush = Brush(state_.fill_tint ? tint_ : state_.fill)) {
        context_->FillGeometry(path_.Get(), brush.Get());
      }
    }
    if (stroke) {
      if (auto brush = Brush(state_.stroke_tint ? tint_ : state_.stroke)) {
        context_->DrawGeometry(path_.Get(), brush.Get(), state_.stroke_width);
      }
    }
  }

  ComPtr<ID2D1SolidColorBrush> Brush(std::optional<D2D1_COLOR_F> color) {
    ComPtr<ID2D1SolidColorBrush> brush;
    if (color) {
      context_->CreateSolidColorBrush(*color, D2D1::BrushProperties(state_.opacity), &brush);
    }
    return brush;
  }

  ID2D1DeviceContext* context_;
  D2D1_COLOR_F tint_;
  float width_;
  float height_;
  State state_;
  std::vector<State> saved_;
  ComPtr<ID2D1PathGeometry> path_;
  ComPtr<ID2D1GeometrySink> sink_;
  bool figure_open_ = false;
  D2D1_POINT_2F figure_start_{0, 0};
};

class SvgView : public whisker::ModuleView {
 public:
  using ModuleView::ModuleView;

  void SetDisplayList(std::string_view base64) { display_list_ = DecodeBase64(base64); }
  void SetColor(std::string_view css) { tint_ = ParseColor(css); }

  void Draw(ID2D1DeviceContext* context, float width, float height) override {
    if (display_list_.empty()) return;
    D2D1_MATRIX_3X2_F transform;
    context->GetTransform(&transform);
    Replayer(context, tint_, width, height).Run(display_list_);
    context->SetTransform(transform);
  }

 private:
  std::vector<uint8_t> display_list_;
  D2D1_COLOR_F tint_ = D2D1::ColorF(0, 0, 0);
};

class SvgModule : public whisker::Module {
 public:
  whisker::ModuleDefinition Definition() override {
    return whisker::ModuleDefinition().Name("Svg").View<SvgView>(
        "whisker-svg:Svg", [](auto& view) {
          view.Prop("display-list", [](SvgView& svg, const whisker::Value& value) {
            svg.SetDisplayList(value.AsString().value_or(""));
          });
          view.Prop("color", [](SvgView& svg, const whisker::Value& value) {
            svg.SetColor(value.AsString().value_or(""));
          });
        });
  }
};

}  // namespace

WHISKER_MODULE(SvgModule)
