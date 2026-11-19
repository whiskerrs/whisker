#pragma once

#include <d2d1_1.h>

#include <cstdint>
#include <functional>
#include <initializer_list>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <type_traits>
#include <unordered_map>
#include <utility>
#include <variant>
#include <vector>

namespace whisker {

// Owned form of the values exchanged with the Rust runtime.
class Value {
 public:
  using Bytes = std::vector<uint8_t>;
  using Array = std::vector<Value>;
  using Map = std::vector<std::pair<std::string, Value>>;
  struct Error {
    std::string message;
  };
  using Data = std::variant<std::monostate, bool, int64_t, double, std::string, Bytes, Array, Map,
                            Error>;

  Value() = default;
  Value(std::nullptr_t) {}
  Value(bool value) : data_(value) {}
  Value(int value) : data_(int64_t{value}) {}
  Value(int64_t value) : data_(value) {}
  Value(double value) : data_(value) {}
  Value(std::string value) : data_(std::move(value)) {}
  Value(std::string_view value) : data_(std::string(value)) {}
  Value(const char* value) : data_(std::string(value)) {}
  Value(Bytes value) : data_(std::move(value)) {}
  Value(Array value) : data_(std::move(value)) {}
  Value(Map value) : data_(std::move(value)) {}
  Value(Error value) : data_(std::move(value)) {}

  bool IsNull() const { return std::holds_alternative<std::monostate>(data_); }
  std::optional<bool> AsBool() const { return Get<bool>(); }
  std::optional<int64_t> AsInt() const { return Get<int64_t>(); }
  // Integers widen to doubles.
  std::optional<double> AsNumber() const {
    if (auto value = Get<int64_t>()) return static_cast<double>(*value);
    return Get<double>();
  }
  std::optional<std::string_view> AsString() const {
    auto value = std::get_if<std::string>(&data_);
    return value ? std::optional<std::string_view>(*value) : std::nullopt;
  }
  const Bytes* AsBytes() const { return std::get_if<Bytes>(&data_); }
  const Array* AsArray() const { return std::get_if<Array>(&data_); }
  const Map* AsMap() const { return std::get_if<Map>(&data_); }
  const Error* AsError() const { return std::get_if<Error>(&data_); }
  const Value* Get(std::string_view key) const {
    if (auto map = AsMap()) {
      for (auto const& [name, value] : *map) {
        if (name == key) return &value;
      }
    }
    return nullptr;
  }
  const Data& data() const { return data_; }

 private:
  template <typename T>
  std::optional<T> Get() const {
    auto value = std::get_if<T>(&data_);
    return value ? std::optional<T>(*value) : std::nullopt;
  }

  Data data_;
};

using Arguments = std::vector<Value>;

// Host services available to one mounted view.
class ViewContext {
 public:
  virtual void Emit(std::string_view event, Value detail) = 0;
  virtual void Invalidate() = 0;

 protected:
  ~ViewContext() = default;
};

// One mounted module view, painted into its node's content box.
class ModuleView {
 public:
  explicit ModuleView(ViewContext& context) : context_(&context) {}
  virtual ~ModuleView() = default;
  ModuleView(ModuleView const&) = delete;
  ModuleView& operator=(ModuleView const&) = delete;

  // User space is logical pixels with the content box at the origin; the Host has cleared it.
  virtual void Draw(ID2D1DeviceContext* context, float width, float height) = 0;

 protected:
  // Delivered on the next frame, so it is safe to call from a property setter.
  void Emit(std::string_view event, Value detail = {}) { context_->Emit(event, std::move(detail)); }
  // Schedules `Draw`; property and command handlers already imply it.
  void Invalidate() { context_->Invalidate(); }

 private:
  ViewContext* context_;
};

struct Size {
  float width = 0;
  float height = 0;
};

// Logical pixels; an unset available size means the axis is unconstrained.
struct MeasureRequest {
  std::optional<float> available_width;
  std::optional<float> available_height;
  std::optional<float> known_width;
  std::optional<float> known_height;
  uint16_t payload_version = 0;
  Value payload;
};

enum class TextAlignment { Start, Center, End, Justify };
enum class TextDirection { Ltr, Rtl };

// The inherited text style the Host resolves for a view element declared with `text_style`.
struct TextStyle {
  std::vector<std::string> font_families;
  float font_size = 14;
  uint16_t font_weight = 400;
  bool italic = false;
  std::optional<float> line_height;
  float letter_spacing = 0;
  D2D1_COLOR_F color{0, 0, 0, 1};
  TextAlignment alignment = TextAlignment::Start;
  TextDirection direction = TextDirection::Ltr;
};

class PromiseState {
 public:
  virtual ~PromiseState() = default;
  virtual void Settle(Value result) = 0;
};

// Completes one `AsyncFunction` call; only the first settlement counts, from any thread.
class Promise {
 public:
  explicit Promise(std::shared_ptr<PromiseState> state) : state_(std::move(state)) {}
  void Resolve(Value value = {}) const { state_->Settle(std::move(value)); }
  void Reject(std::string message) const { state_->Settle(Value::Error{std::move(message)}); }

