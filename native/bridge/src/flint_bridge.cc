// flint_bridge.cc
//
// Stub implementation. Will be replaced with actual Lynx Element PAPI calls
// once Lynx prebuilt is integrated. See native/bridge/include/flint_bridge.h
// for the public ABI contract.

#include "flint_bridge.h"

extern "C" {

FlintEngine* flint_bridge_engine_attach(void* /*lynx_shell_ptr*/) {
    return nullptr;
}

void flint_bridge_engine_detach(FlintEngine* /*engine*/) {}

FlintElement* flint_bridge_create_page(FlintEngine* /*engine*/) { return nullptr; }
FlintElement* flint_bridge_create_view(FlintEngine* /*engine*/) { return nullptr; }
FlintElement* flint_bridge_create_text(FlintEngine* /*engine*/) { return nullptr; }

void flint_bridge_append(FlintEngine* /*engine*/,
                         FlintElement* /*parent*/,
                         FlintElement* /*child*/) {}

void flint_bridge_remove(FlintEngine* /*engine*/,
                         FlintElement* /*parent*/,
                         FlintElement* /*child*/) {}

void flint_bridge_set_attribute(FlintEngine* /*engine*/,
                                FlintElement* /*elem*/,
                                const char* /*key*/,
                                const uint8_t* /*value_msgpack*/,
                                size_t /*value_len*/) {}

void flint_bridge_flush(FlintEngine* /*engine*/, FlintElement* /*root*/) {}

void flint_bridge_release_element(FlintElement* /*elem*/) {}

}  // extern "C"
