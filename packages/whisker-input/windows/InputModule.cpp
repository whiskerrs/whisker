// `whisker-input` ModuleDefinition (Windows): a WinUI TextBox, or a PasswordBox while `secure`.

#include <dwrite_1.h>
#include <whisker/xaml_module.h>
#include <winrt/Microsoft.UI.Content.h>
#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Text.h>
#include <winrt/Microsoft.UI.Xaml.Controls.h>
#include <winrt/Microsoft.UI.Xaml.Input.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.System.h>
#include <winrt/Windows.UI.Text.h>

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>
#include <string_view>

namespace {

namespace xaml = winrt::Microsoft::UI::Xaml;
namespace controls = xaml::Controls;

std::wstring Widen(std::string_view text) {
  if (text.empty()) return {};
  int length =
      MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0);
  std::wstring wide(length, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), wide.data(), length);
  return wide;
}

std::string Narrow(std::wstring_view text) {
  if (text.empty()) return {};
  int length = WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr,
                                   0, nullptr, nullptr);
  std::string narrow(length, '\0');
  WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), narrow.data(), length,
                      nullptr, nullptr);
  return narrow;
}

std::wstring ResolveFamily(std::string_view family) {
  if (family.empty() || family == "system" || family == "system-ui" || family == "sans-serif") {
    return L"Segoe UI";
  }
  if (family == "serif") return L"Times New Roman";
  if (family == "monospace") return L"Consolas";
  return Widen(family);
}

// `#RGB`, `#RRGGBB`, `#RRGGBBAA`, `rgb()` and `rgba()`.
std::optional<winrt::Windows::UI::Color> ParseColor(std::string_view text) {
  auto byte = [](double value) {
    return static_cast<uint8_t>(std::clamp(std::lround(value), 0L, 255L));
  };
  if (text.starts_with('#')) {
    auto hex = text.substr(1);
    uint32_t value = 0;
    for (char c : hex) {
      int digit = c >= '0' && c <= '9'   ? c - '0'
                  : c >= 'a' && c <= 'f' ? c - 'a' + 10
                  : c >= 'A' && c <= 'F' ? c - 'A' + 10
                                         : -1;
      if (digit < 0) return std::nullopt;
      value = value << 4 | static_cast<uint32_t>(digit);
    }
    auto channel = [&](int shift) { return static_cast<uint8_t>(value >> shift & 0xFF); };
    switch (hex.size()) {
      case 3: {
        auto nibble = [&](int shift) { return static_cast<uint8_t>((value >> shift & 0xF) * 17); };
        return winrt::Windows::UI::Color{255, nibble(8), nibble(4), nibble(0)};
      }
      case 6:
        return winrt::Windows::UI::Color{255, channel(16), channel(8), channel(0)};
      case 8:
        return winrt::Windows::UI::Color{channel(0), channel(24), channel(16), channel(8)};
      default:
        return std::nullopt;
    }
  }
  if (text.starts_with("rgb")) {
    auto open = text.find('(');
    auto close = text.rfind(')');
    if (open == std::string_view::npos || close == std::string_view::npos || close < open) {
      return std::nullopt;
    }
    std::string inner(text.substr(open + 1, close - open - 1));
    double parts[4] = {0, 0, 0, 1};
    int count = 0;
    size_t start = 0;
    while (count < 4 && start <= inner.size()) {
      size_t end = inner.find(',', start);
      parts[count++] = std::atof(inner.substr(start, end - start).c_str());
      if (end == std::string::npos) break;
      start = end + 1;
    }
    if (count < 3) return std::nullopt;
    return winrt::Windows::UI::Color{byte(parts[3] * 255), byte(parts[0]), byte(parts[1]),
                                     byte(parts[2])};
  }
  return std::nullopt;
}

std::string StringOf(const whisker::Value& value) {
  auto text = value.AsString();
  return text ? std::string(*text) : std::string();
}

bool BoolOf(const whisker::Value& value, bool fallback) {
  return value.AsBool().value_or(fallback);
}

