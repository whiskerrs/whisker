// lyra_bridge.cc
//
// Stub implementation. Will be replaced with actual Lynx Element PAPI calls
// once Lynx prebuilt is integrated. See native/bridge/include/lyra_bridge.h
// for the public ABI contract.

#include "lyra_bridge.h"

extern "C" {

LyraEngine* lyra_bridge_engine_attach(void* /*lynx_shell_ptr*/) {
    return nullptr;
}

void lyra_bridge_engine_detach(LyraEngine* /*engine*/) {}

LyraElement* lyra_bridge_create_page(LyraEngine* /*engine*/) { return nullptr; }
LyraElement* lyra_bridge_create_view(LyraEngine* /*engine*/) { return nullptr; }
LyraElement* lyra_bridge_create_text(LyraEngine* /*engine*/) { return nullptr; }

void lyra_bridge_append(LyraEngine* /*engine*/,
                         LyraElement* /*parent*/,
                         LyraElement* /*child*/) {}

void lyra_bridge_remove(LyraEngine* /*engine*/,
                         LyraElement* /*parent*/,
                         LyraElement* /*child*/) {}

void lyra_bridge_set_attribute(LyraEngine* /*engine*/,
                                LyraElement* /*elem*/,
                                const char* /*key*/,
                                const uint8_t* /*value_msgpack*/,
                                size_t /*value_len*/) {}

void lyra_bridge_flush(LyraEngine* /*engine*/, LyraElement* /*root*/) {}

void lyra_bridge_release_element(LyraElement* /*elem*/) {}

}  // extern "C"
