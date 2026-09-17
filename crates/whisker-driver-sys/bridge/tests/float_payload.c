// Regression test for empty JNI collection payloads. On macOS, compile with:
// clang -std=c11 -ffunction-sections -Wl,-dead_strip -I<JNICALL_HEADERS>
//   -Ibridge/include bridge/tests/float_payload.c -o /tmp/float-payload-test
// JNICALL_HEADERS should contain the NDK jni.h only, so Android libc headers
// do not replace the host compiler's standard headers. Then run the executable.
#define __ANDROID__ 1
#include "../src/whisker_mobile_android.c"
#include <assert.h>
static int allocations, copies;
static jsize last_size;
static jfloatArray make_array(JNIEnv *env, jsize count) { allocations++; last_size=count; return (jfloatArray)(uintptr_t)1; }
static void copy_array(JNIEnv *env, jfloatArray a, jsize start, jsize count, const jfloat *src) { assert(src != NULL); copies++; }
int main(void) {
 struct JNINativeInterface table = {0}; table.NewFloatArray=make_array; table.SetFloatArrayRegion=copy_array;
 JNIEnv env=&table;
 assert(floats(&env, NULL, 0) != NULL); assert(allocations==1 && last_size==0 && copies==0);
 assert(floats(&env, NULL, 1) == NULL); assert(allocations==1);
 float value=1.0f; assert(floats(&env,&value,1)!=NULL); assert(last_size==1 && copies==1);
 return 0;
}