xaml::FrameworkElement FindNamed(xaml::DependencyObject const& parent, std::wstring_view name) {
  int32_t count = xaml::Media::VisualTreeHelper::GetChildrenCount(parent);
  for (int32_t i = 0; i < count; ++i) {
    auto child = xaml::Media::VisualTreeHelper::GetChild(parent, i);
    if (auto element = child.try_as<xaml::FrameworkElement>(); element && element.Name() == name) {
      return element;
    }
    if (auto found = FindNamed(child, name)) return found;
  }
  return nullptr;
}

whisker::Value Detail(std::string value) {
  return whisker::Value::Map{{"value", whisker::Value(std::move(value))}};
}

winrt::com_ptr<IDWriteFactory1> WriteFactory() {
  static winrt::com_ptr<IDWriteFactory1> factory = [] {
    winrt::com_ptr<IDWriteFactory1> created;
    winrt::check_hresult(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory1),
                                             reinterpret_cast<IUnknown**>(created.put())));
    return created;
  }();
  return factory;
}

std::optional<whisker::Size> Measure(const whisker::MeasureRequest& request) {
  if (request.payload_version != 1) return std::nullopt;
  auto const& payload = request.payload;
  auto field = [&](std::string_view name) { return payload.Get(name); };
  auto number = [&](std::string_view name) -> std::optional<double> {
    auto value = field(name);
    return value ? value->AsNumber() : std::nullopt;
  };
  auto text_value = field("text");
  auto font_size = number("font_size");
  if (!text_value || !text_value->AsString() || !font_size) return std::nullopt;
  bool multiline = field("multiline") && BoolOf(*field("multiline"), false);
  bool italic = field("italic") && BoolOf(*field("italic"), false);
  auto text = Widen(*text_value->AsString());
  if (!multiline) std::replace(text.begin(), text.end(), L'\n', L' ');
  auto family = field("font_family") ? ResolveFamily(StringOf(*field("font_family"))) : L"Segoe UI";
  auto weight =
      static_cast<DWRITE_FONT_WEIGHT>(std::clamp(number("font_weight").value_or(400), 1.0, 999.0));
  auto line_height = number("line_height");
  float spacing = static_cast<float>(number("letter_spacing").value_or(0));

  auto factory = WriteFactory();
  winrt::com_ptr<IDWriteTextFormat> format;
  winrt::check_hresult(factory->CreateTextFormat(
      family.c_str(), nullptr, weight, italic ? DWRITE_FONT_STYLE_ITALIC : DWRITE_FONT_STYLE_NORMAL,
      DWRITE_FONT_STRETCH_NORMAL, static_cast<float>(*font_size), L"", format.put()));
  if (line_height && *line_height > 0) {
    format->SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, static_cast<float>(*line_height),
                           static_cast<float>(*line_height * 0.8));
  }
  auto layout_at = [&](float width) {
    winrt::com_ptr<IDWriteTextLayout> layout;
    winrt::check_hresult(factory->CreateTextLayout(text.c_str(), static_cast<UINT32>(text.size()),
                                                   format.get(), width, 1.0e6f, layout.put()));
    layout->SetWordWrapping(multiline ? DWRITE_WORD_WRAPPING_WRAP : DWRITE_WORD_WRAPPING_NO_WRAP);
    if (spacing != 0 && !text.empty()) {
      if (auto spaced = layout.try_as<IDWriteTextLayout1>()) {
        spaced->SetCharacterSpacing(0, spacing, 0, {0, static_cast<UINT32>(text.size())});
      }
    }
    DWRITE_TEXT_METRICS metrics{};
    layout->GetMetrics(&metrics);
    return metrics;
  };
  float natural = layout_at(1.0e6f).widthIncludingTrailingWhitespace;
  float available = request.available_width ? std::min(natural, *request.available_width) : natural;
  float width = std::max(1.0f, request.known_width.value_or(available));
  auto metrics = layout_at(width);
  float height = line_height && *line_height > 0
                     ? static_cast<float>(*line_height) * std::max<UINT32>(metrics.lineCount, 1)
                     : metrics.height;
  return whisker::Size{
      request.known_width.value_or(std::ceil(metrics.widthIncludingTrailingWhitespace)),
      request.known_height.value_or(std::ceil(height))};
}

