#!/bin/sh
# Build the probe without CMake.
#
# MoonRay's own `moonray_dso_simple` needs its build tree -- and, on
# this container, a CppUnit its config asks for and nobody has. The DSO
# itself is two translation units and a handful of flags, so this
# compiles them directly. The flags are the ones `build.rs` uses for
# the shim, for the same reason: rdl2's headers assume them.
#
# Usage: build.sh [install-root] [output-directory]

set -e
root=${1:-/home/user/mr/install}
out=${2:-$(dirname "$0")/build}
here=$(cd "$(dirname "$0")" && pwd)

mkdir -p "$out"

flags="-std=c++17 -O2 -fPIC -shared -mavx
    -D__cdecl= -DPLATFORM_UNIX -DPLATFORM_LINUX -D__AVX__
    -Wno-strict-aliasing -Wno-unused-parameter -Wno-deprecated-declarations
    -I$here -I$root/include -I/usr/include/lua5.3"

# shellcheck disable=SC2086
g++ $flags "$here/ScalarProbe.cc" -o "$out/ScalarProbe.so" \
    -L"$root/lib64" -L"$root/lib" -lscene_rdl2 -lrendering_shading

# shellcheck disable=SC2086
g++ $flags "$here/attributes.cc" -o "$out/ScalarProbe.so.proxy" \
    -L"$root/lib64" -L"$root/lib" -lscene_rdl2

echo "built $out/ScalarProbe.so and .so.proxy"
