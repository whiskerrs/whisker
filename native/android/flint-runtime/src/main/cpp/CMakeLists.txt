cmake_minimum_required(VERSION 3.22.1)
project(flint_jni LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# JNI shim that bridges Kotlin <-> Rust runtime <-> flint_bridge.
add_library(flint_runtime SHARED
    jni_flint.cc
)

# native/bridge headers — the bridge .so is built separately and shipped
# alongside this AAR.
target_include_directories(flint_runtime PRIVATE
    ${CMAKE_SOURCE_DIR}/../../../../bridge/include
)

find_library(log-lib log)
target_link_libraries(flint_runtime PRIVATE ${log-lib})
