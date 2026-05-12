cmake_minimum_required(VERSION 3.22.1)
project(lyra_jni LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# JNI shim that bridges Kotlin <-> Rust runtime <-> lyra_bridge.
add_library(lyra_runtime SHARED
    jni_lyra.cc
)

# native/bridge headers — the bridge .so is built separately and shipped
# alongside this AAR.
target_include_directories(lyra_runtime PRIVATE
    ${CMAKE_SOURCE_DIR}/../../../../bridge/include
)

find_library(log-lib log)
target_link_libraries(lyra_runtime PRIVATE ${log-lib})
