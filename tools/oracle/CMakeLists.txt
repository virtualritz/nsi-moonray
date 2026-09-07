# Copyright 2026 Moritz Moeller
# SPDX-License-Identifier: MIT OR Apache-2.0 OR Zlib

cmake_minimum_required(VERSION 3.23)
project(nsi_moonray_oracle LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Point this at an installed or in-tree `scene_rdl2`. See
# `tools/oracle/README.md` for the build recipe that produces one.
if(NOT DEFINED SCENE_RDL2_ROOT)
    message(FATAL_ERROR "Set -DSCENE_RDL2_ROOT to a scene_rdl2 install prefix")
endif()

find_package(Lua REQUIRED)

add_executable(oracle oracle.cc)

target_include_directories(oracle PRIVATE ${SCENE_RDL2_ROOT}/include ${LUA_INCLUDE_DIR})

# `scene_rdl2` headers use `__cdecl` and the `PLATFORM_*` switches its own
# build passes on the command line; a consumer has to pass them too, or
# `rdl2/Types.h` does not parse. Found by reading the flags the library
# compiles itself with, not by guessing.
target_compile_definitions(oracle PRIVATE __cdecl= PLATFORM_UNIX PLATFORM_LINUX __AVX__)
target_compile_options(oracle PRIVATE -mavx)
target_link_directories(oracle PRIVATE ${SCENE_RDL2_ROOT}/lib ${SCENE_RDL2_ROOT}/lib64)
target_link_libraries(oracle PRIVATE scene_rdl2 common_math common_platform render_logging render_util)
set_target_properties(oracle PROPERTIES
    BUILD_RPATH "${SCENE_RDL2_ROOT}/lib;${SCENE_RDL2_ROOT}/lib64")
