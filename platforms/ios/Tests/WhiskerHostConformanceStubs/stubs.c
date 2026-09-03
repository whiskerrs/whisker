#include "../../Sources/WhiskerCBridge/include/whisker_mobile.h"

void whisker_host_conformance_stubs_link_anchor(void) {}

void *whisker_view_create(float width,
                          float height,
                          float scale,
                          const struct WhiskerMobileHostCapabilities *capabilities,
                          WhiskerMobileRequestFrameCallback request_frame,
                          void *request_frame_data,
                          WhiskerMobileBootstrapCallback bootstrap,
                          void *bootstrap_data,
                          WhiskerMobileMeasureCallback measure,
                          void *measure_data,
                          WhiskerMobilePresentFrameCallback present_frame,
                          void *present_frame_data,
                          WhiskerMobileResourceCommandCallback resource_command,
                          void *resource_data,
                          WhiskerMobileInvokeModuleCallback invoke_module,
                          WhiskerMobileObserveModuleCallback observe_module,
                          void *module_data) {
  (void)width;
  (void)height;
  (void)scale;
  (void)capabilities;
  (void)request_frame;
  (void)request_frame_data;
  (void)bootstrap;
  (void)bootstrap_data;
  (void)measure;
  (void)measure_data;
  (void)present_frame;
  (void)present_frame_data;
  (void)resource_command;
  (void)resource_data;
  (void)invoke_module;
  (void)observe_module;
  (void)module_data;
  return NULL;
}

bool whisker_view_tick(void *handle,
                       double timestamp_ms,
                       float width,
                       float height,
                       float scale) {
  (void)handle;
  (void)timestamp_ms;
  (void)width;
  (void)height;
  (void)scale;
  return true;
}

void whisker_view_destroy(void *handle) { (void)handle; }

bool whisker_view_dispatch_event(void *handle,
                                 double timestamp_ms,
                                 uint64_t node,
                                 const uint8_t *name,
                                 size_t name_len,
                                 const struct WhiskerValueRaw *detail) {
  (void)handle;
  (void)timestamp_ms;
  (void)node;
  (void)name;
  (void)name_len;
  (void)detail;
  return false;
}

bool whisker_view_dispatch_pointer(void *handle,
                                   double timestamp_ms,
                                   uint32_t event,
                                   uint64_t pointer_id,
                                   uint32_t pointer_kind,
                                   float x,
                                   float y,
                                   uint32_t buttons,
                                   int16_t changed_button) {
  (void)handle;
  (void)timestamp_ms;
  (void)event;
  (void)pointer_id;
  (void)pointer_kind;
  (void)x;
  (void)y;
  (void)buttons;
  (void)changed_button;
  return false;
}

bool whisker_view_dispatch_module_event(void *handle,
                                        const uint8_t *module,
                                        size_t module_len,
                                        const uint8_t *event,
                                        size_t event_len,
                                        const struct WhiskerValueRaw *payload) {
  (void)handle;
  (void)module;
  (void)module_len;
  (void)event;
  (void)event_len;
  (void)payload;
  return false;
}

bool whisker_view_dispatch_resource_event(
    void *handle,
    const struct WhiskerMobileResourceEvent *event) {
  (void)handle;
  (void)event;
  return true;
}
