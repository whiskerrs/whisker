#ifndef WHISKER_VALUE_H_
#define WHISKER_VALUE_H_

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef enum {
  WHISKER_VALUE_NULL = 0, WHISKER_VALUE_BOOL, WHISKER_VALUE_INT,
  WHISKER_VALUE_FLOAT, WHISKER_VALUE_STRING, WHISKER_VALUE_BYTES,
  WHISKER_VALUE_ARRAY, WHISKER_VALUE_MAP, WHISKER_VALUE_ERROR,
} WhiskerValueType;
typedef struct { const char* ptr; size_t len; } WhiskerStringRef;
typedef struct { const uint8_t* ptr; size_t len; } WhiskerBytesRef;
typedef struct WhiskerValueRec WhiskerValueRaw;
typedef struct WhiskerKeyValueRec WhiskerKeyValueRaw;
typedef struct { WhiskerValueRaw* items; size_t count; } WhiskerValueArray;
typedef struct { WhiskerKeyValueRaw* entries; size_t count; } WhiskerValueMap;
struct WhiskerValueRec {
  uint8_t type; uint8_t _pad[7];
  union {
    bool b; int64_t i; double f; WhiskerStringRef s; WhiskerBytesRef bytes;
    WhiskerValueArray array; WhiskerValueMap map;
  } v;
};
struct WhiskerKeyValueRec { WhiskerStringRef key; WhiskerValueRaw value; };

#endif
