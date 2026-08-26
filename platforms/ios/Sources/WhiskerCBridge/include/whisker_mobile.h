#ifndef WHISKER_MOBILE_H_
#define WHISKER_MOBILE_H_
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "whisker_bridge.h"

enum { WHISKER_MOBILE_ABI_MAJOR = 2, WHISKER_MOBILE_ABI_MINOR = 4 };
enum { WHISKER_APPLY_ACCEPTED = 0, WHISKER_APPLY_NEED_SNAPSHOT = 1, WHISKER_APPLY_REJECTED = 2 };
enum { WHISKER_FRAME_SNAPSHOT = 0, WHISKER_FRAME_DELTA = 1 };
enum {
  WHISKER_OP_CREATE = 1, WHISKER_OP_DELETE, WHISKER_OP_INSERT, WHISKER_OP_REMOVE,
  WHISKER_OP_MOVE, WHISKER_OP_LAYOUT, WHISKER_OP_PAINT, WHISKER_OP_CLIP,
  WHISKER_OP_TRANSFORM, WHISKER_OP_OPACITY, WHISKER_OP_VISIBILITY,
  WHISKER_OP_Z_ORDER, WHISKER_OP_TEXT, WHISKER_OP_PROPERTY,
  WHISKER_OP_CLEAR_PROPERTY, WHISKER_OP_EVENT_MASK, WHISKER_OP_HIT_TEST,
  WHISKER_OP_CAPTURE, WHISKER_OP_RELEASE_CAPTURE, WHISKER_OP_COMMAND,
  WHISKER_OP_BACKGROUND_LAYERS
};
enum {
  WHISKER_BACKGROUND_LINEAR = 0, WHISKER_BACKGROUND_RADIAL = 1,
  WHISKER_BACKGROUND_CONIC = 2
};
enum { WHISKER_BACKGROUND_SIZE_AUTO = 0, WHISKER_BACKGROUND_SIZE_EXPLICIT = 1 };
enum { WHISKER_BACKGROUND_REPEAT = 0, WHISKER_BACKGROUND_NO_REPEAT = 1 };
enum {
  WHISKER_BACKGROUND_BOX_BORDER = 0, WHISKER_BACKGROUND_BOX_PADDING = 1,
  WHISKER_BACKGROUND_BOX_CONTENT = 2
};
enum { WHISKER_BACKGROUND_ATTACHMENT_SCROLL = 0 };
enum { WHISKER_BACKGROUND_BLEND_NORMAL = 0 };
enum {
  WHISKER_MEASURE_TEXT = 1, WHISKER_MEASURE_REPLACED_CONTENT,
  WHISKER_MEASURE_NATIVE_CONTROL, WHISKER_MEASURE_EMBEDDED_SURFACE,
  WHISKER_MEASURE_CUSTOM
};
enum { WHISKER_MEASURE_READY = 1, WHISKER_MEASURE_PENDING, WHISKER_MEASURE_UNSUPPORTED };

