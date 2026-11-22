#pragma once

#include <deque>
#include <functional>
#include <string>
#include <string_view>
#include <vector>

#include "whisker/module.h"
#include "whisker_mobile.h"

namespace whisker {

Value FromRaw(const WhiskerValueRaw* value);

// Borrows strings from the encoded `Value`, which must outlive the raw tree.
class RawEncoder {
 public:
  const WhiskerValueRaw* Encode(const Value& value);

 private:
  void Fill(const Value& value, WhiskerValueRaw& raw);

  std::deque<WhiskerValueRaw> values_;
  std::deque<std::vector<WhiskerValueRaw>> arrays_;
  std::deque<std::vector<WhiskerKeyValueRaw>> maps_;
};

const ViewDefinition* FindModuleView(std::string_view element);

using ModuleEventSink = std::function<void(std::string module, std::string event, Value payload)>;

// Answers the driver's module callbacks from the registered C++ modules.
class ModuleServices {
 public:
  explicit ModuleServices(ModuleEventSink sink);
  ~ModuleServices();
  ModuleServices(ModuleServices const&) = delete;
  ModuleServices& operator=(ModuleServices const&) = delete;

  bool Invoke(std::string_view module, std::string_view function, const WhiskerValueRaw* args,
              size_t count, bool is_async, WhiskerMobileModuleResultCallback result,
              void* result_data);
};

}  // namespace whisker