class InputView final : public whisker::XamlModuleView {
 public:
  explicit InputView(whisker::ViewContext& context) : XamlModuleView(context) {
    root_.Background(nullptr);
    root_.Loaded([this](auto&&, auto&&) {
      loaded_ = true;
      if (auto_focus_ || focus_pending_) Focus();
    });
    Rebuild();
  }

  xaml::UIElement Element() override { return root_; }

  void SetValue(std::string value) {
    value_ = std::move(value);
    Write(value_);
  }
  void SetPlaceholder(std::string text) {
    placeholder_ = Widen(text);
    if (box_) box_.PlaceholderText(placeholder_);
    if (password_) password_.PlaceholderText(placeholder_);
  }
  void SetPlaceholderColor(std::string_view text) {
    placeholder_brush_.Color(
        ParseColor(text).value_or(winrt::Windows::UI::Color{255, 153, 153, 153}));
  }
  void SetSelectionColor(std::string_view text) {
    selection_ = ParseColor(text);
    ApplySelection();
  }
  void SetMultiline(bool multiline) {
    if (multiline == multiline_) return;
    multiline_ = multiline;
    Rebuild();
  }
  void SetSecure(bool secure) {
    if (secure == secure_) return;
    secure_ = secure;
    Rebuild();
  }
  void SetEditable(bool editable) {
    editable_ = editable;
    if (box_) box_.IsReadOnly(!editable);
  }
  void SetAutoFocus(bool auto_focus) {
    auto_focus_ = auto_focus;
    if (auto_focus && loaded_) Focus();
  }
  void SetMaxLength(int64_t length) {
    max_length_ = static_cast<int32_t>(std::clamp<int64_t>(length, 0, INT32_MAX));
    if (box_) box_.MaxLength(max_length_);
    if (password_) password_.MaxLength(max_length_);
  }
  void SetKeyboardType(std::string type) {
    keyboard_type_ = std::move(type);
    ApplyInputScope();
  }
  void SetAutocorrect(bool enabled) {
    autocorrect_ = enabled;
    if (box_) box_.IsTextPredictionEnabled(enabled);
  }
  void SetSpellCheck(bool enabled) {
    spell_check_ = enabled;
    if (box_) box_.IsSpellCheckEnabled(enabled);
  }
  void ApplyTextStyle(const whisker::TextStyle& style) {
    style_ = style;
    foreground_brush_.Color({static_cast<uint8_t>(std::lround(style.color.a * 255)),
                             static_cast<uint8_t>(std::lround(style.color.r * 255)),
                             static_cast<uint8_t>(std::lround(style.color.g * 255)),
                             static_cast<uint8_t>(std::lround(style.color.b * 255))});
    ApplyFont(current());
  }

  void Focus() {
    if (!loaded_) {
      focus_pending_ = true;
      return;
    }
    focus_pending_ = false;
    // XAML focus alone stays inside this island while the Host's island keeps the keyboard.
    if (auto root = root_.XamlRoot()) {
      winrt::Microsoft::UI::Input::InputFocusController::GetForIsland(root.ContentIsland())
          .TrySetFocus();
    }
    current().Focus(xaml::FocusState::Programmatic);
    if (box_) box_.Select(static_cast<int32_t>(box_.Text().size()), 0);
  }
  void Blur() {
    focus_pending_ = false;
    auto root = root_.XamlRoot();
    if (!focused_ || !root) return;
    // Focus leaves this view's island for the Host's, which has no focusable element to take it.
    auto island = root.ContentIsland();
    winrt::Microsoft::UI::Input::InputFocusController::GetForIsland(island).DepartFocus(
        winrt::Microsoft::UI::Input::FocusNavigationRequest::Create(
            winrt::Microsoft::UI::Input::FocusNavigationReason::Programmatic));
  }
  void Clear() {
    value_.clear();
    Write(value_);
    Emit("input", Detail(""));
  }