typedef struct { float x, y, width, height; } WhiskerMobileRect;
typedef struct { WhiskerMobileRect border, content; } WhiskerMobileLayoutGeometry;
typedef struct {
  uint32_t kind; uint8_t red, green, blue, _pad; float alpha; WhiskerStringRef name;
} WhiskerMobileColor;
typedef struct { float length, fraction; } WhiskerMobileLengthPercentage;
typedef struct {
  WhiskerMobileColor background;
  WhiskerMobileLengthPercentage widths[4];
  WhiskerMobileColor colors[4];
  uint32_t styles[4];
  WhiskerMobileLengthPercentage radii_horizontal[4];
  WhiskerMobileLengthPercentage radii_vertical[4];
} WhiskerMobileBoxPaint;
typedef struct {
  WhiskerMobileColor color;
  WhiskerMobileLengthPercentage position;
} WhiskerMobileGradientStop;
typedef struct {
  WhiskerMobileLengthPercentage center_x, center_y, radius_x, radius_y;
  const WhiskerMobileGradientStop *stops;
  size_t stop_count;
} WhiskerMobileRadialGradient;
typedef struct {
  WhiskerMobileLengthPercentage center_x, center_y;
  const WhiskerMobileGradientStop *stops;
  size_t stop_count;
} WhiskerMobileConicGradient;
typedef struct {
  uint32_t kind; float scalar; const void *payload; size_t payload_count;
} WhiskerMobileBackgroundImage;
typedef struct {
  WhiskerMobileBackgroundImage image;
  WhiskerMobileLengthPercentage position_x, position_y;
  WhiskerMobileLengthPercentage size_width, size_height;
  uint32_t size_kind, repeat_x, repeat_y, origin, clip, attachment, blend_mode;
} WhiskerMobileBackgroundLayer;
typedef struct {
  WhiskerStringRef text; float font_size; uint16_t font_weight;
  uint8_t font_style, wrap; uint32_t max_lines; float line_height, letter_spacing;
  WhiskerMobileColor color; uint64_t prepared_content;
} WhiskerMobileText;
typedef struct {
  uint32_t tag, flags; uint64_t node, parent, child; uint32_t index, member;
  int32_t integer; float scalar; uint64_t wide; const void* payload; size_t payload_count;
} WhiskerMobileOperation;
typedef struct {
  uint16_t abi_major, abi_minor, protocol_major, protocol_minor; uint8_t mode, _pad[7];
  uint64_t surface; uint32_t scene_epoch, viewport_epoch;
  uint64_t frame_id, base_revision, target_revision;
  const WhiskerMobileOperation* operations; size_t operation_count;
} WhiskerMobileFrame;
typedef struct { uint8_t status, _pad[7]; uint64_t revision; } WhiskerMobileApplyResponse;
_Static_assert(sizeof(WhiskerMobileOperation) == 72, "WhiskerMobileOperation ABI drift");
_Static_assert(sizeof(WhiskerMobileFrame) == 72, "WhiskerMobileFrame ABI drift");
_Static_assert(sizeof(WhiskerMobileApplyResponse) == 16, "WhiskerMobileApplyResponse ABI drift");
_Static_assert(sizeof(WhiskerMobileGradientStop) == 40, "WhiskerMobileGradientStop ABI drift");
_Static_assert(sizeof(WhiskerMobileRadialGradient) == 48, "WhiskerMobileRadialGradient ABI drift");
_Static_assert(sizeof(WhiskerMobileConicGradient) == 32, "WhiskerMobileConicGradient ABI drift");
_Static_assert(sizeof(WhiskerMobileBackgroundImage) == 24, "WhiskerMobileBackgroundImage ABI drift");
_Static_assert(sizeof(WhiskerMobileBackgroundLayer) == 88, "WhiskerMobileBackgroundLayer ABI drift");
typedef struct {
  uint32_t id; uint8_t value_kind, optional_kind, _pad[2]; WhiskerStringRef name;
} WhiskerMobileMemberRegistration;
typedef struct {
  uint32_t element_type; uint8_t child_policy, measurement, _pad[2]; WhiskerStringRef name;
  const WhiskerMobileMemberRegistration* properties; size_t property_count;
  const WhiskerMobileMemberRegistration* events; size_t event_count;
  const WhiskerMobileMemberRegistration* commands; size_t command_count;
} WhiskerMobileElementRegistration;
typedef struct {
  uint16_t abi_major, abi_minor, protocol_major, protocol_minor;
  const WhiskerMobileElementRegistration* registrations; size_t registration_count;
} WhiskerMobileBootstrap;
typedef struct {
  uint64_t key, node; uint32_t element_type, kind; uint64_t environment_epoch;
  float known_width, known_height; uint32_t known_mask;
  float available_width, available_height;
  uint8_t available_width_kind, available_height_kind, font_style, wrap;
  WhiskerStringRef text, locale, font_family; float font_size;
  uint16_t font_weight, payload_version; float line_height, letter_spacing;
  uint32_t max_lines; WhiskerBytesRef payload;
  float intrinsic_width, intrinsic_height; uint32_t intrinsic_mask;
} WhiskerMobileMeasureRequest;
typedef struct {
  uint64_t key, environment_epoch; uint32_t status, reason;
  float width, height, first_baseline, last_baseline; uint32_t metrics_mask;
  uint64_t request_id, prepared_content;
} WhiskerMobileMeasureResponse;
_Static_assert(sizeof(WhiskerMobileMemberRegistration) == 24, "WhiskerMobileMemberRegistration ABI drift");
_Static_assert(sizeof(WhiskerMobileElementRegistration) == 72, "WhiskerMobileElementRegistration ABI drift");
_Static_assert(sizeof(WhiskerMobileBootstrap) == 24, "WhiskerMobileBootstrap ABI drift");
_Static_assert(sizeof(WhiskerMobileMeasureRequest) == 160, "WhiskerMobileMeasureRequest ABI drift");
_Static_assert(sizeof(WhiskerMobileMeasureResponse) == 64, "WhiskerMobileMeasureResponse ABI drift");
_Static_assert(sizeof(WhiskerMobileText) == 80, "WhiskerMobileText ABI drift");
_Static_assert(sizeof(WhiskerMobileBoxPaint) == 272, "WhiskerMobileBoxPaint ABI drift");
#endif
