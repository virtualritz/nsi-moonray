# The C shim over `scene_rdl2`, built standalone.
#
# Cargo drives this through `build.rs` for the crate; this file is what
# lets the shim be built and smoke-tested on its own, the way
# `tools/oracle` is -- which is how the calls below were checked against
# the real library rather than against the headers alone.
#
#   cmake -S shim -B build-shim -DSCENE_RDL2_ROOT=/path/to/install
#   cmake --build build-shim
#   ./build-shim/shim_smoke /path/to/build-rdl2/tests/lib/scene/rdl2 out.rdla

cmake_minimum_required(VERSION 3.16)
project(nsi_moonray_shim CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED SCENE_RDL2_ROOT)
    message(FATAL_ERROR "set -DSCENE_RDL2_ROOT=<install prefix>")
endif()

find_package(Lua REQUIRED)

add_library(nsi_moonray_shim STATIC src/scene.cc)

target_include_directories(nsi_moonray_shim PUBLIC include)
target_include_directories(nsi_moonray_shim PRIVATE
    ${SCENE_RDL2_ROOT}/include ${LUA_INCLUDE_DIR})

# `rdl2/Types.h` does not parse without the definitions rdl2's own
# build passes on the command line -- it dies at the first `__cdecl`
# function typedef. See `quickstart.md`.
target_compile_definitions(nsi_moonray_shim PRIVATE
    __cdecl= PLATFORM_UNIX PLATFORM_LINUX __AVX__)
target_compile_options(nsi_moonray_shim PRIVATE -mavx)

target_link_directories(nsi_moonray_shim PUBLIC ${SCENE_RDL2_ROOT}/lib)
# `render_logging` is needed explicitly: rdl2 calls `Logger::logFatal`
# and the linker will not pull it in transitively.
target_link_libraries(nsi_moonray_shim PUBLIC scene_rdl2 render_logging)

add_executable(shim_smoke tests/smoke.cc)
target_link_libraries(shim_smoke PRIVATE nsi_moonray_shim)
set_target_properties(shim_smoke PROPERTIES
    BUILD_RPATH ${SCENE_RDL2_ROOT}/lib)
