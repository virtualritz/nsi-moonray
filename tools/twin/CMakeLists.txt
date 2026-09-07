# Authoring twins of MoonRay's geometry DSOs.
#
# Declarations only, built against `scene_rdl2` alone, so a mesh scene
# can be built and read back on a host that has not built MoonRay.
#
#   cmake -S tools/twin -B build-twin \
#       -DSCENE_RDL2_ROOT=$PREFIX -DMOONRAY_SOURCE=/path/to/moonray
#   cmake --build build-twin
#
# `MOONRAY_SOURCE` is a *source* checkout, not an install: the
# attribute declarations are compiled out of it verbatim so the twins
# cannot drift from what the renderer declares.

cmake_minimum_required(VERSION 3.16)
project(nsi_moonray_twin CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED SCENE_RDL2_ROOT)
    message(FATAL_ERROR "set -DSCENE_RDL2_ROOT=<scene_rdl2 install prefix>")
endif()
if(NOT DEFINED MOONRAY_SOURCE)
    message(FATAL_ERROR
        "set -DMOONRAY_SOURCE=<moonray source checkout>; the attribute "
        "declarations are compiled out of it rather than copied")
endif()

find_package(Lua REQUIRED)

function(add_twin name relative_dir)
    add_library(${name} MODULE ${name}.cc)
    # rdl2 loads a DSO as `<class>.so`, with no `lib` prefix.
    set_target_properties(${name} PROPERTIES PREFIX "" SUFFIX ".so")
    target_include_directories(${name} PRIVATE
        # First, so `#include "attributes.cc"` finds MoonRay's.
        ${MOONRAY_SOURCE}/${relative_dir}
        ${SCENE_RDL2_ROOT}/include
        ${LUA_INCLUDE_DIR})
    # Without these `rdl2/Types.h` does not parse at all; see
    # `quickstart.md`.
    target_compile_definitions(${name} PRIVATE
        __cdecl= PLATFORM_UNIX PLATFORM_LINUX __AVX__)
    target_compile_options(${name} PRIVATE -mavx)
    target_link_directories(${name} PRIVATE ${SCENE_RDL2_ROOT}/lib)
    target_link_libraries(${name} PRIVATE scene_rdl2 render_logging)
endfunction()

add_twin(RdlMeshGeometry dso/geometry/RdlMesh)
add_twin(RdlInstancerGeometry dso/geometry/RdlInstancerGeometry)
add_twin(PerspectiveCamera dso/camera/PerspectiveCamera)
add_twin(EnvLight dso/light/EnvLight)

# `UsdPreviewSurface` has **no twin**, and cannot have one built this
# way. Its `attributes.cc` does not exist in the source tree: it is
# generated from `UsdPreviewSurface.ispc` by `moonray_ispc_dso`, so
# building a twin from it would need a MoonRay build -- which is the
# thing these exist to avoid needing.
#
# Declaring the six parameters this backend carries by hand instead is
# exactly the copy that drifts, and would be worse than the gap: a
# renamed parameter would keep working here and be silently ignored by
# the real material. A scene checked with these twins therefore reports
# `UsdPreviewSurface` as an unknown class, which is honest.