 private:
  std::shared_ptr<PromiseState> state_;
};

struct ViewDefinition {
  using Handler = std::function<void(ModuleView&, const Value&)>;

  std::string name;
  std::function<std::unique_ptr<ModuleView>(ViewContext&)> create;
  std::unordered_map<std::string, Handler> properties;
  std::unordered_map<std::string, Handler> commands;
  std::vector<std::string> events;
  std::function<std::optional<Size>(const MeasureRequest&)> measure;
  std::function<void(ModuleView&, const TextStyle&)> text_style;
};

template <typename T>
class ViewBuilder {
 public:
  explicit ViewBuilder(ViewDefinition& definition) : definition_(definition) {}

  // A cleared property arrives as a null `Value`.
  ViewBuilder& Prop(std::string name, std::function<void(T&, const Value&)> set) {
    definition_.properties[std::move(name)] = Erase(std::move(set));
    return *this;
  }
  ViewBuilder& Command(std::string name, std::function<void(T&, const Value&)> run) {
    definition_.commands[std::move(name)] = Erase(std::move(run));
    return *this;
  }
  ViewBuilder& Events(std::initializer_list<std::string_view> names) {
    for (auto name : names) definition_.events.emplace_back(name);
    return *this;
  }
  // Runs on the UI thread during layout; `nullopt` leaves the element unmeasured.
  ViewBuilder& Measurement(std::function<std::optional<Size>(const MeasureRequest&)> measure) {
    definition_.measure = std::move(measure);
    return *this;
  }
  ViewBuilder& TextStyle(std::function<void(T&, const whisker::TextStyle&)> apply) {
    definition_.text_style = [apply = std::move(apply)](ModuleView& view,
                                                        const whisker::TextStyle& style) {
      apply(static_cast<T&>(view), style);
    };
    return *this;
  }

 private:
  static ViewDefinition::Handler Erase(std::function<void(T&, const Value&)> handler) {
    return [handler = std::move(handler)](ModuleView& view, const Value& value) {
      handler(static_cast<T&>(view), value);
    };
  }

  ViewDefinition& definition_;
};

// The Windows counterpart of the Swift and Kotlin `ModuleDefinition` DSL.
class ModuleDefinition {
 public:
  using SyncFunction = std::function<Value(const Arguments&)>;
  using AsyncFunctionHandler = std::function<void(const Arguments&, Promise)>;

  ModuleDefinition& Name(std::string name) {
    name_ = std::move(name);
    return *this;
  }
  ModuleDefinition& Function(std::string name, SyncFunction function) {
    functions_[std::move(name)] = std::move(function);
    return *this;
  }
  ModuleDefinition& AsyncFunction(std::string name, AsyncFunctionHandler function) {
    async_functions_[std::move(name)] = std::move(function);
    return *this;
  }
  ModuleDefinition& Events(std::initializer_list<std::string_view> names) {
    for (auto name : names) events_.emplace_back(name);
    return *this;
  }
  template <typename T, typename Configure>
  ModuleDefinition& View(std::string element, Configure configure) {
    static_assert(std::is_base_of_v<ModuleView, T>, "View types derive from whisker::ModuleView");
    ViewDefinition view;
    view.name = std::move(element);
    view.create = [](ViewContext& context) { return std::make_unique<T>(context); };
    ViewBuilder<T> builder(view);
    configure(builder);
    views_.push_back(std::move(view));
    return *this;
  }

  const std::string& name() const { return name_; }
  const std::unordered_map<std::string, SyncFunction>& functions() const { return functions_; }
  const std::unordered_map<std::string, AsyncFunctionHandler>& async_functions() const {
    return async_functions_;
  }
  const std::vector<std::string>& events() const { return events_; }
  const std::vector<ViewDefinition>& views() const { return views_; }

 private:
  std::string name_;
  std::unordered_map<std::string, SyncFunction> functions_;
  std::unordered_map<std::string, AsyncFunctionHandler> async_functions_;
  std::vector<std::string> events_;
  std::vector<ViewDefinition> views_;
};

class Module {
 public:
  virtual ~Module() = default;
  virtual ModuleDefinition Definition() = 0;

 protected:
  // Sends one of the module's `Events` to Rust observers; callable from any thread.
  void SendEvent(std::string_view event, Value payload = {});
};

void RegisterModule(std::unique_ptr<Module> module);

}  // namespace whisker

// Registers `Type` from a static initializer, so its object file must be linked directly.
#define WHISKER_MODULE(Type)                                         \
  namespace {                                                        \
  const bool whisker_module_registered_##Type =                      \
      (::whisker::RegisterModule(std::make_unique<Type>()), true);   \
  }
