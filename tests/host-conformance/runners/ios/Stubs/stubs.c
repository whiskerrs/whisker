#include <stdbool.h>
#include <stddef.h>

void whisker_host_conformance_stubs_link_anchor(void) {}

void *whisker_view_create(void) { return NULL; }
bool whisker_view_tick(void) { return true; }
void whisker_view_destroy(void *handle) { (void)handle; }
bool whisker_view_dispatch_event(void) { return false; }
bool whisker_view_dispatch_module_event(void) { return false; }
bool whisker_view_dispatch_resource_event(void) { return true; }
