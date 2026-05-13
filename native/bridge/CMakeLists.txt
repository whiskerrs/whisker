cmake_minimum_required(VERSION 3.22)
project(tuft_bridge LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN YES)

add_library(tuft_bridge SHARED
    src/tuft_bridge.cc
)

target_include_directories(tuft_bridge PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

# TODO: link against Lynx prebuilt (libLynx.so / Lynx.framework).
# TODO: include Lynx headers from the prebuilt distribution.

set_target_properties(tuft_bridge PROPERTIES
    PUBLIC_HEADER ${CMAKE_CURRENT_SOURCE_DIR}/include/tuft_bridge.h
)
