cmake_minimum_required(VERSION 3.22)
project(lyra_bridge LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN YES)

add_library(lyra_bridge SHARED
    src/lyra_bridge.cc
)

target_include_directories(lyra_bridge PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

# TODO: link against Lynx prebuilt (libLynx.so / Lynx.framework).
# TODO: include Lynx headers from the prebuilt distribution.

set_target_properties(lyra_bridge PROPERTIES
    PUBLIC_HEADER ${CMAKE_CURRENT_SOURCE_DIR}/include/lyra_bridge.h
)