 private:
  xaml::Controls::Control current() const {
    return box_ ? xaml::Controls::Control(box_) : xaml::Controls::Control(password_);
  }

  std::string Read() const { return box_ ? Narrow(box_.Text()) : Narrow(password_.Password()); }

  // Leaves the control untouched when the value already matches, so the caret stays put.
  void Write(const std::string& value) {
    if (Read() == value) return;
    if (box_) box_.Text(Widen(value));
    if (password_) password_.Password(Widen(value));
  }

  void Rebuild() {
    bool had_focus = focused_;
    root_.Children().Clear();
    box_ = nullptr;
    password_ = nullptr;
    focused_ = false;
    xaml::Controls::Control control{nullptr};
    if (secure_ && !multiline_) {
      password_ = xaml::Controls::PasswordBox();
      password_.IsPasswordRevealButtonEnabled(false);
      password_.PlaceholderText(placeholder_);
      password_.MaxLength(max_length_);
      password_.Password(Widen(value_));
      password_.PasswordChanged([this](auto&&, auto&&) { Edited(); });
      control = password_;
    } else {
      box_ = xaml::Controls::TextBox();
      box_.PlaceholderText(placeholder_);
      box_.PlaceholderForeground(placeholder_brush_);
      box_.MaxLength(max_length_);
      box_.IsReadOnly(!editable_);
      box_.IsTextPredictionEnabled(autocorrect_);
      box_.IsSpellCheckEnabled(spell_check_);
      box_.AcceptsReturn(multiline_);
      box_.TextWrapping(multiline_ ? xaml::TextWrapping::Wrap : xaml::TextWrapping::NoWrap);
      box_.Text(Widen(value_));
      box_.TextChanged([this](auto&&, auto&&) { Edited(); });
      // The template's clear button would overlap the node's own padding and has no opt-out
      // property.
      box_.Loaded([box = box_](auto&&, auto&&) {
        if (auto clear = FindNamed(box, L"DeleteButton")) clear.MaxWidth(0);
      });
      control = box_;
    }
    Strip(control);
    control.VerticalAlignment(multiline_ ? xaml::VerticalAlignment::Stretch
                                         : xaml::VerticalAlignment::Center);
    control.GotFocus([this](auto&&, auto&&) {
      // GotFocus also bubbles up from the template's own focusable parts.
      if (focused_) return;
      focused_ = true;
      Emit("focus");
    });
    control.LostFocus([this](auto&&, auto&&) {
      if (!focused_) return;
      focused_ = false;
      Emit("blur");
      Emit("change", Detail(Read()));
    });
    control.PreviewKeyDown([this](auto&&, xaml::Input::KeyRoutedEventArgs const& e) {
      if (e.Key() != winrt::Windows::System::VirtualKey::Enter) return;
      bool control_held = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
      if (multiline_ && !control_held) return;
      e.Handled(true);
      auto value = Read();
      Emit("submit", Detail(value));
      Emit("change", Detail(value));
    });
    ApplyFont(control);
    ApplySelection();
    ApplyInputScope();
    root_.Children().Append(control);
    if (had_focus) Focus();
  }

