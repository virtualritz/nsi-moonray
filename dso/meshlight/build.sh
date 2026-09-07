#!/bin/sh
# Build the `DwaBaseMaterial` stand-in.
#
# Not OSL's, and built whether or not OSL is: `MeshLight` is how an
# emissive object lights a scene, and without this class no scene
# containing one renders at all. See `attributes.cc`.
#
# Usage: build.sh [moonray-install-root] [output-dir]

set -e
moonray=${1:-${MOONRAY_ROOT:-/home/user/mr/install}}
here=$(cd "$(dirname "$0")" && pwd)
out=${2:-$here/build}

mkdir -p "$out"

flags="-std=c++17 -O2 -fPIC -shared -mavx
    -D__cdecl= -DPLATFORM_UNIX -DPLATFORM_LINUX -D__AVX__
    -Wno-strict-aliasing -Wno-unused-parameter -Wno-deprecated-declarations
    -I$here -I$moonray/include -I/usr/include/lua5.3"

# shellcheck disable=SC2086
g++ $flags "$here/DwaBaseMaterial.cc" -o "$out/DwaBaseMaterial.so" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2 -lrendering_shading

# shellcheck disable=SC2086
g++ $flags "$here/attributes.cc" -o "$out/DwaBaseMaterial.so.proxy" \
    -L"$moonray/lib64" -L"$moonray/lib" -lscene_rdl2

echo "built $out/DwaBaseMaterial.so and .so.proxy"
