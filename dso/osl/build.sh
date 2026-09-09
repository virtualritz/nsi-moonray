#!/bin/sh
# Build the `Osl` material, `OslDisplacement` and `OslMap` DSOs.
#
# Compiled directly rather than through MoonRay's `moonray_dso_simple`,
# which lives in MoonRay's build tree and whose CMake config pulls in a
# CppUnit that is not needed here. The flags are the ones `build.rs`
# uses for the shim: rdl2's headers assume them.
#
# Usage: build.sh [moonray-install-root] [osl-install-root] [output-dir]

set -e
moonray=${1:-${MOONRAY_ROOT:-/home/user/mr/install}}
osl=${2:-${OSL_ROOT:-/home/user/osl/install}}
here=$(cd "$(dirname "$0")" && pwd)
out=${3:-$here/build}

mkdir -p "$out"

flags="-std=c++17 -O2 -fPIC -shared -mavx
    -D__cdecl= -DPLATFORM_UNIX -DPLATFORM_LINUX -D__AVX__
    -Wno-strict-aliasing -Wno-unused-parameter -Wno-deprecated-declarations
    -I$here -I$moonray/include -I$osl/include -I/usr/include/lua5.3"

# shellcheck disable=SC2086
g++ $flags "$here/Osl.cc" "$here/shading_system.cc" -o "$out/Osl.so" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2 -lrendering_shading \
    -L"$osl/lib" -loslexec -lOpenImageIO -lOpenImageIO_Util \
    -Wl,-rpath,"$osl/lib"

# The proxy carries the attribute declarations alone, which is what
# rdl2 reads when it only needs the class's shape.
# shellcheck disable=SC2086
g++ $flags -DNSI_MOONRAY_OSL_ROOT=rdl2::Material -DNSI_MOONRAY_OSL_LABELS \
    "$here/attributes.cc" -o "$out/Osl.so.proxy" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2

# The displacement root shader, from the same shading system.
# shellcheck disable=SC2086
g++ $flags "$here/OslDisplacement.cc" "$here/shading_system.cc" \
    -o "$out/OslDisplacement.so" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2 -lrendering_shading \
    -L"$osl/lib" -loslexec -lOpenImageIO -lOpenImageIO_Util \
    -Wl,-rpath,"$osl/lib"

# shellcheck disable=SC2086
g++ $flags -DNSI_MOONRAY_OSL_ROOT=rdl2::Displacement \
    "$here/attributes.cc" -o "$out/OslDisplacement.so.proxy" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2

# The emission of a network, for a `MeshLight` to sample. This is what
# makes an OSL light light the scene rather than merely look bright.
# shellcheck disable=SC2086
g++ $flags "$here/OslMap.cc" "$here/shading_system.cc" \
    -o "$out/OslMap.so" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2 -lrendering_shading \
    -L"$osl/lib" -loslexec -lOpenImageIO -lOpenImageIO_Util \
    -Wl,-rpath,"$osl/lib"

# shellcheck disable=SC2086
g++ $flags -DNSI_MOONRAY_OSL_ROOT=rdl2::Map \
    "$here/attributes.cc" -o "$out/OslMap.so.proxy" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2

# The volume root shader. Four virtuals over one execution; see the
# note at the top of `OslVolume.cc`.
# shellcheck disable=SC2086
g++ $flags "$here/OslVolume.cc" "$here/shading_system.cc" \
    -o "$out/OslVolume.so" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2 -lrendering_shading \
    -L"$osl/lib" -loslexec -lOpenImageIO -lOpenImageIO_Util \
    -Wl,-rpath,"$osl/lib"

# shellcheck disable=SC2086
g++ $flags -DNSI_MOONRAY_OSL_ROOT=rdl2::VolumeShader \
    "$here/attributes.cc" -o "$out/OslVolume.so.proxy" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2

echo "built $out/Osl.so, $out/OslDisplacement.so, $out/OslMap.so, \
$out/OslVolume.so and their proxies"