  // Strips the control's own chrome so the node's box paints the field.
  void Strip(xaml::Controls::Control const& control) {
    auto resources = control.Resources();
    auto set = [&](wchar_t const* key, winrt::Windows::Foundation::IInspectable const& value) {
      resources.Insert(winrt::box_value(key), value);
    };
    xaml::Media::SolidColorBrush transparent(winrt::Windows::UI::Color{0, 0, 0, 0});
    for (auto key : {L"TextControlBackground", L"TextControlBackgroundPointerOver",
                     L"TextControlBackgroundFocused", L"TextControlBackgroundDisabled",
                     L"TextControlBorderBrush", L"TextControlBorderBrushPointerOver",
                     L"TextControlBorderBrushFocused", L"TextControlBorderBrushDisabled"}) {
      set(key, transparent);
    }
    for (auto key : {L"TextControlForeground", L"TextControlForegroundPointerOver",
                     L"TextControlForegroundFocused"}) {
      set(key, foreground_brush_);
    }
    for (auto key :
         {L"TextControlPlaceholderForeground", L"TextControlPlaceholderForegroundPointerOver",
          L"TextControlPlaceholderForegroundFocused"}) {
      set(key, placeholder_brush_);
    }
    auto none = winrt::box_value(xaml::Thickness{0, 0, 0, 0});
    set(L"TextControlBorderThemeThickness", none);
    set(L"TextControlBorderThemeThicknessFocused", none);
    set(L"TextControlThemePadding", none);
    set(L"TextControlThemeMinHeight", winrt::box_value(0.0));
    set(L"TextControlThemeMinWidth", winrt::box_value(0.0));
    control.Padding({0, 0, 0, 0});
    control.BorderThickness({0, 0, 0, 0});
    control.MinHeight(0);
    control.MinWidth(0);
    control.Background(transparent);
    control.Foreground(foreground_brush_);
  }

  void ApplyFont(xaml::Controls::Control const& control) {
    if (!control) return;
    std::wstring family = L"Segoe UI";
    if (!style_.font_families.empty()) family = ResolveFamily(style_.font_families.front());
    control.FontFamily(xaml::Media::FontFamily(family));
    control.FontSize(style_.font_size);
    control.FontWeight(winrt::Windows::UI::Text::FontWeight{style_.font_weight});
    control.FontStyle(style_.italic ? winrt::Windows::UI::Text::FontStyle::Italic
                                    : winrt::Windows::UI::Text::FontStyle::Normal);
    control.CharacterSpacing(
        style_.font_size > 0
            ? static_cast<int32_t>(std::lround(style_.letter_spacing / style_.font_size * 1000))
            : 0);
    control.FlowDirection(style_.direction == whisker::TextDirection::Rtl
                              ? xaml::FlowDirection::RightToLeft
                              : xaml::FlowDirection::LeftToRight);
    if (box_) {
      switch (style_.alignment) {
        case whisker::TextAlignment::Center:
          box_.TextAlignment(xaml::TextAlignment::Center);
          break;
        case whisker::TextAlignment::End:
          box_.TextAlignment(xaml::TextAlignment::End);
          break;
        case whisker::TextAlignment::Justify:
          box_.TextAlignment(xaml::TextAlignment::Justify);
          break;
        default:
          box_.TextAlignment(xaml::TextAlignment::Start);
          break;
      }
    }
  }

  void ApplySelection() {
    xaml::Media::SolidColorBrush brush{nullptr};
    if (selection_) brush = xaml::Media::SolidColorBrush(*selection_);
    if (box_ && brush) box_.SelectionHighlightColor(brush);
    if (password_ && brush) password_.SelectionHighlightColor(brush);
  }

  void ApplyInputScope() {
    using winrt::Microsoft::UI::Xaml::Input::InputScopeNameValue;
    bool numeric =
        keyboard_type_ == "number" || keyboard_type_ == "numeric" || keyboard_type_ == "decimal";
    if (password_) {
      // PasswordBox rejects every scope except these two.
      xaml::Input::InputScope scope;
      scope.Names().Append(xaml::Input::InputScopeName(numeric ? InputScopeNameValue::NumericPin
                                                               : InputScopeNameValue::Password));
      password_.InputScope(scope);
      return;
    }
    auto value = InputScopeNameValue::Default;
    if (numeric) {
      value = InputScopeNameValue::Number;
    } else if (keyboard_type_ == "phone") {
      value = InputScopeNameValue::TelephoneNumber;
    } else if (keyboard_type_ == "email") {
      value = InputScopeNameValue::EmailSmtpAddress;
    } else if (keyboard_type_ == "url") {
      value = InputScopeNameValue::Url;
    }
    xaml::Input::InputScope scope;
    scope.Names().Append(xaml::Input::InputScopeName(value));
    if (box_) box_.InputScope(scope);
  }

  void Edited() {
    auto value = Read();
    if (value == value_) return;
    value_ = std::move(value);
    Emit("input", Detail(value_));
  }

