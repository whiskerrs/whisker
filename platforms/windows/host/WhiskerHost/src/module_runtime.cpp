#include "module_runtime.h"

#include <windows.h>
#undef GetCurrentTime

#include <atomic>
#include <memory>
#include <mutex>
#include <utility>

#include <winrt/Microsoft.UI.Dispatching.h>

#include "value.h"

namespace whisker {

namespace {

struct Entry {
  std::unique_ptr<Module> module;
  std::optional<ModuleDefinition> definition;
};

// Function-local so registrations from other objects' static initializers find it constructed.
std::vector<Entry>& Entries() {
  static std::vector<Entry> entries;
  return entries;
}

std::vector<Entry>& Defined() {
  auto& entries = Entries();
  for (auto& entry : entries) {
    if (!entry.definition) entry.definition = entry.module->Definition();
  }
  return entries;
}

std::mutex sink_mutex;
ModuleEventSink* active_sink = nullptr;

class HostPromise final : public PromiseState {
 public:
  HostPromise(WhiskerMobileModuleResultCallback result, void* data)
      : result_(result),
        data_(data),
        queue_(winrt::Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()) {}

  ~HostPromise() override {
    if (!settled_.load()) Complete(Value::Error{"AsyncFunction finished without settling"});
  }

  void Settle(Value result) override {
    if (settled_.exchange(true)) return;
    Complete(std::move(result));
  }

 private:
  void Complete(Value result) {
    auto deliver = [result = std::move(result), callback = result_, data = data_]() {
      RawEncoder encoder;
      callback(data, encoder.Encode(result));
    };
    if (queue_.HasThreadAccess()) {
      deliver();
    } else {
      queue_.TryEnqueue(std::move(deliver));
    }
  }

  WhiskerMobileModuleResultCallback result_;
  void* data_;
  winrt::Microsoft::UI::Dispatching::DispatcherQueue queue_;
  std::atomic<bool> settled_ = false;
};

}  // namespace

void RegisterModule(std::unique_ptr<Module> module) {
  Entries().push_back({std::move(module), std::nullopt});
}

void Module::SendEvent(std::string_view event, Value payload) {
  std::string name;
  for (auto const& entry : Defined()) {
    if (entry.module.get() == this) name = entry.definition->name();
  }
  std::lock_guard lock(sink_mutex);
  if (active_sink) (*active_sink)(std::move(name), std::string(event), std::move(payload));
}

const ViewDefinition* FindModuleView(std::string_view element) {
  for (auto const& entry : Defined()) {
    for (auto const& view : entry.definition->views()) {
      if (view.name == element) return &view;
    }
  }
  return nullptr;
}

ModuleServices::ModuleServices(ModuleEventSink sink) {
  std::lock_guard lock(sink_mutex);
  active_sink = new ModuleEventSink(std::move(sink));
}

ModuleServices::~ModuleServices() {
  std::lock_guard lock(sink_mutex);
  delete std::exchange(active_sink, nullptr);
}

bool ModuleServices::Invoke(std::string_view module, std::string_view function,
                            const WhiskerValueRaw* args, size_t count, bool is_async,
                            WhiskerMobileModuleResultCallback result, void* result_data) {
  for (auto const& entry : Defined()) {
    auto const& definition = *entry.definition;
    if (definition.name() != module) continue;
    Arguments arguments;
    for (size_t i = 0; i < count; ++i) arguments.push_back(FromRaw(&args[i]));
    std::string key(function);
    if (is_async) {
      auto it = definition.async_functions().find(key);
      if (it == definition.async_functions().end()) return false;
      it->second(arguments, Promise(std::make_shared<HostPromise>(result, result_data)));
      return true;
    }
    auto it = definition.functions().find(key);
    if (it == definition.functions().end()) return false;
    auto value = it->second(arguments);
    RawEncoder encoder;
    result(result_data, encoder.Encode(value));
    return true;
  }
  return false;
}

Value FromRaw(const WhiskerValueRaw* value) {
  if (!value) return {};
  switch (value->type) {
    case WHISKER_VALUE_BOOL:
      return value->v.b;
    case WHISKER_VALUE_INT:
      return value->v.i;
    case WHISKER_VALUE_FLOAT:
      return value->v.f;
    case WHISKER_VALUE_STRING:
      return View(value->v.s);
    case WHISKER_VALUE_ERROR:
      return Value::Error{std::string(View(value->v.s))};
    case WHISKER_VALUE_BYTES: {
      auto const& bytes = value->v.bytes;
      return Value::Bytes(bytes.ptr, bytes.ptr + (bytes.ptr ? bytes.len : 0));
    }
    case WHISKER_VALUE_ARRAY: {
      Value::Array items;
      for (size_t i = 0; i < value->v.array.count; ++i) {
        items.push_back(FromRaw(&value->v.array.items[i]));
      }
      return items;
    }
    case WHISKER_VALUE_MAP: {
      Value::Map entries;
      for (size_t i = 0; i < value->v.map.count; ++i) {
        auto const& entry = value->v.map.entries[i];
        entries.emplace_back(std::string(View(entry.key)), FromRaw(&entry.value));
      }
      return entries;
    }
    default:
      return {};
  }
}

const WhiskerValueRaw* RawEncoder::Encode(const Value& value) {
  auto& raw = values_.emplace_back();
  Fill(value, raw);
  return &raw;
}

void RawEncoder::Fill(const Value& value, WhiskerValueRaw& raw) {
  raw = {};
  auto string = [](std::string const& text) { return WhiskerStringRef{text.data(), text.size()}; };
  std::visit(
      [&](auto const& data) {
        using T = std::decay_t<decltype(data)>;
        if constexpr (std::is_same_v<T, std::monostate>) {
          raw.type = WHISKER_VALUE_NULL;
        } else if constexpr (std::is_same_v<T, bool>) {
          raw.type = WHISKER_VALUE_BOOL;
          raw.v.b = data;
        } else if constexpr (std::is_same_v<T, int64_t>) {
          raw.type = WHISKER_VALUE_INT;
          raw.v.i = data;
        } else if constexpr (std::is_same_v<T, double>) {
          raw.type = WHISKER_VALUE_FLOAT;
          raw.v.f = data;
        } else if constexpr (std::is_same_v<T, std::string>) {
          raw.type = WHISKER_VALUE_STRING;
          raw.v.s = string(data);
        } else if constexpr (std::is_same_v<T, Value::Error>) {
          raw.type = WHISKER_VALUE_ERROR;
          raw.v.s = string(data.message);
        } else if constexpr (std::is_same_v<T, Value::Bytes>) {
          raw.type = WHISKER_VALUE_BYTES;
          raw.v.bytes = {data.data(), data.size()};
        } else if constexpr (std::is_same_v<T, Value::Array>) {
          auto& items = arrays_.emplace_back(data.size());
          for (size_t i = 0; i < data.size(); ++i) Fill(data[i], items[i]);
          raw.type = WHISKER_VALUE_ARRAY;
          raw.v.array = {items.data(), items.size()};
        } else if constexpr (std::is_same_v<T, Value::Map>) {
          auto& entries = maps_.emplace_back(data.size());
          for (size_t i = 0; i < data.size(); ++i) {
            entries[i].key = string(data[i].first);
            Fill(data[i].second, entries[i].value);
          }
          raw.type = WHISKER_VALUE_MAP;
          raw.v.map = {entries.data(), entries.size()};
        }
      },
      value.data());
}

}  // namespace whisker
