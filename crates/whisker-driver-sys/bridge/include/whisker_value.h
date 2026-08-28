#ifndef WHISKER_VALUE_H_
#define WHISKER_VALUE_H_

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef enum {
  WHISKER_VALUE_NULL = 0,
  WHISKER_VALUE_BOOL = 1,
  WHISKER_VALUE_INT = 2,
  WHISKER_VALUE_FLOAT = 3,
  WHISKER_VALUE_STRING = 4,
  WHISKER_VALUE_BYTES = 5,
  WHISKER_VALUE_ARRAY = 6,
  WHISKER_VALUE_MAP = 7,
  WHISKER_VALUE_ERROR = 8,
} WhiskerValueType;

typedef struct { const char* ptr; size_t len; } WhiskerStringRef;
typedef struct { const uint8_t* ptr; size_t len; } WhiskerBytesRef;

typedef struct WhiskerValueRec WhiskerValueRaw;
typedef struct WhiskerKeyValueRec WhiskerKeyValueRaw;
typedef struct { WhiskerValueRaw* items; size_t count; } WhiskerValueArray;
typedef struct { WhiskerKeyValueRaw* entries; size_t count; } WhiskerValueMap;

struct WhiskerValueRec {
  uint8_t type;
  uint8_t _pad[7];
  union {
    bool b;
    int64_t i;
    double f;
    WhiskerStringRef s;
    WhiskerBytesRef bytes;
    WhiskerValueArray array;
    WhiskerValueMap map;
  } v;
};

struct WhiskerKeyValueRec {
  WhiskerStringRef key;
  WhiskerValueRaw value;
};

#endif