  xaml::Controls::Grid root_;
  xaml::Controls::TextBox box_{nullptr};
  xaml::Controls::PasswordBox password_{nullptr};
  xaml::Media::SolidColorBrush foreground_brush_{winrt::Windows::UI::Color{255, 0, 0, 0}};
  xaml::Media::SolidColorBrush placeholder_brush_{winrt::Windows::UI::Color{255, 153, 153, 153}};
  std::optional<winrt::Windows::UI::Color> selection_;
  whisker::TextStyle style_;
  std::string value_;
  std::wstring placeholder_;
  std::string keyboard_type_;
  int32_t max_length_ = 0;
  bool multiline_ = false;
  bool secure_ = false;
  bool editable_ = true;
  bool auto_focus_ = false;
  bool autocorrect_ = true;
  bool spell_check_ = true;
  bool loaded_ = false;
  bool focus_pending_ = false;
  bool focused_ = false;
};

class InputModule final : public whisker::Module {
 public:
  whisker::ModuleDefinition Definition() override {
    return whisker::ModuleDefinition().Name("Input").View<InputView>(
        "whisker-input:Input", [](whisker::ViewBuilder<InputView>& view) {
          view.Prop("value", [](InputView& input, const whisker::Value& value) {
            input.SetValue(StringOf(value));
          });
          view.Prop("placeholder", [](InputView& input, const whisker::Value& value) {
            input.SetPlaceholder(StringOf(value));
          });
          view.Prop("placeholder-color", [](InputView& input, const whisker::Value& value) {
            input.SetPlaceholderColor(StringOf(value));
          });
          view.Prop("caret-color", [](InputView&, const whisker::Value&) {});
          view.Prop("selection-color", [](InputView& input, const whisker::Value& value) {
            input.SetSelectionColor(StringOf(value));
          });
          view.Prop("multiline", [](InputView& input, const whisker::Value& value) {
            input.SetMultiline(BoolOf(value, false));
          });
          view.Prop("lines", [](InputView&, const whisker::Value&) {});
          view.Prop("secure", [](InputView& input, const whisker::Value& value) {
            input.SetSecure(BoolOf(value, false));
          });
          view.Prop("editable", [](InputView& input, const whisker::Value& value) {
            input.SetEditable(BoolOf(value, true));
          });
          view.Prop("auto-focus", [](InputView& input, const whisker::Value& value) {
            input.SetAutoFocus(BoolOf(value, false));
          });
          view.Prop("max-length", [](InputView& input, const whisker::Value& value) {
            input.SetMaxLength(value.AsInt().value_or(0));
          });
          view.Prop("keyboard-type", [](InputView& input, const whisker::Value& value) {
            input.SetKeyboardType(StringOf(value));
          });
          view.Prop("return-key", [](InputView&, const whisker::Value&) {});
          view.Prop("auto-capitalize", [](InputView&, const whisker::Value&) {});
          view.Prop("auto-size", [](InputView&, const whisker::Value&) {});
          view.Prop("autocorrect", [](InputView& input, const whisker::Value& value) {
            input.SetAutocorrect(BoolOf(value, true));
          });
          view.Prop("spell-check", [](InputView& input, const whisker::Value& value) {
            input.SetSpellCheck(BoolOf(value, true));
          });
          view.Measurement(Measure);
          view.TextStyle([](InputView& input, const whisker::TextStyle& style) {
            input.ApplyTextStyle(style);
          });
          view.Events({"input", "change", "focus", "blur", "submit"});
          view.Command("focus", [](InputView& input, const whisker::Value&) { input.Focus(); });
          view.Command("blur", [](InputView& input, const whisker::Value&) { input.Blur(); });
          view.Command("clear", [](InputView& input, const whisker::Value&) { input.Clear(); });
          view.Command("setValue", [](InputView& input, const whisker::Value& value) {
            if (auto text = value.Get("value"); text && text->AsString()) {
              input.SetValue(std::string(*text->AsString()));
            }
          });
        });
  }
};

}  // namespace

WHISKER_MODULE(InputModule)
